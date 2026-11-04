//! Where the desktop app keeps its files (state, configuration, admin daemon files).
//!
//! On Windows this is `%APPDATA%\com.rustatio`: the folder the installer removes when the user
//! ticks "Delete the application data" while uninstalling. Anywhere else it is the folder the
//! app has always used, `~/.config/rustatio`. Pure path logic, no dependency on the app.

use std::io;
use std::path::{Path, PathBuf};

/// Identifier of the app (`identifier` in `tauri.conf.json`), name of the folder the Windows
/// uninstaller deletes.
const APP_IDENTIFIER: &str = "com.rustatio";

/// Folder of the daemon files inside the data folder.
pub const DAEMON_DIR: &str = "daemon";
pub const STATE_FILE: &str = "desktop-state.json";
pub const CONFIG_FILE: &str = "config.toml";

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The folder of the app files. Empty (the working directory) when nothing better is known.
pub fn data_dir_from(windows: bool, appdata: Option<&str>, home: Option<&str>) -> PathBuf {
    if windows {
        if let Some(appdata) = non_empty(appdata) {
            return Path::new(appdata).join(APP_IDENTIFIER);
        }
    }

    legacy_dirs_from(home).into_iter().next().unwrap_or_default()
}

/// Where earlier versions kept the files, most likely first: `~/.config/rustatio`, then the
/// working directory when there was no home folder.
pub fn legacy_dirs_from(home: Option<&str>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = non_empty(home) {
        dirs.push(Path::new(home).join(".config").join("rustatio"));
    }
    dirs.push(PathBuf::new());
    dirs
}

pub fn data_dir() -> PathBuf {
    data_dir_from(
        cfg!(windows),
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

pub fn legacy_dirs() -> Vec<PathBuf> {
    legacy_dirs_from(std::env::var("HOME").ok().as_deref())
}

/// A folder called `daemon` found in the working directory is only ours if it holds daemon files.
pub fn is_daemon_dir(dir: &Path) -> bool {
    ["rules.txt", "rustatio_daemon.log"].iter().any(|name| dir.join(name).is_file())
}

fn copy_recursively(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursively(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Moves a file or a folder. Does nothing (and returns `false`) when there is nothing to move or
/// when the destination already exists: what the user has there always wins.
pub fn move_entry(from: &Path, to: &Path) -> io::Result<bool> {
    if !from.exists() || to.exists() {
        return Ok(false);
    }
    if let Some(parent) = to.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    // A rename cannot cross volumes: copy then delete
    if std::fs::rename(from, to).is_err() {
        copy_recursively(from, to)?;
        if from.is_dir() {
            std::fs::remove_dir_all(from)?;
        } else {
            std::fs::remove_file(from)?;
        }
    }
    Ok(true)
}

/// Brings the files of an earlier location into `data`. Returns what was moved.
pub fn migrate_from(legacy: &Path, data: &Path) -> Vec<PathBuf> {
    if legacy == data {
        return Vec::new();
    }

    let mut moved = Vec::new();
    let mut attempt = |from: PathBuf, to: PathBuf| match move_entry(&from, &to) {
        Ok(true) => moved.push(to),
        Ok(false) => {}
        Err(e) => log::warn!("Could not move {} to {}: {e}", from.display(), to.display()),
    };

    attempt(legacy.join(STATE_FILE), data.join(STATE_FILE));
    let daemon_dir = legacy.join(DAEMON_DIR);
    if is_daemon_dir(&daemon_dir) {
        attempt(daemon_dir, data.join(DAEMON_DIR));
    }

    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustatio-data-dir-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn windows_uses_the_folder_the_uninstaller_deletes() {
        let dir =
            data_dir_from(true, Some("C:\\Users\\me\\AppData\\Roaming"), Some("C:\\Users\\me"));
        assert_eq!(dir, Path::new("C:\\Users\\me\\AppData\\Roaming").join("com.rustatio"));
    }

    #[test]
    fn other_systems_keep_the_historical_folder() {
        let dir = data_dir_from(false, Some("/ignored"), Some("/home/me"));
        assert_eq!(dir, Path::new("/home/me/.config/rustatio"));
    }

    #[test]
    fn without_any_folder_the_working_directory_is_used() {
        assert_eq!(data_dir_from(false, None, None), PathBuf::new());
        assert_eq!(data_dir_from(true, Some("  "), Some("")), PathBuf::new());
        assert_eq!(data_dir_from(true, None, None).join(STATE_FILE), Path::new(STATE_FILE));
    }

    #[test]
    fn windows_without_appdata_falls_back_to_home() {
        assert_eq!(data_dir_from(true, None, Some("/h")), Path::new("/h/.config/rustatio"));
    }

    #[test]
    fn legacy_folders_are_home_then_working_directory() {
        assert_eq!(
            legacy_dirs_from(Some("/home/me")),
            vec![PathBuf::from("/home/me/.config/rustatio"), PathBuf::new()]
        );
        assert_eq!(legacy_dirs_from(None), vec![PathBuf::new()]);
    }

    #[test]
    fn state_and_daemon_files_are_moved_once() {
        let root = unique_dir("migrate");
        let (legacy, data) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(legacy.join("daemon")).unwrap();
        std::fs::write(legacy.join(STATE_FILE), "state").unwrap();
        std::fs::write(legacy.join("daemon").join("rules.txt"), "rules").unwrap();

        let moved = migrate_from(&legacy, &data);
        assert_eq!(moved.len(), 2);
        assert_eq!(std::fs::read_to_string(data.join(STATE_FILE)).unwrap(), "state");
        assert_eq!(
            std::fs::read_to_string(data.join("daemon").join("rules.txt")).unwrap(),
            "rules"
        );
        assert!(!legacy.join(STATE_FILE).exists());
        assert!(!legacy.join("daemon").exists());

        // nothing left to move
        assert_eq!(migrate_from(&legacy, &data), Vec::<PathBuf>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn existing_files_are_never_replaced() {
        let root = unique_dir("keep");
        let (legacy, data) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(legacy.join(STATE_FILE), "old").unwrap();
        std::fs::write(data.join(STATE_FILE), "new").unwrap();

        assert_eq!(migrate_from(&legacy, &data), Vec::<PathBuf>::new());
        assert_eq!(std::fs::read_to_string(data.join(STATE_FILE)).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(legacy.join(STATE_FILE)).unwrap(), "old");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_foreign_daemon_folder_is_left_alone() {
        let root = unique_dir("foreign");
        let (legacy, data) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(legacy.join("daemon")).unwrap();
        std::fs::write(legacy.join("daemon").join("notes.md"), "mine").unwrap();

        assert_eq!(migrate_from(&legacy, &data), Vec::<PathBuf>::new());
        assert!(legacy.join("daemon").join("notes.md").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn same_folder_is_a_no_op() {
        let root = unique_dir("same");
        std::fs::write(root.join(STATE_FILE), "state").unwrap();
        assert_eq!(migrate_from(&root, &root), Vec::<PathBuf>::new());
        assert!(root.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
