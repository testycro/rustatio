//! The admin daemon (`scripts/rustatio_daemon.py`) embedded in the desktop app.
//!
//! The Python script is compiled into a standalone executable (`PyInstaller`, see
//! `scripts/build_daemon_sidecar.py`) and bundled as a Tauri sidecar, so nothing else has to be
//! installed. At startup the app:
//!   1. starts a loopback HTTP bridge ([`bridge`]) that gives the daemon the REST API it expects,
//!   2. launches the daemon on a random loopback port and restarts it when it exits,
//!   3. proxies the Daemon view requests to the daemon admin panel ([`daemon_request`]).
//!
//! The daemon only starts with the app when the user turned "autostart" on (off by default), and
//! can be started and stopped from the Daemon view. Its options (autostart, variables applied
//! from the Daemon view) are kept in the state file with the other desktop options. It never outlives the app: the app keeps a
//! pipe open on its input, which the daemon watches, and the daemon also checks that the app
//! process still exists.
//!
//! Without a compiled daemon (plain `cargo run`, CI checks) the app works as before and the
//! Daemon view reports that the daemon is not available.

mod bridge;
mod support;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rustatio_core::reqwest::{Client, Method};
use serde::Serialize;
use serde_json::Value;
use tauri::async_runtime::Receiver;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::process::{Command, CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

use crate::persistence::DaemonSettings;
use crate::state::AppState;
use support::{
    admin_url, daemon_paths, force_kill_command, is_real_sidecar, is_saveable_env_key,
    legacy_autostart, needs_default_rules, next_failure_count, sidecar_env, DaemonPaths,
    SidecarEnv, APP_HELLO,
};

/// Exit code of the daemon when it asks to be restarted (admin panel "restart").
const RESTART_EXIT_CODE: i32 = 42;
/// Pause before relaunching a daemon that stopped on its own (same as the Docker entrypoint).
const RESTART_DELAY: Duration = Duration::from_secs(3);
/// Failed launches in a row before giving up.
const MAX_FAILED_LAUNCHES: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a daemon gets to exit by itself once the app closed its input, before it is killed.
const GRACEFUL_STOP: Duration = Duration::from_secs(3);
const STOP_POLL: Duration = Duration::from_millis(50);
/// Time to accept a connection on the admin port, to tell that the panel is up.
const READY_PROBE: Duration = Duration::from_millis(300);

/// Rules of the Docker image, the starting point of the rules file (editable from the panel).
const DEFAULT_RULES: &str = include_str!("../../../scripts/rules.txt");

/// Development: run this script with Python instead of the bundled executable.
const SCRIPT_OVERRIDE_ENV: &str = "RUSTATIO_DAEMON_SCRIPT";
const PYTHON_OVERRIDE_ENV: &str = "RUSTATIO_DAEMON_PYTHON";

pub struct DaemonState {
    token: String,
    client: Client,
    /// Port of the daemon admin panel, 0 while the daemon is not running.
    admin_port: AtomicU16,
    /// The app is exiting: nothing may be started any more.
    stopping: AtomicBool,
    /// A compiled daemon is bundled with this build.
    available: AtomicBool,
    /// The daemon should be running (started by the user or by the autostart).
    wanted: AtomicBool,
    /// The supervisor task is alive.
    supervising: AtomicBool,
    /// Process id of the running daemon, 0 when there is none.
    child_pid: AtomicU32,
    child: Mutex<Option<CommandChild>>,
    bridge: Mutex<Option<bridge::BridgeHandle>>,
    /// Start and stop never run at the same time.
    lifecycle: tokio::sync::Mutex<()>,
}

impl Default for DaemonState {
    fn default() -> Self {
        let client =
            Client::builder().no_proxy().timeout(REQUEST_TIMEOUT).build().unwrap_or_default();

        Self {
            token: support::hex(&rand::random::<[u8; 32]>()),
            client,
            admin_port: AtomicU16::new(0),
            stopping: AtomicBool::new(false),
            available: AtomicBool::new(false),
            wanted: AtomicBool::new(false),
            supervising: AtomicBool::new(false),
            child_pid: AtomicU32::new(0),
            child: Mutex::new(None),
            bridge: Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        }
    }
}

impl DaemonState {
    fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    fn is_wanted(&self) -> bool {
        self.wanted.load(Ordering::Acquire) && !self.is_stopping()
    }

    /// Keeps the handle of the running daemon so that it can be stopped.
    fn register(&self, child: CommandChild) {
        self.child_pid.store(child.pid(), Ordering::Release);
        if let Ok(mut guard) = self.child.lock() {
            *guard = Some(child);
        }
        // A stop may have been asked while the process was starting
        if !self.is_wanted() {
            self.release_child();
        }
    }

    /// Lets the daemon go: dropping the handle closes the pipe the daemon watches, and it exits.
    fn release_child(&self) {
        let child = self.child.lock().ok().and_then(|mut guard| guard.take());
        drop(child);
    }

    fn has_exited(&self) -> bool {
        self.child_pid.load(Ordering::Acquire) == 0
    }

    /// Stops the daemon: asks it to exit, then kills it if it did not within [`GRACEFUL_STOP`].
    /// Blocks, so it also works while the app is closing.
    fn terminate_blocking(&self) {
        self.admin_port.store(0, Ordering::Release);
        let pid = self.child_pid.load(Ordering::Acquire);
        self.release_child();

        let deadline = Instant::now() + GRACEFUL_STOP;
        while !self.has_exited() && Instant::now() < deadline {
            std::thread::sleep(STOP_POLL);
        }
        if !self.has_exited() {
            log::warn!("The admin daemon did not exit by itself, killing it");
            force_kill(pid);
        }
    }
}

/// Ends a process and its children without any window flashing on Windows.
fn force_kill(pid: u32) {
    let (program, args) = force_kill_command(cfg!(windows), pid);
    let mut command = std::process::Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    if let Err(e) = command.output() {
        log::warn!("Could not kill the admin daemon (pid {pid}): {e}");
    }
}

#[derive(Serialize)]
pub struct DaemonResponse {
    status: u16,
    body: String,
}

/// Forwards a request of the Daemon view to the daemon admin panel.
#[tauri::command]
pub async fn daemon_request(
    state: State<'_, DaemonState>,
    app_state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<DaemonResponse, String> {
    let is_post = method.eq_ignore_ascii_case("POST");
    let method = match method.to_ascii_uppercase().as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        other => return Err(format!("Unsupported method '{other}'")),
    };

    let port = state.admin_port.load(Ordering::Acquire);
    if port == 0 {
        return Err("The admin daemon is not running".to_string());
    }

    let url = admin_url(port, &path)?;
    let sent = body.clone();
    let mut request = state.client.request(method, url).bearer_auth(&state.token);
    if let Some(body) = body {
        request = request.header("content-type", "application/json").body(body);
    }

    let response = request.send().await.map_err(|e| format!("Admin daemon unreachable: {e}"))?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| format!("Invalid admin daemon response: {e}"))?;

    if is_post && (200..300).contains(&status) {
        remember_env(&app_state, &path, sent.as_deref()).await;
    }

    Ok(DaemonResponse { status, body })
}

/// The daemon only keeps what the Daemon view applies to its environment until it stops. In the
/// desktop app it is saved in the state file and given back to the daemon at every start
/// ([`support::SAVED_ENV_VAR`]); a reset goes back to the daemon defaults.
async fn remember_env(state: &AppState, path: &str, sent: Option<&str>) {
    // `None`: back to the daemon defaults
    let applied = match path {
        "/env" => {
            let parsed =
                sent.and_then(|body| serde_json::from_str::<BTreeMap<String, Value>>(body).ok());
            let Some(values) = parsed else {
                return;
            };
            Some(values)
        }
        "/env/reset" => None,
        _ => return,
    };

    let mut guard = state.daemon_settings.write().await;
    let settings = guard.get_or_insert_with(DaemonSettings::default);
    match applied {
        Some(values) => {
            settings.env.extend(values.into_iter().filter(|(key, _)| is_saveable_env_key(key)));
        }
        None => settings.env.clear(),
    }
    drop(guard);

    if let Err(e) = state.save_state().await {
        log::warn!("Could not save the daemon settings: {e}");
    }
}

/// Autostart choice. An earlier version kept it in its own file: it moves into the state file.
async fn load_autostart(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();

    let legacy = paths().legacy_settings;
    if let Ok(content) = std::fs::read_to_string(&legacy) {
        let mut guard = state.daemon_settings.write().await;
        if guard.is_none() {
            *guard = Some(DaemonSettings {
                autostart: legacy_autostart(&content),
                ..DaemonSettings::default()
            });
        }
        drop(guard);

        if state.save_state().await.is_ok() {
            let _ = std::fs::remove_file(&legacy);
        }
    }

    let autostart =
        state.daemon_settings.read().await.as_ref().is_some_and(|settings| settings.autostart);
    autostart
}

/// Daemon variables saved from the Daemon view, as the JSON object the daemon reads.
async fn saved_env(app: &AppHandle) -> Option<String> {
    let settings = app.state::<AppState>().daemon_settings.read().await.clone()?;
    if settings.env.is_empty() {
        return None;
    }
    serde_json::to_string(&settings.env).ok()
}

/// How the daemon is launched.
enum Launcher {
    /// The compiled executable bundled with the app.
    Sidecar,
    /// `python <script>`, for development.
    Script { python: String, script: String },
}

fn find_launcher() -> Option<Launcher> {
    if let Ok(script) = std::env::var(SCRIPT_OVERRIDE_ENV) {
        let default_python = if cfg!(windows) { "python" } else { "python3" };
        let python = std::env::var(PYTHON_OVERRIDE_ENV).unwrap_or_else(|_| default_python.into());
        return Some(Launcher::Script { python, script });
    }

    let executable = format!("{}{}", support::SIDECAR_NAME, std::env::consts::EXE_SUFFIX);
    let path: PathBuf = std::env::current_exe().ok()?.parent()?.join(executable);
    let len = std::fs::metadata(path).ok()?.len();

    is_real_sidecar(len).then_some(Launcher::Sidecar)
}

fn command_for(app: &AppHandle, launcher: &Launcher) -> Result<Command, tauri_plugin_shell::Error> {
    match launcher {
        Launcher::Sidecar => app.shell().sidecar(support::SIDECAR_NAME),
        Launcher::Script { python, script } => {
            Ok(app.shell().command(python).args([script.as_str(), "--daemon-mode"]))
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn paths() -> DaemonPaths {
    daemon_paths(&crate::persistence::state_file_path())
}

/// Starts the daemon with the app when the user turned the autostart on. Never blocks the
/// startup of the app.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let daemon = app.state::<DaemonState>();

        if find_launcher().is_none() {
            log::info!("No compiled admin daemon bundled with this build, Daemon view disabled");
            return;
        }
        daemon.available.store(true, Ordering::Release);

        if !load_autostart(&app).await {
            log::info!("Admin daemon autostart is off");
            return;
        }

        if let Err(e) = start(&app).await {
            log::error!("Admin daemon not started: {e}");
        }
    });
}

/// What every launch of the daemon needs.
struct Launch {
    env: HashMap<String, String>,
    admin_port: u16,
}

/// Starts the daemon. Does nothing when it already runs.
async fn start(app: &AppHandle) -> Result<(), String> {
    let daemon = app.state::<DaemonState>();
    let _lifecycle = daemon.lifecycle.lock().await;

    if daemon.is_stopping() {
        return Err("The app is closing".to_string());
    }
    let launcher =
        find_launcher().ok_or_else(|| "No admin daemon is bundled with this build".to_string())?;
    if daemon.supervising.load(Ordering::Acquire) {
        daemon.wanted.store(true, Ordering::Release);
        return Ok(());
    }

    let launch = prepare(app).await?;
    daemon.wanted.store(true, Ordering::Release);
    daemon.supervising.store(true, Ordering::Release);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = supervise(&app, &launcher, &launch).await {
            log::error!("{e}");
        }
        let daemon = app.state::<DaemonState>();
        daemon.wanted.store(false, Ordering::Release);
        daemon.supervising.store(false, Ordering::Release);
    });
    Ok(())
}

/// Stops the daemon and waits until it is gone. The bridge stays up for the next start.
async fn stop(app: &AppHandle) -> Result<(), String> {
    let daemon = app.state::<DaemonState>();
    let _lifecycle = daemon.lifecycle.lock().await;

    daemon.wanted.store(false, Ordering::Release);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle.state::<DaemonState>().terminate_blocking();
    })
    .await
    .map_err(|e| format!("Could not stop the admin daemon: {e}"))?;

    // The supervisor ends right after the process
    for _ in 0..40 {
        if !daemon.supervising.load(Ordering::Acquire) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("The admin daemon did not stop".to_string())
}

/// Creates the daemon files, starts the bridge (once) and works out the daemon environment.
async fn prepare(app: &AppHandle) -> Result<Launch, String> {
    let daemon = app.state::<DaemonState>();

    // The daemon reads the app defaults from the state file: make sure it holds a complete state
    // before the daemon can create a bare one.
    if let Err(e) = app.state::<AppState>().save_state().await {
        log::warn!("Could not write the state file before starting the daemon: {e}");
    }

    let paths = paths();
    prepare_files(&paths)?;

    let bridge_port = ensure_bridge(app).await?;
    let admin_port = free_port().map_err(|e| format!("No free port for the daemon: {e}"))?;
    let saved_env = saved_env(app).await;
    let env = sidecar_env(&SidecarEnv {
        bridge_port,
        admin_port,
        token: &daemon.token,
        parent_pid: std::process::id(),
        paths: &paths,
        saved_env: saved_env.as_deref(),
    });

    Ok(Launch { env, admin_port })
}

fn prepare_files(paths: &DaemonPaths) -> Result<(), String> {
    std::fs::create_dir_all(&paths.dir)
        .map_err(|e| format!("Cannot create {}: {e}", paths.dir.display()))?;

    let existing_rules = std::fs::read_to_string(&paths.rules).ok();
    if needs_default_rules(existing_rules.as_deref()) {
        std::fs::write(&paths.rules, DEFAULT_RULES)
            .map_err(|e| format!("Cannot create {}: {e}", paths.rules.display()))?;
    }
    Ok(())
}

/// Port of the bridge, which is started the first time and then kept.
async fn ensure_bridge(app: &AppHandle) -> Result<u16, String> {
    let daemon = app.state::<DaemonState>();
    let running = daemon.bridge.lock().ok().and_then(|guard| guard.as_ref().map(|b| b.port));
    if let Some(port) = running {
        return Ok(port);
    }

    let bridge = bridge::start(app.clone(), &daemon.token)
        .await
        .map_err(|e| format!("Cannot start the daemon bridge: {e}"))?;
    let port = bridge.port;
    if let Ok(mut guard) = daemon.bridge.lock() {
        *guard = Some(bridge);
    }
    Ok(port)
}

/// Keeps the daemon running while it is wanted: relaunches it when it stops, gives up if it
/// cannot start.
async fn supervise(app: &AppHandle, launcher: &Launcher, launch: &Launch) -> Result<(), String> {
    let daemon = app.state::<DaemonState>();
    let mut failed_launches = 0;

    while daemon.is_wanted() {
        let started = Instant::now();
        let exit_code = run_once(app, launcher, launch).await;
        daemon.admin_port.store(0, Ordering::Release);

        if !daemon.is_wanted() {
            break;
        }
        if exit_code == Some(RESTART_EXIT_CODE) {
            log::info!("Admin daemon restart requested");
            failed_launches = 0;
            continue;
        }

        failed_launches = next_failure_count(failed_launches, started.elapsed());
        if failed_launches >= MAX_FAILED_LAUNCHES {
            return Err(format!("The admin daemon keeps stopping (last exit code {exit_code:?})"));
        }

        log::warn!("Admin daemon stopped (exit code {exit_code:?}), restarting shortly");
        tokio::time::sleep(RESTART_DELAY).await;
    }

    Ok(())
}

/// Runs the daemon once, until its process ends. Returns its exit code if it has one.
async fn run_once(app: &AppHandle, launcher: &Launcher, launch: &Launch) -> Option<i32> {
    let daemon = app.state::<DaemonState>();

    let spawned =
        command_for(app, launcher).and_then(|command| command.envs(launch.env.clone()).spawn());
    let (events, mut child) = match spawned {
        Ok(spawned) => spawned,
        Err(e) => {
            log::error!("Cannot launch the admin daemon: {e}");
            return None;
        }
    };

    // From now on the daemon exits when the app closes this pipe, whatever the reason
    if let Err(e) = child.write(APP_HELLO) {
        log::warn!("Could not open the guard pipe of the admin daemon: {e}");
    }

    daemon.register(child);
    daemon.admin_port.store(launch.admin_port, Ordering::Release);
    log::info!("Admin daemon started (panel on 127.0.0.1:{})", launch.admin_port);

    let exit_code = wait_for_exit(events).await;

    if let Ok(mut guard) = daemon.child.lock() {
        guard.take();
    }
    daemon.child_pid.store(0, Ordering::Release);
    exit_code
}

async fn wait_for_exit(mut events: Receiver<CommandEvent>) -> Option<i32> {
    while let Some(event) = events.recv().await {
        match event {
            CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                log::debug!("[daemon] {}", String::from_utf8_lossy(&line).trim_end());
            }
            CommandEvent::Error(e) => log::warn!("[daemon] {e}"),
            CommandEvent::Terminated(payload) => return payload.code,
            _ => {}
        }
    }
    None
}

/// State of the daemon, for the Daemon view.
#[derive(Serialize)]
pub struct DaemonStatus {
    /// A compiled daemon is bundled with this build.
    available: bool,
    /// The daemon should be running (it can still be starting).
    wanted: bool,
    /// The admin panel answers.
    ready: bool,
    autostart: bool,
}

async fn panel_answers(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    tokio::time::timeout(READY_PROBE, tokio::net::TcpStream::connect(("127.0.0.1", port)))
        .await
        .is_ok_and(|connection| connection.is_ok())
}

#[tauri::command]
#[allow(clippy::unnecessary_wraps)]
pub async fn daemon_status(
    state: State<'_, DaemonState>,
    app_state: State<'_, AppState>,
) -> Result<DaemonStatus, String> {
    let autostart =
        app_state.daemon_settings.read().await.as_ref().is_some_and(|settings| settings.autostart);

    Ok(DaemonStatus {
        available: state.available.load(Ordering::Acquire),
        wanted: state.is_wanted(),
        ready: panel_answers(state.admin_port.load(Ordering::Acquire)).await,
        autostart,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn daemon_start(app: AppHandle) -> Result<(), String> {
    start(&app).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn daemon_stop(app: AppHandle) -> Result<(), String> {
    stop(&app).await
}

/// Chooses whether the daemon starts with the app (saved in the state file).
#[tauri::command]
pub async fn daemon_set_autostart(
    app_state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    app_state.daemon_settings.write().await.get_or_insert_with(DaemonSettings::default).autostart =
        enabled;
    app_state.save_state().await
}

/// Stops the daemon and the bridge. Called when the app exits.
pub fn shutdown(app: &AppHandle) {
    let daemon = app.state::<DaemonState>();
    daemon.stopping.store(true, Ordering::Release);
    daemon.wanted.store(false, Ordering::Release);
    daemon.terminate_blocking();

    let bridge = daemon.bridge.lock().ok().and_then(|mut guard| guard.take());
    if let Some(mut bridge) = bridge {
        bridge.shutdown();
    }
}
