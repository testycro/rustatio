use super::instance::FakerInstance;
use super::lifecycle::InstanceLifecycle;
use super::state::AppState;
use rustatio_core::{FakerState, FakerStats, RatioFakerHandle};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

pub struct Scheduler {
    shutdown_tx: Option<mpsc::Sender<()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self { shutdown_tx: None, task_handle: None }
    }

    pub fn start(
        &mut self,
        state: AppState,
        instances: Arc<RwLock<HashMap<String, FakerInstance>>>,
    ) {
        if self.task_handle.is_some() {
            return;
        }

        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let handle = tokio::spawn(scheduler_loop(state, instances, shutdown_rx));

        self.shutdown_tx = Some(shutdown_tx);
        self.task_handle = Some(handle);

        tracing::info!("Centralized scheduler started");
    }

    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        if let Some(handle) = self.task_handle.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
        tracing::info!("Centralized scheduler stopped");
    }
}

/// How long a tick waits for the updates it started. Most finish at once; an update waiting
/// for a tracker (announce or retry, up to the request timeout) is collected at a later tick
/// and never holds up the other instances.
const COLLECT_WINDOW: Duration = Duration::from_secs(3);

async fn scheduler_loop(
    state: AppState,
    instances: Arc<RwLock<HashMap<String, FakerInstance>>>,
    mut shutdown_rx: mpsc::Receiver<()>,
) {
    let update_interval = Duration::from_secs(5);
    let save_interval = Duration::from_secs(30);
    let mut last_save = std::time::Instant::now();
    let mut updates = PendingUpdates::new();

    tracing::info!("Scheduler loop started");

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("Scheduler received shutdown signal");
                break;
            }
            () = tokio::time::sleep(update_interval) => {
                updates.spawn_due(&state, &instances).await;
                let (dirty, updated) = updates.collect(COLLECT_WINDOW).await;

                if !updated.is_empty() {
                    state.emit_instance_summaries(&updated).await;
                }

                if dirty {
                    if let Err(e) = state.save_state().await {
                        tracing::warn!("Scheduler: failed to save state after runtime change: {}", e);
                    }
                }

                if last_save.elapsed() >= save_interval {
                    if let Err(e) = state.save_state().await {
                        tracing::warn!("Scheduler: failed to save state: {}", e);
                    }
                    last_save = std::time::Instant::now();
                }
            }
        }
    }

    tracing::info!("Scheduler loop stopped");
}

/// Result of one update: `None` when it failed, else whether the persisted state changed.
type UpdateDone = (String, Option<bool>);

/// Instance updates run as their own tasks. An instance whose previous update is still running
/// (tracker not answering) is skipped until it is done.
struct PendingUpdates {
    in_flight: HashSet<String>,
    done_tx: mpsc::UnboundedSender<UpdateDone>,
    done_rx: mpsc::UnboundedReceiver<UpdateDone>,
}

impl PendingUpdates {
    fn new() -> Self {
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        Self { in_flight: HashSet::new(), done_tx, done_rx }
    }

    async fn spawn_due(
        &mut self,
        state: &AppState,
        instances: &Arc<RwLock<HashMap<String, FakerInstance>>>,
    ) {
        let items: Vec<(String, Arc<RatioFakerHandle>)> = {
            let guard = instances.read().await;
            guard.iter().map(|(id, inst)| (id.clone(), Arc::clone(&inst.faker))).collect()
        };

        for (id, faker) in items {
            if self.in_flight.contains(&id) {
                continue;
            }

            let before = faker.stats_snapshot();
            let should_update = matches!(before.state, FakerState::Running);
            let should_retry = matches!(before.state, FakerState::Stopped)
                && before.tracker_error.as_deref() == Some("Tracker unavailable")
                && faker.tracker_retry_due_now().await;

            if !should_update && !should_retry {
                continue;
            }

            self.in_flight.insert(id.clone());
            let job = UpdateJob { id, faker, before, retry: should_retry };
            let state = state.clone();
            let instances = Arc::clone(instances);
            let done_tx = self.done_tx.clone();
            tokio::spawn(async move {
                let changed = update_instance(&state, &instances, &job).await;
                let _ = done_tx.send((job.id, changed));
            });
        }
    }

    /// Updates finished so far (waiting at most `window` for the ones still running).
    async fn collect(&mut self, window: Duration) -> (bool, Vec<String>) {
        let deadline = tokio::time::Instant::now() + window;
        let mut dirty = false;
        let mut updated = Vec::new();

        while !self.in_flight.is_empty() {
            let Ok(Some((id, changed))) =
                tokio::time::timeout_at(deadline, self.done_rx.recv()).await
            else {
                break;
            };
            self.in_flight.remove(&id);
            if let Some(changed) = changed {
                dirty |= changed;
                updated.push(id);
            }
        }

        (dirty, updated)
    }
}

struct UpdateJob {
    id: String,
    faker: Arc<RatioFakerHandle>,
    /// Stats when the update was decided
    before: FakerStats,
    /// Tracker recovery attempt instead of a regular update
    retry: bool,
}

async fn update_instance(
    state: &AppState,
    instances: &Arc<RwLock<HashMap<String, FakerInstance>>>,
    job: &UpdateJob,
) -> Option<bool> {
    let UpdateJob { id, faker, before, retry: should_retry } = job;
    let should_retry = *should_retry;
    let label = {
        let guard = instances.read().await;
        guard
            .get(id)
            .map(|instance| instance.summary.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| id.clone())
    };
    let result = rustatio_core::logger::with_instance_context(label, async {
        if should_retry {
            state.recover_tracker_instance(id).await.map(|_| ())
        } else {
            faker.update().await.map_err(|e| e.to_string())
        }
    })
    .await;
    if let Err(e) = result {
        let action = if should_retry { "tracker recovery" } else { "update" };
        tracing::warn!("Scheduler: {} failed for instance {}: {}", action, id, e);
        return None;
    }

    let after = faker.stats_snapshot();
    {
        let mut guard = instances.write().await;
        if let Some(instance) = guard.get_mut(id) {
            instance.cumulative_uploaded = after.uploaded;
            instance.cumulative_downloaded = after.downloaded;
            instance.config.completion_percent = after.torrent_completion;
        }
    }

    Some(
        std::mem::discriminant(&after.state) != std::mem::discriminant(&before.state)
            || after.stop_condition_met != before.stop_condition_met
            || after.is_idling != before.is_idling
            || after.tracker_error != before.tracker_error
            || after.tracker_retry_attempt != before.tracker_retry_attempt
            || after.tracker_retry_at_ms != before.tracker_retry_at_ms,
    )
}
