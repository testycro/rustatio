use crate::state::{build_summary, AppState};
use rustatio_core::logger::with_instance_context;
use rustatio_core::{FakerState, FakerStats, InstanceSummary, PostStopAction, RatioFakerHandle};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

const TICK: Duration = Duration::from_secs(5);
/// How long a tick waits for the updates it started. Most finish at once; an update waiting
/// for a tracker (announce or retry, up to the request timeout) is collected at a later tick
/// and never holds up the other instances.
const COLLECT_WINDOW: Duration = Duration::from_secs(3);

/// Result of one update: the stats before and after it, `None` when nothing was done.
type UpdateDone = (u32, Option<(FakerStats, FakerStats)>);

/// Instance updates run as their own tasks. An instance whose previous update is still running
/// (tracker not answering) is skipped until it is done.
struct PendingUpdates {
    /// Instances being updated, with their update interval
    in_flight: HashMap<u32, Duration>,
    done_tx: mpsc::UnboundedSender<UpdateDone>,
    done_rx: mpsc::UnboundedReceiver<UpdateDone>,
}

impl PendingUpdates {
    fn new() -> Self {
        let (done_tx, done_rx) = mpsc::unbounded_channel();
        Self { in_flight: HashMap::new(), done_tx, done_rx }
    }

    fn spawn(&mut self, id: u32, faker: Arc<RatioFakerHandle>, name: String, interval: Duration) {
        self.in_flight.insert(id, interval);
        let done_tx = self.done_tx.clone();
        tauri::async_runtime::spawn(async move {
            let result = run(id, &faker, &name).await;
            let _ = done_tx.send((id, result));
        });
    }

    /// Updates finished so far (waiting at most `window` for the ones still running).
    async fn collect(&mut self, window: Duration) -> Vec<(u32, Duration, FakerStats, FakerStats)> {
        let deadline = tokio::time::Instant::now() + window;
        let mut done = Vec::new();

        while !self.in_flight.is_empty() {
            let Ok(Some((id, result))) =
                tokio::time::timeout_at(deadline, self.done_rx.recv()).await
            else {
                break;
            };
            let interval = self.in_flight.remove(&id).unwrap_or(TICK);
            if let Some((before, after)) = result {
                done.push((id, interval, before, after));
            }
        }

        done
    }
}

fn should_update(state: FakerState, tracker_error: Option<&str>, retry_due: bool) -> bool {
    match state {
        FakerState::Running => true,
        FakerState::Stopped if tracker_error == Some("Tracker unavailable") => retry_due,
        _ => false,
    }
}

fn should_delete(stats: &FakerStats) -> bool {
    stats.stop_condition_met && stats.post_stop_action == PostStopAction::DeleteInstance
}

pub fn spawn(state: AppState, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(TICK);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut due = HashMap::new();
        let mut updates = PendingUpdates::new();

        loop {
            timer.tick().await;
            tick(&state, &app, &mut due, &mut updates).await;
        }
    });
}

async fn tick(
    state: &AppState,
    app: &AppHandle,
    due: &mut HashMap<u32, Instant>,
    updates: &mut PendingUpdates,
) {
    let mut dirty = false;
    let mut updated = Vec::new();
    let mut deleted = Vec::new();
    let now = Instant::now();

    let instances = snapshot(state).await;
    let ids: HashSet<u32> = instances.iter().map(|(id, _, _, _, _)| *id).collect();

    for (id, faker, name, interval, retry) in instances {
        if updates.in_flight.contains_key(&id) {
            continue;
        }
        let interval = Duration::from_secs(interval.max(1));
        let next = *due.entry(id).or_insert(now + interval);
        if !retry && now < next {
            continue;
        }

        updates.spawn(id, faker, name, interval);
    }

    // An update that waited for its tracker is collected at a later tick
    for (id, interval, before, after) in updates.collect(COLLECT_WINDOW).await {
        due.insert(id, now + interval);

        if should_delete(&after) {
            delete(state, id).await;
            due.remove(&id);
            deleted.push(id);
            dirty = true;
            continue;
        }

        store(state, id, &after).await;
        dirty |= changed(&before, &after);
        updated.push(id);
    }

    due.retain(|id, _| ids.contains(id));
    publish(state, app, dirty, &updated, &deleted).await;
}

async fn snapshot(state: &AppState) -> Vec<(u32, Arc<RatioFakerHandle>, String, u64, bool)> {
    let fakers = state.fakers.read().await;
    fakers
        .iter()
        .map(|(id, instance)| {
            let name = if instance.summary.name.is_empty() {
                id.to_string()
            } else {
                instance.summary.name.clone()
            };
            let retry = instance.faker.with_stats(|stats| {
                matches!(stats.state, FakerState::Stopped)
                    && stats.tracker_error.as_deref() == Some("Tracker unavailable")
            });
            (*id, Arc::clone(&instance.faker), name, instance.config.update_interval_seconds, retry)
        })
        .collect()
}

async fn run(id: u32, faker: &RatioFakerHandle, name: &str) -> Option<(FakerStats, FakerStats)> {
    let before = faker.stats_snapshot();
    let retry = retry_due(&before, faker).await;

    if !should_update(before.state, before.tracker_error.as_deref(), retry) {
        return None;
    }

    // The context follows the task (updates now run side by side, a thread-local value set
    // before an await would be read by the others)
    let result = with_instance_context(name, async {
        if retry {
            faker.recover_tracker().await.map(|_| ()).map_err(|error| error.to_string())
        } else {
            faker.update().await.map_err(|error| error.to_string())
        }
    })
    .await;

    if let Err(error) = result {
        log::warn!("Scheduler update failed for instance {id}: {error}");
        return None;
    }

    Some((before, faker.stats_snapshot()))
}

async fn retry_due(stats: &FakerStats, faker: &RatioFakerHandle) -> bool {
    matches!(stats.state, FakerState::Stopped)
        && stats.tracker_error.as_deref() == Some("Tracker unavailable")
        && faker.tracker_retry_due_now().await
}

async fn delete(state: &AppState, id: u32) {
    let removed = state.fakers.write().await.remove(&id);
    let Some(instance) = removed else {
        return;
    };

    if matches!(instance.source, rustatio_watch::InstanceSource::WatchFolder) {
        remove_hash(state, instance.summary.info_hash).await;
    }

    if let Err(error) = instance.faker.stop().await {
        log::warn!("Scheduler failed to stop deleted instance {id}: {error}");
    }
}

async fn remove_hash(state: &AppState, hash: [u8; 20]) {
    let guard = state.watch.read().await;
    if let Some(watch) = guard.as_ref() {
        watch.remove_info_hash(&hash).await;
    }
}

async fn store(state: &AppState, id: u32, stats: &FakerStats) {
    let mut fakers = state.fakers.write().await;
    if let Some(instance) = fakers.get_mut(&id) {
        instance.cumulative_uploaded = stats.uploaded;
        instance.cumulative_downloaded = stats.downloaded;
        instance.config.completion_percent = stats.torrent_completion;
    }
}

fn changed(before: &FakerStats, after: &FakerStats) -> bool {
    std::mem::discriminant(&after.state) != std::mem::discriminant(&before.state)
        || after.stop_condition_met != before.stop_condition_met
        || after.is_idling != before.is_idling
        || after.tracker_error != before.tracker_error
        || after.tracker_retry_attempt != before.tracker_retry_attempt
        || after.tracker_retry_at_ms != before.tracker_retry_at_ms
}

async fn summaries(state: &AppState, ids: &[u32]) -> Vec<InstanceSummary> {
    let fakers = state.fakers.read().await;
    ids.iter()
        .filter_map(|id| {
            fakers.get(id).map(|instance| {
                instance.faker.with_stats(|stats| build_summary(*id, instance, stats))
            })
        })
        .collect()
}

async fn publish(state: &AppState, app: &AppHandle, dirty: bool, updated: &[u32], deleted: &[u32]) {
    if !updated.is_empty() {
        let list = summaries(state, updated).await;
        if let Err(error) = app.emit("instance-summaries", list) {
            log::warn!("Failed to emit instance summaries: {error}");
        }
    }

    for id in deleted {
        if let Err(error) = app.emit("instance-deleted", id.to_string()) {
            log::warn!("Failed to emit deleted instance {id}: {error}");
        }
    }

    if !deleted.is_empty() {
        state.refresh_peer_listener_port().await;
    }

    if dirty {
        if let Err(error) = state.save_state().await {
            log::warn!("Scheduler failed to save state: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_running_or_due_tracker_retries_are_updated() {
        assert!(should_update(FakerState::Running, None, false));
        assert!(should_update(FakerState::Stopped, Some("Tracker unavailable"), true));
        assert!(!should_update(FakerState::Stopped, Some("Tracker unavailable"), false));
        assert!(!should_update(FakerState::Stopped, Some("Other error"), true));
        assert!(!should_update(FakerState::Paused, None, false));
        assert!(!should_update(FakerState::Idle, None, false));
    }
}
