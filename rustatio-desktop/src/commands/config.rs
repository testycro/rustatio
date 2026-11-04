use rustatio_core::validation;
use rustatio_core::{AppConfig, ClientDefinition, ClientInfo, ClientType};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn update_config(config: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    validation::validate_rate(config.faker.default_upload_rate, "upload_rate")
        .map_err(|e| format!("{e}"))?;
    validation::validate_rate(config.faker.default_download_rate, "download_rate")
        .map_err(|e| format!("{e}"))?;
    validation::validate_update_interval(config.faker.update_interval)
        .map_err(|e| format!("{e}"))?;
    validation::validate_port(config.client.default_port).map_err(|e| format!("{e}"))?;

    let mut app_config = state.config.write().await;
    *app_config = config.clone();

    let path = crate::persistence::config_file_path();
    config.save(&path).map_err(|e| format!("Failed to save config: {e}"))?;

    log::info!("Configuration updated and saved");
    Ok(())
}

#[tauri::command]
pub async fn get_client_types() -> Vec<String> {
    ClientType::all_ids()
}

#[tauri::command]
pub async fn get_client_infos() -> Vec<ClientInfo> {
    ClientType::all_infos()
}

/// Full definition of every emulated client (stored in the state file next to `default_config`)
#[tauri::command]
pub async fn get_client_definitions() -> Vec<ClientDefinition> {
    rustatio_core::client_definitions()
}

/// Add a client, or replace the one with the same id
#[tauri::command]
pub async fn upsert_client_definition(
    definition: ClientDefinition,
    state: State<'_, AppState>,
) -> Result<(), String> {
    rustatio_core::upsert_client_definition(definition)?;
    state.save_state().await
}

/// Delete a client (the last remaining one cannot be deleted)
#[tauri::command]
pub async fn delete_client_definition(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if rustatio_core::client_definition(&id).is_none() {
        return Err(format!("client '{id}' not found"));
    }
    rustatio_core::remove_client_definition(&id)?;
    state.save_state().await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_log_level(level: String) {
    rustatio_core::logger::set_max_emit_level(&level);
    log::info!("IPC log level set to: {level}");
}

/// Only used to export presets. Limited to `.json` files so that the webview cannot overwrite
/// any file of the user (scripts, shell profile...) through this command.
#[tauri::command]
pub async fn write_file(path: String, contents: String) -> Result<(), String> {
    let path = json_export_path(&path)?;
    std::fs::write(&path, contents).map_err(|e| format!("Failed to write file: {e}"))?;
    log::info!("File written to: {}", path.display());
    Ok(())
}

/// The save dialog does not add the extension everywhere (Linux): a name without extension
/// gets `.json`, any other extension is refused.
fn json_export_path(path: &str) -> Result<std::path::PathBuf, String> {
    let mut path = std::path::PathBuf::from(path);
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("json") => Ok(path),
        None => {
            path.set_extension("json");
            Ok(path)
        }
        Some(_) => Err("Only .json files can be written".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::json_export_path;
    use std::path::PathBuf;

    #[test]
    fn json_export_path_only_allows_json_files() {
        assert_eq!(json_export_path("/tmp/preset.json"), Ok(PathBuf::from("/tmp/preset.json")));
        assert_eq!(json_export_path("/tmp/preset.JSON"), Ok(PathBuf::from("/tmp/preset.JSON")));
        assert_eq!(json_export_path("/tmp/preset"), Ok(PathBuf::from("/tmp/preset.json")));
        // never the file itself: a dot file has no extension, ".json" is added
        assert_eq!(
            json_export_path("/home/user/.bashrc"),
            Ok(PathBuf::from("/home/user/.bashrc.json"))
        );
        assert!(json_export_path("/tmp/run.sh").is_err());
    }
}
