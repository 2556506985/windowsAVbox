use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::{
    config::{load_config_file, load_config_url, parse_config_payload, ConfigPayload},
    database::{ConfigDetail, ConfigSummary, ProxyConfigItem, SiteHealthItem},
    state::{DebugLogEntry, SharedState},
};

#[tauri::command]
pub fn config_list(state: State<'_, SharedState>) -> Result<Vec<ConfigSummary>, String> {
    state.database.list_configs()
}

#[tauri::command]
pub fn config_active(state: State<'_, SharedState>) -> Result<Option<ConfigDetail>, String> {
    state.database.active_config()
}

#[tauri::command]
pub async fn config_load_url(
    url: String,
    name: Option<String>,
    state: State<'_, SharedState>,
) -> Result<ConfigDetail, String> {
    let state = state.inner().clone();
    let loaded = load_config_url(&state.http, &url, name.as_deref()).await?;
    let detail =
        state
            .database
            .save_config(&loaded.source, &loaded.name, &loaded.document, true)?;
    state.spiders.invalidate_all();
    Ok(detail)
}

#[tauri::command]
pub fn config_import_json(
    json: String,
    name: Option<String>,
    state: State<'_, SharedState>,
) -> Result<ConfigDetail, String> {
    let ConfigPayload::Document(document) = parse_config_payload(&json, None)? else {
        return Err("paste import requires a direct configuration, not a depot".to_string());
    };
    let source = format!("inline://{}", now_nanos());
    let detail = state.database.save_config(
        &source,
        name.as_deref().unwrap_or("Local configuration"),
        &document,
        true,
    )?;
    state.spiders.invalidate_all();
    Ok(detail)
}

#[tauri::command]
pub fn config_import_file(
    path: String,
    name: Option<String>,
    state: State<'_, SharedState>,
) -> Result<ConfigDetail, String> {
    let loaded = load_config_file(&path, name.as_deref())?;
    let detail =
        state
            .database
            .save_config(&loaded.source, &loaded.name, &loaded.document, true)?;
    state.spiders.invalidate_all();
    Ok(detail)
}

#[tauri::command]
pub fn config_activate(id: i64, state: State<'_, SharedState>) -> Result<ConfigDetail, String> {
    let detail = state.database.activate_config(id)?;
    state.spiders.invalidate_all();
    Ok(detail)
}

#[tauri::command]
pub fn config_select_home(
    config_id: i64,
    site_key: String,
    state: State<'_, SharedState>,
) -> Result<ConfigDetail, String> {
    state.database.select_home(config_id, &site_key)
}

#[tauri::command]
pub fn config_delete(id: i64, state: State<'_, SharedState>) -> Result<(), String> {
    state.database.delete_config(id)?;
    state.spiders.invalidate_all();
    Ok(())
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[tauri::command]
pub fn proxy_list(state: State<'_, SharedState>) -> Result<Vec<ProxyConfigItem>, String> {
    state.database.list_proxy_configs()
}

#[tauri::command]
pub fn proxy_upsert(
    id: Option<i64>,
    name: String,
    pattern: String,
    proxy_url: String,
    enabled: bool,
    state: State<'_, SharedState>,
) -> Result<i64, String> {
    state
        .database
        .upsert_proxy_config(id, &name, &pattern, &proxy_url, enabled)
}

#[tauri::command]
pub fn proxy_delete(id: i64, state: State<'_, SharedState>) -> Result<(), String> {
    state.database.delete_proxy_config(id)
}

#[tauri::command]
pub fn proxy_find(url: String, state: State<'_, SharedState>) -> Result<Option<String>, String> {
    state.database.find_proxy_for_url(&url)
}

#[tauri::command]
pub fn debug_logs_list(state: State<'_, SharedState>) -> Result<Vec<DebugLogEntry>, String> {
    let logs = state
        .debug_logs
        .lock()
        .map_err(|_| "debug log lock is unavailable".to_string())?;
    Ok(logs.iter().rev().cloned().collect())
}

#[tauri::command]
pub fn debug_logs_clear(state: State<'_, SharedState>) -> Result<(), String> {
    let mut logs = state
        .debug_logs
        .lock()
        .map_err(|_| "debug log lock is unavailable".to_string())?;
    logs.clear();
    Ok(())
}

#[tauri::command]
pub fn debug_log_add(
    level: String,
    module: String,
    message: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    state.debug_log(&level, &module, &message);
    Ok(())
}

#[tauri::command]
pub fn site_health_record(
    config_id: i64,
    site_key: String,
    method: String,
    success: bool,
    error: Option<String>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    state
        .database
        .record_site_health(config_id, &site_key, &method, success, error.as_deref())
}

#[tauri::command]
pub fn site_health_list(config_id: i64, state: State<'_, SharedState>) -> Result<Vec<SiteHealthItem>, String> {
    state.database.list_site_health(config_id)
}

#[tauri::command]
pub fn site_health_reset(
    config_id: i64,
    site_key: Option<String>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    state.database.reset_site_health(config_id, site_key.as_deref())
}
