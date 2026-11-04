//! Pure helpers of the embedded daemon (no Tauri or HTTP types), unit tested on their own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Name of the bundled executable: `externalBin` in `tauri.conf.json`, without the target triple.
pub const SIDECAR_NAME: &str = "rustatio-daemon";

/// A real compiled daemon weighs several MB. `build.rs` creates an empty placeholder when the
/// sidecar was not built (so that `cargo check` and `clippy` work), anything tiny is not bundled.
const MIN_SIDECAR_BYTES: u64 = 64 * 1024;

pub const fn is_real_sidecar(len: u64) -> bool {
    len >= MIN_SIDECAR_BYTES
}

/// A daemon that stops sooner than this after its launch did not really start.
const MIN_HEALTHY_RUN: Duration = Duration::from_secs(10);

/// Failed launches in a row, once the daemon has just stopped after running for `ran_for`.
pub const fn next_failure_count(previous: u32, ran_for: Duration) -> u32 {
    if ran_for.as_secs() < MIN_HEALTHY_RUN.as_secs() {
        previous.saturating_add(1)
    } else {
        1
    }
}

/// Lowercase hexadecimal encoding.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

pub fn constant_time_eq(expected: &[u8], actual: &[u8]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(actual.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

/// Token of an `Authorization: Bearer <token>` header value.
pub fn bearer_token(header: Option<&str>) -> Option<&str> {
    header?.strip_prefix("Bearer ").map(str::trim)
}

/// Instance ids sent by the daemon (strings, like the server API); unparsable ones are ignored.
pub fn parse_ids(ids: &[String]) -> Vec<u32> {
    ids.iter().filter_map(|id| id.trim().parse().ok()).collect()
}

/// URL of a daemon admin route: `path` is what follows `/api` (for example `/logs?lines=100`).
/// The host and port are fixed, `path` can only choose the route on the daemon.
pub fn admin_url(port: u16, path: &str) -> Result<String, String> {
    let route = path.split('?').next().unwrap_or_default();

    let invalid = !path.starts_with('/')
        || path.starts_with("//")
        || path.chars().any(|c| c.is_whitespace() || c.is_control() || c == '#' || c == '\\')
        || route.split('/').any(|segment| segment == "..");

    if invalid {
        return Err(format!("Invalid daemon path '{path}'"));
    }
    Ok(format!("http://127.0.0.1:{port}/api{path}"))
}

/// Everything the daemon writes lives in a `daemon` folder next to the desktop state file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPaths {
    pub dir: PathBuf,
    pub rules: PathBuf,
    pub log: PathBuf,
    pub check_logs: PathBuf,
    pub archive: PathBuf,
    /// Options file of an earlier version (now in the state file, see [`legacy_autostart`]).
    pub legacy_settings: PathBuf,
    /// The desktop state file: the daemon reads `default_config` from it.
    pub defaults: PathBuf,
}

pub fn daemon_paths(state_file: &Path) -> DaemonPaths {
    // The state file path can be relative (no HOME): the daemon runs elsewhere, make it absolute
    let state_dir = state_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let state_dir = std::path::absolute(state_dir).unwrap_or_else(|_| state_dir.to_path_buf());

    let dir = state_dir.join("daemon");
    DaemonPaths {
        rules: dir.join("rules.txt"),
        log: dir.join("rustatio_daemon.log"),
        check_logs: dir.join("check_logs.json"),
        archive: dir.join("archived"),
        legacy_settings: dir.join("desktop-settings.txt"),
        defaults: state_dir.join(state_file.file_name().unwrap_or_default()),
        dir,
    }
}

/// What the daemon needs to know to talk to the app.
pub struct SidecarEnv<'a> {
    pub bridge_port: u16,
    pub admin_port: u16,
    pub token: &'a str,
    pub parent_pid: u32,
    pub paths: &'a DaemonPaths,
    /// Values saved from the Daemon view (JSON object), applied by the daemon over its defaults.
    pub saved_env: Option<&'a str>,
}

/// Variable holding the values saved from the Daemon view (see `apply_saved_env` in the daemon).
pub const SAVED_ENV_VAR: &str = "RUSTATIO_SAVED_ENV";

/// Variables the app decides (ports, token, files): never saved from the Daemon view.
const APP_ENV_KEYS: [&str; 10] = [
    "PORT",
    "RUSTATIO_API",
    "ADMIN_PORT",
    "ADMIN_HOST",
    "AUTH_TOKEN",
    "RULES_FILE",
    "DEFAULTS_FILE",
    "LOGFILE",
    "CHECK_LOGS_FILE",
    "ARCHIVE_FOLDER",
];

/// Whether a variable of the daemon environment edited in the Daemon view is kept for the next
/// starts.
pub fn is_saveable_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !APP_ENV_KEYS.contains(&key)
}

/// Environment of the daemon process (same variables as in the Docker image).
pub fn sidecar_env(params: &SidecarEnv<'_>) -> HashMap<String, String> {
    let path = |value: &Path| value.to_string_lossy().into_owned();

    let mut env = vec![
        ("PORT", params.bridge_port.to_string()),
        ("RUSTATIO_API", format!("http://127.0.0.1:{}", params.bridge_port)),
        ("ADMIN_PORT", params.admin_port.to_string()),
        // The panel is only reachable through the app
        ("ADMIN_HOST", "127.0.0.1".to_string()),
        ("AUTH_TOKEN", params.token.to_string()),
        ("RULES_FILE", path(&params.paths.rules)),
        ("DEFAULTS_FILE", path(&params.paths.defaults)),
        ("LOGFILE", path(&params.paths.log)),
        ("CHECK_LOGS_FILE", path(&params.paths.check_logs)),
        ("ARCHIVE_FOLDER", path(&params.paths.archive)),
        // The log watcher reads the app's INFO logs
        ("RUST_LOG", "info".to_string()),
        // Lets the daemon exit by itself if the app disappears without stopping it: when the
        // pipe the app keeps open on its input closes, and when the app process is gone
        ("RUSTATIO_EXIT_ON_STDIN_EOF", "1".to_string()),
        ("RUSTATIO_PARENT_PID", params.parent_pid.to_string()),
        // Logs contain emoji: never fall back to a legacy console code page
        ("PYTHONUTF8", "1".to_string()),
        ("PYTHONIOENCODING", "utf-8".to_string()),
        ("PYTHONUNBUFFERED", "1".to_string()),
    ];
    if let Some(saved) = params.saved_env {
        env.push((SAVED_ENV_VAR, saved.to_string()));
    }

    env.into_iter().map(|(key, value)| (key.to_string(), value)).collect()
}

/// Written by the app on the daemon input right after the launch. The daemon only watches that
/// input once it has received it, then exits when the app closes it (the app ended, even badly).
pub const APP_HELLO: &[u8] = b"rustatio\n";

/// Autostart choice of the options file written by an earlier version
/// (`autostart=true|false`): moved into the state file at the first start.
pub fn legacy_autostart(content: &str) -> bool {
    let mut autostart = false;

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "autostart" {
            continue;
        }
        match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => autostart = true,
            "false" | "0" | "no" | "off" => autostart = false,
            _ => {}
        }
    }

    autostart
}

/// Program and arguments that end a process and the ones it started (last resort, when the
/// daemon did not exit by itself).
pub fn force_kill_command(windows: bool, pid: u32) -> (&'static str, Vec<String>) {
    if windows {
        ("taskkill", vec!["/PID".to_string(), pid.to_string(), "/T".to_string(), "/F".to_string()])
    } else {
        ("kill", vec!["-KILL".to_string(), pid.to_string()])
    }
}

/// First lines of the rules files generated by the app and by the daemon itself when none
/// existed (an earlier version of the app wrote the default rules commented out).
const GENERATED_HEADERS: [&str; 2] =
    ["# Rustatio daemon rules (condition | action | assignment)", "# Fichier de règles Rustatio"];

/// Whether the rules file must be (re)created with the default rules: it does not exist, or it
/// only holds what was generated and no rule at all. A file the user edited is never replaced.
pub fn needs_default_rules(existing: Option<&str>) -> bool {
    let Some(content) = existing else {
        return true;
    };

    let has_rule = content.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with('#')
    });

    !has_rule && GENERATED_HEADERS.iter().any(|header| content.starts_with(header))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_stops_count_as_failed_launches() {
        assert_eq!(next_failure_count(0, Duration::from_secs(1)), 1);
        assert_eq!(next_failure_count(3, Duration::from_secs(9)), 4);
        // a daemon that ran for a while was healthy: the streak starts over
        assert_eq!(next_failure_count(4, Duration::from_secs(10)), 1);
        assert_eq!(next_failure_count(u32::MAX, Duration::ZERO), u32::MAX);
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0a, 0xff]), "000aff");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn constant_time_eq_compares_content_and_length() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokeN"));
        assert!(!constant_time_eq(b"token", b"toke"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn bearer_token_needs_the_bearer_scheme() {
        assert_eq!(bearer_token(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer_token(Some("Bearer  abc ")), Some("abc"));
        assert_eq!(bearer_token(Some("Basic abc")), None);
        assert_eq!(bearer_token(Some("abc")), None);
        assert_eq!(bearer_token(None), None);
    }

    #[test]
    fn ids_are_parsed_and_junk_is_ignored() {
        let ids = ["3".to_string(), " 12 ".to_string(), "x".to_string(), "-1".to_string()];
        assert_eq!(parse_ids(&ids), vec![3, 12]);
        assert_eq!(parse_ids(&[]), Vec::<u32>::new());
    }

    #[test]
    fn admin_url_keeps_route_and_query() {
        assert_eq!(admin_url(8081, "/status").as_deref(), Ok("http://127.0.0.1:8081/api/status"));
        assert_eq!(
            admin_url(8081, "/logs?lines=200").as_deref(),
            Ok("http://127.0.0.1:8081/api/logs?lines=200")
        );
    }

    #[test]
    fn admin_url_refuses_anything_that_could_change_the_target() {
        for path in
            ["status", "//evil.example/x", "/a b", "/a\r\nHost: x", "/x#y", "/../x", "/a\\b"]
        {
            assert!(admin_url(8081, path).is_err(), "{path:?} should be refused");
        }
        // ".." is only a problem as a path segment
        assert!(admin_url(8081, "/logs?lines=..").is_ok());
    }

    #[test]
    fn daemon_files_live_next_to_the_state_file() {
        let paths = daemon_paths(Path::new("/home/me/.config/rustatio/desktop-state.json"));
        assert_eq!(paths.dir, Path::new("/home/me/.config/rustatio/daemon"));
        assert_eq!(paths.rules, Path::new("/home/me/.config/rustatio/daemon/rules.txt"));
        assert_eq!(paths.log, Path::new("/home/me/.config/rustatio/daemon/rustatio_daemon.log"));
        assert_eq!(paths.check_logs, Path::new("/home/me/.config/rustatio/daemon/check_logs.json"));
        assert_eq!(paths.archive, Path::new("/home/me/.config/rustatio/daemon/archived"));
        assert_eq!(
            paths.legacy_settings,
            Path::new("/home/me/.config/rustatio/daemon/desktop-settings.txt")
        );
        assert_eq!(paths.defaults, Path::new("/home/me/.config/rustatio/desktop-state.json"));
    }

    #[test]
    fn relative_state_file_gives_absolute_paths() {
        let paths = daemon_paths(Path::new("desktop-state.json"));
        assert!(paths.dir.is_absolute());
        assert!(paths.defaults.is_absolute());
        assert!(paths.defaults.ends_with("desktop-state.json"));
    }

    #[test]
    fn sidecar_env_points_the_daemon_at_the_app() {
        let paths = daemon_paths(Path::new("/data/desktop-state.json"));
        let env = sidecar_env(&SidecarEnv {
            bridge_port: 50123,
            admin_port: 50124,
            token: "secret",
            parent_pid: 4242,
            paths: &paths,
            saved_env: None,
        });

        assert_eq!(env["PORT"], "50123");
        assert_eq!(env["RUSTATIO_API"], "http://127.0.0.1:50123");
        assert_eq!(env["ADMIN_PORT"], "50124");
        assert_eq!(env["ADMIN_HOST"], "127.0.0.1");
        assert_eq!(env["AUTH_TOKEN"], "secret");
        assert_eq!(env["DEFAULTS_FILE"], "/data/desktop-state.json");
        assert_eq!(env["RULES_FILE"], "/data/daemon/rules.txt");
        assert_eq!(env["RUSTATIO_PARENT_PID"], "4242");
        assert_eq!(env["RUSTATIO_EXIT_ON_STDIN_EOF"], "1");
        assert_eq!(env["PYTHONUTF8"], "1");
    }

    #[test]
    fn default_rules_are_created_when_missing() {
        assert!(needs_default_rules(None));
    }

    #[test]
    fn generated_files_without_rules_are_replaced() {
        let legacy = "# Rustatio daemon rules (condition | action | assignment)\n# a | stop |\n";
        assert!(needs_default_rules(Some(legacy)));
        assert!(needs_default_rules(Some("# Fichier de règles Rustatio\n")));
    }

    #[test]
    fn edited_files_are_kept() {
        let edited = "# Rustatio daemon rules (condition | action | assignment)\na | stop |\n";
        assert!(!needs_default_rules(Some(edited)));
        assert!(!needs_default_rules(Some("")));
        assert!(!needs_default_rules(Some("# my own notes\n")));
        assert!(!needs_default_rules(Some("stats.state = \"Stopped\" | start |\n")));
    }

    #[test]
    fn legacy_autostart_is_off_unless_turned_on() {
        assert!(!legacy_autostart(""));
        assert!(!legacy_autostart("autostart=false\n"));
        assert!(!legacy_autostart("autostart = maybe"));
        assert!(legacy_autostart("autostart=true\n"));
        assert!(legacy_autostart("# note\nautostart = On"));
        assert!(legacy_autostart("other=0\nautostart=1\n"));
    }

    #[test]
    fn only_daemon_options_are_saved() {
        for key in ["REFRESH_INTERVAL", "DRY_RUN", "RUST_DAEMON_LOG", "WATCHER_MAX_STRIKE"] {
            assert!(is_saveable_env_key(key), "{key} should be saved");
        }
        for key in ["PORT", "AUTH_TOKEN", "RULES_FILE", "ADMIN_HOST", "", "lower", "A B", "PATH="] {
            assert!(!is_saveable_env_key(key), "{key:?} should not be saved");
        }
    }

    #[test]
    fn saved_values_reach_the_daemon() {
        let paths = daemon_paths(Path::new("/data/desktop-state.json"));
        let mut params = SidecarEnv {
            bridge_port: 1,
            admin_port: 2,
            token: "t",
            parent_pid: 3,
            paths: &paths,
            saved_env: Some(r#"{"DRY_RUN":true}"#),
        };
        assert_eq!(sidecar_env(&params)[SAVED_ENV_VAR], r#"{"DRY_RUN":true}"#);

        params.saved_env = None;
        assert!(!sidecar_env(&params).contains_key(SAVED_ENV_VAR));
    }

    #[test]
    fn force_kill_ends_the_whole_process_tree_on_windows() {
        let (program, args) = force_kill_command(true, 42);
        assert_eq!(program, "taskkill");
        assert_eq!(args, ["/PID", "42", "/T", "/F"]);
        let (program, args) = force_kill_command(false, 42);
        assert_eq!(program, "kill");
        assert_eq!(args, ["-KILL", "42"]);
    }

    #[test]
    fn placeholder_sidecars_are_not_real() {
        assert!(!is_real_sidecar(0));
        assert!(!is_real_sidecar(1024));
        assert!(is_real_sidecar(8 * 1024 * 1024));
    }
}
