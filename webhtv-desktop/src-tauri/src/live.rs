use std::collections::BTreeMap;

use reqwest::{header::HeaderName, Url};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::State;

use crate::{config::Site, spider::SpiderCall, state::SharedState};

const MAX_LIVE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSource {
    pub name: String,
    pub url: String,
    pub api: String,
    pub ext: Value,
    pub jar: String,
    pub logo: String,
    pub epg: String,
    pub ua: String,
    pub origin: String,
    pub referer: String,
    pub header: BTreeMap<String, String>,
    pub timeout: i32,
    pub groups: Vec<LiveGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSourceSummary {
    pub name: String,
    pub logo: String,
    pub epg: String,
    pub channel_count: usize,
    pub embedded: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveGroup {
    pub name: String,
    pub channels: Vec<LiveChannel>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveChannel {
    pub name: String,
    pub number: String,
    pub logo: String,
    pub tvg_id: String,
    pub tvg_name: String,
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCatalog {
    pub source: LiveSourceSummary,
    pub groups: Vec<LiveGroup>,
}

#[tauri::command]
pub fn live_sources(state: State<'_, SharedState>) -> Result<Vec<LiveSourceSummary>, String> {
    let active = state
        .database
        .active_config()?
        .ok_or_else(|| "no active configuration is available".to_string())?;
    Ok(parse_sources(&active.document.lives)
        .into_iter()
        .map(|source| source_summary(&source))
        .collect())
}

#[tauri::command]
pub async fn live_load(
    source_name: String,
    state: State<'_, SharedState>,
) -> Result<LiveCatalog, String> {
    let active = state
        .database
        .active_config()?
        .ok_or_else(|| "no active configuration is available".to_string())?;
    let sources = parse_sources(&active.document.lives);
    let mut source = sources
        .into_iter()
        .find(|source| source.name == source_name)
        .ok_or_else(|| format!("live source `{source_name}` was not found"))?;
    if source.groups.is_empty() {
        let text = if source.api.trim().is_empty() {
            fetch_live_text(&state, &source).await?
        } else {
            invoke_live_spider(&state, active.summary.id, &source).await?
        };
        source.groups = parse_playlist(&text)?;
    }
    normalize_groups(&mut source.groups);
    let summary = source_summary(&source);
    Ok(LiveCatalog {
        source: summary,
        groups: source.groups,
    })
}

async fn fetch_live_text(state: &SharedState, source: &LiveSource) -> Result<String, String> {
    let url = Url::parse(source.url.trim())
        .map_err(|error| format!("live source URL is invalid: {error}"))?;
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|_| "live file URL cannot be converted to a path".to_string())?;
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("unable to inspect live file: {error}"))?;
        if metadata.len() > MAX_LIVE_BYTES {
            return Err("live source is larger than 32 MB".to_string());
        }
        let bytes =
            std::fs::read(&path).map_err(|error| format!("unable to read live file: {error}"))?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err("live source URL must use HTTP, HTTPS, or file".to_string());
    }
    let mut request = state.http.get(url);
    for (name, value) in source_headers(source) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            value.parse::<reqwest::header::HeaderValue>(),
        ) {
            request = request.header(name, value);
        }
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("unable to fetch live source: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("live source request returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LIVE_BYTES)
    {
        return Err("live source is larger than 32 MB".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("unable to read live source response: {error}"))?;
    if bytes.len() as u64 > MAX_LIVE_BYTES {
        return Err("live source is larger than 32 MB".to_string());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn invoke_live_spider(
    state: &SharedState,
    config_id: i64,
    source: &LiveSource,
) -> Result<String, String> {
    let site: Site = serde_json::from_value(json!({
        "key": format!("live:{}", source.name),
        "name": source.name,
        "type": 3,
        "api": source.api,
        "ext": source.ext,
        "jar": source.jar,
        "timeout": source.timeout,
    }))
    .map_err(|error| format!("live Spider settings are invalid: {error}"))?;
    let call = SpiderCall::parse("liveContent", json!({"url": source.url}))?;
    let value = state.spiders.invoke(config_id, site, call).await?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "live Spider returned a non-text response".to_string())
}

fn source_headers(source: &LiveSource) -> BTreeMap<String, String> {
    let mut headers = source.header.clone();
    if !source.ua.is_empty() {
        headers.insert("User-Agent".to_string(), source.ua.clone());
    }
    if !source.origin.is_empty() {
        headers.insert("Origin".to_string(), source.origin.clone());
    }
    if !source.referer.is_empty() {
        headers.insert("Referer".to_string(), source.referer.clone());
    }
    headers
}

fn parse_sources(value: &Value) -> Vec<LiveSource> {
    match value {
        Value::Array(items) => items.iter().filter_map(source_from_value).collect(),
        Value::Object(_) => source_from_value(value).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn source_from_value(value: &Value) -> Option<LiveSource> {
    let object = value.as_object()?;
    let url = string_field(object, "url");
    let api = string_field(object, "api");
    let mut groups = object
        .get("groups")
        .map(parse_json_groups)
        .unwrap_or_default();
    normalize_groups(&mut groups);
    if url.is_empty() && api.is_empty() && groups.is_empty() {
        return None;
    }
    let name = string_field(object, "name");
    Some(LiveSource {
        name: if name.is_empty() {
            if url.is_empty() {
                api.clone()
            } else {
                url.clone()
            }
        } else {
            name
        },
        url,
        api,
        ext: object.get("ext").cloned().unwrap_or(Value::Null),
        jar: string_field(object, "jar"),
        logo: string_field(object, "logo"),
        epg: string_field(object, "epg"),
        ua: string_field(object, "ua"),
        origin: string_field(object, "origin"),
        referer: string_field(object, "referer"),
        header: string_map(object.get("header")),
        timeout: integer_field(object, "timeout").unwrap_or(15).clamp(1, 90),
        groups,
    })
}

fn source_summary(source: &LiveSource) -> LiveSourceSummary {
    LiveSourceSummary {
        name: source.name.clone(),
        logo: source.logo.clone(),
        epg: source.epg.clone(),
        channel_count: source.groups.iter().map(|group| group.channels.len()).sum(),
        embedded: !source.groups.is_empty(),
    }
}

fn parse_playlist(text: &str) -> Result<Vec<LiveGroup>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim();
    if text.is_empty() {
        return Err("live source returned empty content".to_string());
    }
    let mut groups = if text.starts_with('[') || text.starts_with('{') {
        let value: Value =
            serde_json::from_str(text).map_err(|error| format!("live JSON is invalid: {error}"))?;
        parse_json_groups(&value)
    } else if text.contains("#EXTM3U") || text.contains("#EXTINF:") {
        parse_m3u(text)
    } else {
        parse_txt(text)
    };
    normalize_groups(&mut groups);
    if groups.is_empty() {
        Err("live source does not contain a playable channel".to_string())
    } else {
        Ok(groups)
    }
}

fn parse_json_groups(value: &Value) -> Vec<LiveGroup> {
    let values = match value {
        Value::Array(items) => items.as_slice(),
        Value::Object(object) => object
            .get("groups")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        _ => &[],
    };
    values
        .iter()
        .filter_map(|value| {
            let object = value.as_object()?;
            let name = string_field(object, "name");
            let channels = object
                .get("channel")
                .or_else(|| object.get("channels"))
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(channel_from_value).collect())
                .unwrap_or_default();
            Some(LiveGroup { name, channels })
        })
        .collect()
}

fn channel_from_value(value: &Value) -> Option<LiveChannel> {
    let object = value.as_object()?;
    let name = string_field(object, "name");
    let urls = object
        .get("urls")
        .or_else(|| object.get("url"))
        .map(string_list)
        .unwrap_or_default();
    if name.is_empty() || urls.is_empty() {
        return None;
    }
    Some(LiveChannel {
        name,
        number: string_field(object, "number"),
        logo: string_field(object, "logo"),
        tvg_id: string_field(object, "tvgId"),
        tvg_name: string_field(object, "tvgName"),
        urls,
    })
}

fn parse_m3u(text: &str) -> Vec<LiveGroup> {
    let mut groups = Vec::new();
    let mut pending: Option<(String, String, String, String, String)> = None;
    let mut ext_group = String::new();
    for raw in normalized_lines(text) {
        let line = raw.trim();
        if let Some(value) = line.strip_prefix("#EXTGRP:") {
            ext_group = value.trim().to_string();
        } else if line.starts_with("#EXTINF:") {
            let name = line
                .rsplit_once(',')
                .map(|(_, name)| name.trim())
                .unwrap_or_default();
            if is_meta_channel(name) {
                pending = None;
                continue;
            }
            let group = attribute(line, "group-title");
            let logo = attribute(line, "tvg-logo");
            let tvg_id = attribute(line, "tvg-id");
            let tvg_name = attribute(line, "tvg-name");
            pending = Some((
                name.to_string(),
                if group.is_empty() {
                    ext_group.clone()
                } else {
                    group
                },
                logo,
                tvg_id,
                tvg_name,
            ));
        } else if !line.is_empty() && !line.starts_with('#') {
            let Some((name, group, logo, tvg_id, tvg_name)) = pending.as_ref() else {
                continue;
            };
            let url = strip_stream_options(line);
            if !is_playable_url(url) {
                continue;
            }
            add_channel(
                &mut groups,
                group,
                LiveChannel {
                    name: name.clone(),
                    number: String::new(),
                    logo: logo.clone(),
                    tvg_id: tvg_id.clone(),
                    tvg_name: tvg_name.clone(),
                    urls: vec![url.to_string()],
                },
            );
        }
    }
    groups
}

fn parse_txt(text: &str) -> Vec<LiveGroup> {
    let mut groups = Vec::new();
    let mut current_group = String::new();
    for raw in normalized_lines(text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, urls)) = line.split_once(',') else {
            continue;
        };
        let name = name.trim();
        if line.contains("#genre#") {
            current_group = name.to_string();
            continue;
        }
        if name.is_empty() || is_meta_channel(name) {
            continue;
        }
        for raw_url in urls.split('#') {
            let url = strip_stream_options(raw_url.trim());
            if !is_playable_url(url) {
                continue;
            }
            add_channel(
                &mut groups,
                &current_group,
                LiveChannel {
                    name: name.to_string(),
                    urls: vec![url.to_string()],
                    ..LiveChannel::default()
                },
            );
        }
    }
    groups
}

fn add_channel(groups: &mut Vec<LiveGroup>, group_name: &str, channel: LiveChannel) {
    let group_name = if group_name.trim().is_empty() {
        "直播"
    } else {
        group_name.trim()
    };
    let group = if let Some(index) = groups.iter().position(|group| group.name == group_name) {
        &mut groups[index]
    } else {
        groups.push(LiveGroup {
            name: group_name.to_string(),
            channels: Vec::new(),
        });
        groups.last_mut().expect("inserted live group must exist")
    };
    if let Some(existing) = group
        .channels
        .iter_mut()
        .find(|existing| existing.name == channel.name)
    {
        for url in channel.urls {
            if !existing.urls.contains(&url) {
                existing.urls.push(url);
            }
        }
        if existing.logo.is_empty() {
            existing.logo = channel.logo;
        }
    } else {
        group.channels.push(channel);
    }
}

fn normalize_groups(groups: &mut Vec<LiveGroup>) {
    let mut number = 0_u32;
    groups.retain_mut(|group| {
        if group.name.trim().is_empty() {
            group.name = "直播".to_string();
        }
        group.channels.retain_mut(|channel| {
            channel.name = channel.name.trim().to_string();
            channel
                .urls
                .retain(|url| is_playable_url(strip_stream_options(url)));
            channel.urls.sort();
            channel.urls.dedup();
            if channel.name.is_empty() || channel.urls.is_empty() {
                return false;
            }
            number = number.saturating_add(1);
            if channel.number.is_empty() {
                channel.number = format!("{number:03}");
            }
            true
        });
        !group.channels.is_empty()
    });
}

fn normalized_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|line| line.trim_end_matches('\r'))
}

fn attribute(line: &str, key: &str) -> String {
    let marker = format!("{key}=\"");
    if let Some(start) = line.find(&marker) {
        let value = &line[start + marker.len()..];
        return value
            .find('"')
            .map(|end| value[..end].trim().to_string())
            .unwrap_or_default();
    }
    let marker = format!("{key}=");
    line.find(&marker)
        .map(|start| {
            line[start + marker.len()..]
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches('"')
                .to_string()
        })
        .unwrap_or_default()
}

fn strip_stream_options(url: &str) -> &str {
    url.split_once('|')
        .map(|(url, _)| url)
        .unwrap_or(url)
        .trim()
}

fn is_playable_url(url: &str) -> bool {
    url.contains("://")
}

fn is_meta_channel(name: &str) -> bool {
    let name = name.trim().to_lowercase();
    [
        "更新时间",
        "更新日期",
        "update time",
        "update date",
        "last update",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn string_field(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn integer_field(object: &Map<String, Value>, key: &str) -> Option<i32> {
    match object.get(key) {
        Some(Value::Number(value)) => value.as_i64().and_then(|value| i32::try_from(value).ok()),
        Some(Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(Value::Object(object)) = value else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

fn string_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => value
            .split('#')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m3u_groups_channels_and_alternate_urls() {
        let groups = parse_playlist(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"cctv1\" tvg-logo=\"logo.png\" group-title=\"央视\",CCTV-1\nhttps://one/live.m3u8\nhttps://two/live.m3u8|User-Agent=test\n",
        )
        .unwrap();
        assert_eq!(groups[0].name, "央视");
        assert_eq!(groups[0].channels[0].name, "CCTV-1");
        assert_eq!(groups[0].channels[0].urls.len(), 2);
        assert_eq!(groups[0].channels[0].tvg_id, "cctv1");
    }

    #[test]
    fn parses_tvbox_txt_and_skips_metadata() {
        let groups = parse_playlist(
            "央视频道,#genre#\n更新时间,2026-07-24\nCCTV-1,https://one/live.m3u8#https://two/live.m3u8\n",
        )
        .unwrap();
        assert_eq!(groups[0].name, "央视频道");
        assert_eq!(groups[0].channels.len(), 1);
        assert_eq!(groups[0].channels[0].urls.len(), 2);
    }

    #[test]
    fn parses_embedded_json_groups() {
        let groups = parse_playlist(
            r#"[{"name":"News","channel":[{"name":"Demo","urls":["https://example.com/live.m3u8"]}]}]"#,
        )
        .unwrap();
        assert_eq!(groups[0].name, "News");
        assert_eq!(groups[0].channels[0].number, "001");
    }
}
