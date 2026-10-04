use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::{
    player::{PlayerControlRequest, PlayerOpenRequest},
    state::{CoreState, SharedState},
};

const BRIDGE_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    app_name: &'static str,
    app_version: &'static str,
    bridge_version: u32,
    mode: &'static str,
    platform: &'static str,
    local_server_base: Option<String>,
    uptime_ms: u128,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, SharedState>) -> BootstrapState {
    BootstrapState {
        app_name: "WebHomeTV Desktop",
        app_version: env!("CARGO_PKG_VERSION"),
        bridge_version: BRIDGE_VERSION,
        mode: "desktop",
        platform: std::env::consts::OS,
        local_server_base: None,
        uptime_ms: state.started_at.elapsed().as_millis(),
    }
}

#[tauri::command]
pub fn bridge_invoke(
    request_id: String,
    method: String,
    payload: String,
    state: State<'_, SharedState>,
) -> Result<Value, String> {
    let payload = parse_payload(&payload)?;
    dispatch(&request_id, &method, &payload, state.inner())
}

#[tauri::command]
pub fn bridge_console(level: String, message: String) {
    eprintln!("[webhome:{level}] {message}");
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn bridge_network(
    net_type: String,
    method: String,
    url: String,
    status: i32,
    duration_ms: i64,
    detail: String,
) {
    eprintln!(
        "[webhome-network] type={net_type} method={method} status={status} duration_ms={duration_ms} url={url} detail={detail}"
    );
}

#[tauri::command]
pub fn bridge_inline_result(
    id: String,
    payload: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    const MAX_INLINE_RESULTS: usize = 256;
    let mut store = state
        .inline_results
        .lock()
        .map_err(|_| "inline result store is unavailable".to_string())?;
    // Bound the in-memory store: no consumer exists for these entries yet, so evict
    // an arbitrary entry when the cap is reached to prevent unbounded growth.
    if store.len() >= MAX_INLINE_RESULTS {
        if let Some(oldest) = store.keys().next().cloned() {
            store.remove(&oldest);
        }
    }
    store.insert(id, payload);
    Ok(())
}

fn parse_payload(payload: &str) -> Result<Value, String> {
    if payload.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(payload).map_err(|error| format!("invalid bridge payload: {error}"))
}

fn dispatch(
    _request_id: &str,
    method: &str,
    payload: &Value,
    state: &CoreState,
) -> Result<Value, String> {
    match method {
        "device.info" => Ok(json!({
            "brand": "Microsoft",
            "model": "Windows PC",
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "mode": "desktop",
            "isLeanback": false,
            "appVersion": env!("CARGO_PKG_VERSION"),
        })),
        "config.info" => config_info(state),
        "site.info" => site_info(state),
        "app.history" => serde_json::to_value(state.database.list_history()?)
            .map_err(|error| format!("unable to serialize history: {error}")),
        "player.playUrl" => player_play_url(payload, state),
        "player.control" => player_control(payload, state),
        "player.status" => serde_json::to_value(state.player.status()?)
            .map_err(|error| format!("unable to serialize player status: {error}")),
        "ext.info" => Ok(json!({
            "siteKey": "",
            "siteName": "",
            "homePage": "",
            "enabled": false,
            "matched": 0,
            "ready": 0,
        })),
        "cache.get" => cache_get(payload, state),
        "cache.set" => cache_set(payload, state),
        "cache.del" => cache_del(payload, state),
        _ => Err(format!("bridge method is not implemented: {method}")),
    }
}

fn player_play_url(payload: &Value, state: &CoreState) -> Result<Value, String> {
    let url = required_string(payload, "url")?.trim().to_string();
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(&url)
        .to_string();
    let mut headers = player_headers(payload.get("headers"));
    if url.starts_with("http://127.0.0.1:9978/proxy")
        || url.starts_with("http://localhost:9978/proxy")
        || url.starts_with("http://[::1]:9978/proxy")
    {
        if url.contains("site=baidu") {
            headers
                .retain(|name, _| !name.eq_ignore_ascii_case("user-agent"));
            headers.insert(
                "User-Agent".to_string(),
                "netdisk;12.11.9;V2238A;android-android;12;JSbridge4.4.0;jointBridge;1.1.0;"
                    .to_string(),
            );
        }
    }
    let status = state.player.open(PlayerOpenRequest {
        url,
        title,
        headers,
        start: None,
    })?;
    serde_json::to_value(status)
        .map_err(|error| format!("unable to serialize player status: {error}"))
}

fn player_control(payload: &Value, state: &CoreState) -> Result<Value, String> {
    let action = required_string(payload, "action")?;
    let command = match action {
        "play" => "resume",
        "pause" => "pause",
        "stop" => "stop",
        "replay" => "seek",
        "fullscreen" => "fullscreen",
        "toggle" => "togglePause",
        _ => return Ok(json!({})),
    };
    let value = (command == "seek").then_some(-10_000.0);
    let status = state.player.control(PlayerControlRequest {
        command: command.to_string(),
        value,
    })?;
    serde_json::to_value(status)
        .map_err(|error| format!("unable to serialize player status: {error}"))
}

fn player_headers(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .as_str()
                        .map(|value| (name.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cache_get(payload: &Value, state: &CoreState) -> Result<Value, String> {
    let key = cache_key(payload)?;
    let value = state.database.cache_get(&key)?;
    Ok(Value::String(value))
}

fn cache_set(payload: &Value, state: &CoreState) -> Result<Value, String> {
    let key = cache_key(payload)?;
    let value = required_string(payload, "value")?;
    state.database.cache_set(&key, value)?;
    Ok(json!({}))
}

fn cache_del(payload: &Value, state: &CoreState) -> Result<Value, String> {
    let key = cache_key(payload)?;
    state.database.cache_delete(&key)?;
    Ok(json!({}))
}

fn config_info(state: &CoreState) -> Result<Value, String> {
    let Some(config) = state.database.active_config()? else {
        return Ok(json!({"id": 0, "url": "", "desc": "", "driveCheck": false}));
    };
    Ok(json!({
        "id": config.summary.id,
        "url": config.summary.url,
        "desc": config.summary.desc,
        "driveCheck": false,
    }))
}

fn site_info(state: &CoreState) -> Result<Value, String> {
    let site = state
        .database
        .active_config()?
        .and_then(|config| config.home_site);
    match site {
        Some(site) => serde_json::to_value(site)
            .map_err(|error| format!("unable to serialize active site: {error}")),
        None => Ok(json!({
            "key": "",
            "name": "",
            "homePage": "",
            "chromeMode": "",
            "type": 0,
            "header": {},
        })),
    }
}

fn cache_key(payload: &Value) -> Result<String, String> {
    let key = required_string(payload, "key")?;
    let rule = payload
        .get("rule")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Use NUL as separator (consistent with progress_cache_key) to avoid ambiguity when
    // either segment itself contains underscores.
    Ok(if rule.is_empty() {
        format!("cache_{key}")
    } else {
        format!("cache_{rule}\0{key}")
    })
}

fn required_string<'a>(payload: &'a Value, name: &str) -> Result<&'a str, String> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("bridge payload field `{name}` must be a string"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_matches_bridge_contract() {
        let state = CoreState::default();
        let set = json!({"rule": "demo", "key": "theme", "value": "dark"});
        let get = json!({"rule": "demo", "key": "theme"});

        assert_eq!(dispatch("1", "cache.set", &set, &state).unwrap(), json!({}));
        assert_eq!(
            dispatch("2", "cache.get", &get, &state).unwrap(),
            json!("dark")
        );
        assert_eq!(dispatch("3", "cache.del", &get, &state).unwrap(), json!({}));
        assert_eq!(dispatch("4", "cache.get", &get, &state).unwrap(), json!(""));
    }

    #[test]
    fn device_info_reports_desktop_mode() {
        let state = CoreState::default();
        let result = dispatch("1", "device.info", &json!({}), &state).unwrap();

        assert_eq!(result["platform"], "windows");
        assert_eq!(result["mode"], "desktop");
        assert_eq!(result["isLeanback"], false);
    }

    #[test]
    fn player_status_does_not_start_a_player_window() {
        let state = CoreState::default();
        let result = dispatch("1", "player.status", &json!({}), &state).unwrap();

        assert_eq!(result["ready"], false);
        assert_eq!(result["idle"], false);
    }

    #[test]
    fn unknown_method_is_rejected() {
        let state = CoreState::default();
        let error = dispatch("1", "not.real", &json!({}), &state).unwrap_err();

        assert!(error.contains("not implemented"));
    }
}
