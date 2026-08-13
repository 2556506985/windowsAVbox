use serde::Deserialize;
use tauri::State;

use crate::{
    database::{LibraryInput, LibraryItem},
    state::SharedState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPayload {
    site_key: String,
    #[serde(default)]
    site_name: String,
    vod_id: String,
    #[serde(default)]
    vod_name: String,
    #[serde(default)]
    vod_pic: String,
    #[serde(default)]
    vod_remarks: String,
}

#[tauri::command]
pub fn library_list_keeps(state: State<'_, SharedState>) -> Result<Vec<LibraryItem>, String> {
    state.database.list_keeps()
}

#[tauri::command]
pub fn library_list_history(state: State<'_, SharedState>) -> Result<Vec<LibraryItem>, String> {
    state.database.list_history()
}

#[tauri::command]
pub fn library_is_kept(
    site_key: String,
    vod_id: String,
    state: State<'_, SharedState>,
) -> Result<bool, String> {
    state.database.is_kept(&site_key, &vod_id)
}

#[tauri::command]
pub fn library_add_keep(
    item: LibraryPayload,
    state: State<'_, SharedState>,
) -> Result<LibraryItem, String> {
    state.database.upsert_keep(&item.into())
}

#[tauri::command]
pub fn library_remove_keep(
    site_key: String,
    vod_id: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    state.database.remove_keep(&site_key, &vod_id)
}

#[tauri::command]
pub fn library_add_history(
    item: LibraryPayload,
    state: State<'_, SharedState>,
) -> Result<LibraryItem, String> {
    state.database.upsert_history(&item.into())
}

#[tauri::command]
pub fn library_remove_history(
    site_key: String,
    vod_id: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    state.database.remove_history(&site_key, &vod_id)
}

#[tauri::command]
pub fn library_clear_history(state: State<'_, SharedState>) -> Result<(), String> {
    state.database.clear_history()
}

impl From<LibraryPayload> for LibraryInput {
    fn from(value: LibraryPayload) -> Self {
        Self {
            site_key: value.site_key,
            site_name: value.site_name,
            vod_id: value.vod_id,
            vod_name: value.vod_name,
            vod_pic: value.vod_pic,
            vod_remarks: value.vod_remarks,
        }
    }
}
