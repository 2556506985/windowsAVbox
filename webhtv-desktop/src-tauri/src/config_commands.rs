use std::time::{SystemTime, UNIX_EPOCH};

use tauri::State;

use crate::{
    config::{load_config_file, load_config_url, parse_config_payload, ConfigPayload},
    database::{ConfigDetail, ConfigSummary},
    state::SharedState,
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
