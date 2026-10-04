use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use percent_encoding::percent_encode;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    config::load_config_file,
    state::SharedState,
};

const MAX_MARKET_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketItem {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketCategory {
    pub name: String,
    pub list: Vec<MarketItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallResult {
    pub config_name: String,
    pub version: String,
    #[serde(default)]
    pub mode: String,
}

#[tauri::command]
pub async fn market_catalog(
    url: String,
    state: State<'_, SharedState>,
) -> Result<Vec<MarketCategory>, String> {
    let encoded = encode_market_url(url.trim())?;
    let response = state
        .http
        .get(encoded)
        .send()
        .await
        .map_err(|error| format!("unable to fetch the version catalog: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the version catalog request returned HTTP {status}"));
    }
    let text = response
        .text()
        .await
        .map_err(|error| format!("unable to read the version catalog: {error}"))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("the version catalog is not a valid JSON document: {error}"))
}

#[tauri::command]
pub async fn market_install(
    url: String,
    state: State<'_, SharedState>,
) -> Result<MarketInstallResult, String> {
    let state = state.inner().clone();
    let encoded = encode_market_url(url.trim())?;
    let response = state
        .http
        .get(encoded)
        .send()
        .await
        .map_err(|error| format!("unable to download the local package: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the local package request returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MARKET_BYTES)
    {
        return Err("the local package is larger than 64 MB".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("unable to download the local package: {error}"))?;
    if bytes.len() as u64 > MAX_MARKET_BYTES {
        return Err("the local package is larger than 64 MB".to_string());
    }

    let package_dir = market_root()?.join("packages");
    fs::create_dir_all(&package_dir)
        .map_err(|error| format!("unable to create the package directory: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let archive_path = package_dir.join(format!("{stamp}.zip"));
    fs::write(&archive_path, &bytes)
        .map_err(|error| format!("unable to save the local package: {error}"))?;

    let extract_dir = package_dir.join(stamp.to_string());
    fs::create_dir_all(&extract_dir)
        .map_err(|error| format!("unable to create the extract directory: {error}"))?;
    unzip(&archive_path, &extract_dir)?;

    let config_path = find_config_json(&extract_dir)
        .ok_or_else(|| "the local package does not contain a configuration (.json)".to_string())?;

    let (loaded, mode) = if let Some(active) = state
        .database
        .active_config()
        .map_err(|error| format!("unable to read the active configuration: {error}"))?
    {
        match replace_in_place(&extract_dir, &config_path, &active.summary.url) {
            Some(replaced) => (replaced, "replace".to_string()),
            None => fallback_import(&config_path)?,
        }
    } else {
        fallback_import(&config_path)?
    };
    let detail = state
        .database
        .save_config(&loaded.source, &loaded.name, &loaded.document, true)?;
    state.spiders.invalidate_all();

    let version = config_version(&config_path);
    Ok(MarketInstallResult {
        config_name: detail.summary.name,
        version,
        mode,
    })
}

fn fallback_import(
    config_path: &Path,
) -> Result<(crate::config::LoadedConfig, String), String> {
    let loaded = load_config_file(&config_path.to_string_lossy(), None)?;
    Ok((loaded, "import".to_string()))
}

fn replace_in_place(
    extract_dir: &Path,
    config_path: &Path,
    active_source: &str,
) -> Option<crate::config::LoadedConfig> {
    let source_file = file_url_to_local_path(active_source)?;
    if !source_file.is_file() {
        return None;
    }
    let source_dir = source_file.parent()?;
    let top_segment = config_path
        .strip_prefix(extract_dir)
        .ok()?
        .components()
        .next()?
        .as_os_str()
        .to_str()
        .map(str::to_string);
    let source_name = source_dir
        .file_name()?
        .to_str()
        .map(str::to_string);
    if top_segment != source_name {
        return None;
    }
    copy_tree(&extract_dir.join(top_segment?), source_dir).ok()?;
    load_config_file(&source_file.to_string_lossy(), None).ok()
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|error| format!("unable to read a package directory: {error}"))? {
        let entry = entry.map_err(|error| format!("unable to read a package entry: {error}"))?;
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| format!("unable to inspect a package entry: {error}"))?
            .is_dir()
        {
            fs::create_dir_all(&target)
                .map_err(|error| format!("unable to create a package directory: {error}"))?;
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(&entry.path(), &target).map_err(|error| {
                format!("unable to replace file `{}`: {error}", target.display())
            })?;
        }
    }
    Ok(())
}

fn file_url_to_local_path(source: &str) -> Option<PathBuf> {
    let url = reqwest::Url::parse(source).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

#[tauri::command]
pub fn app_restart(app: tauri::AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("unable to locate the app: {error}"))?;
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|error| format!("unable to restart the app: {error}"))?;
    app.exit(0);
    Ok(())
}

fn market_root() -> Result<PathBuf, String> {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        return Ok(PathBuf::from(local_app_data).join("webhtv-desktop"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .map(|dir| dir.join("market"))
        .ok_or_else(|| "unable to resolve the local package directory".to_string())
}

fn encode_market_url(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Err("the version catalog URL is empty".to_string());
    }
    let mut url = reqwest::Url::parse(raw).map_err(|error| format!("invalid URL: {error}"))?;
    if url.path().bytes().any(|byte| byte >= 0x80) {
        let encoded = url
            .path()
            .split('/')
            .map(|segment| percent_encode(segment.as_bytes(), percent_encoding::NON_ALPHANUMERIC).to_string())
            .collect::<Vec<_>>()
            .join("/");
        url.set_path(&encoded);
    }
    Ok(url.to_string())
}

const MAX_MARKET_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MARKET_ENTRIES: usize = 20_000;

fn unzip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("unable to open the local package: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("unable to read the local package: {error}"))?;
    if archive.len() > MAX_MARKET_ENTRIES {
        return Err("the local package contains too many entries".to_string());
    }
    let mut total_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("unable to read a package entry: {error}"))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err("the local package contains an unsafe path".to_string());
        };
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("unable to create a package directory: {error}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("unable to create a package directory: {error}"))?;
        }
        let mut output = File::create(&target)
            .map_err(|error| format!("unable to write a package file: {error}"))?;
        // ZIP-bomb guard: stop writing a single entry once it passes the remaining budget.
        let remaining = MAX_MARKET_EXTRACTED_BYTES.saturating_sub(total_bytes);
        if remaining == 0 {
            return Err("the local package is too large after extraction".to_string());
        }
        let written = io::copy(&mut entry.by_ref().take(remaining), &mut output)
            .map_err(|error| format!("unable to extract a package file: {error}"))?;
        total_bytes += written;
        if written >= remaining && entry.size() > written {
            return Err("the local package is too large after extraction".to_string());
        }
    }
    Ok(())
}

fn find_config_json(root: &Path) -> Option<PathBuf> {
    fn walk(directory: &Path, depth: usize, best: &mut Option<(usize, PathBuf)>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1, best);
                continue;
            }
            if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
                if best.as_ref().is_none_or(|(best_depth, _)| depth < *best_depth) {
                    *best = Some((depth, path));
                }
            }
        }
    }
    let mut best = None;
    walk(root, 0, &mut best);
    best.map(|(_, path)| path)
}

fn config_version(config_path: &Path) -> String {
    fs::read_to_string(config_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_url_encoding_preserves_ascii_paths() {
        let url = encode_market_url("https://example.com/catalog.json").unwrap();
        assert_eq!(url, "https://example.com/catalog.json");
    }

    #[test]
    fn market_url_encoding_encodes_unicode_path_segments() {
        let url = encode_market_url("https://example.com/本地包/版本.json").unwrap();
        assert!(url.starts_with("https://example.com/%E6%9C%AC%E5%9C%B0%E5%8C%85/"));
        assert!(url.ends_with("%E7%89%88%E6%9C%AC.json"));
    }

    #[test]
    fn config_finder_prefers_shallow_json_files() {
        let directory = std::env::temp_dir().join(format!("webhtv-market-test-{}", std::process::id()));
        let deep = directory.join("sub").join("deep.json");
        fs::create_dir_all(deep.parent().unwrap()).unwrap();
        fs::write(&deep, "{}").unwrap();
        fs::write(directory.join("top.json"), "{}").unwrap();
        let found = find_config_json(&directory).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "top.json");
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn file_source_converts_back_to_local_path() {
        let source = "file:///C:/Users/demo/AppData/Local/webhtv-desktop/packages/1/%E6%9F%92%E8%B1%AA/%E6%9F%92%E8%B1%AA4K.json";
        let path = file_url_to_local_path(source).unwrap();
        assert!(path.to_string_lossy().contains("柒豪4K.json"));
        assert!(file_url_to_local_path("https://example.com/config.json").is_none());
    }

    #[test]
    fn in_place_replace_updates_the_source_directory() {
        let directory = std::env::temp_dir().join(format!("webhtv-market-replace-{}", std::process::id()));
        let source_dir = directory.join("柒豪");
        fs::create_dir_all(source_dir.join("jar")).unwrap();
        let source_file = source_dir.join("柒豪4K.json");
        fs::write(&source_file, r#"{"spider":"./jar/spider.jar","sites":[{"key":"demo","type":3,"api":"csp_Demo"}]}"#).unwrap();
        let extract = directory.join("extract").join("柒豪");
        fs::create_dir_all(extract.join("jar")).unwrap();
        fs::write(extract.join("jar").join("柒豪.jar"), "new-jar").unwrap();
        fs::write(
            extract.join("柒豪4K.json"),
            r#"{"version":"08.09","sites":[{"key":"demo","type":3,"api":"csp_Demo"}]}"#,
        )
        .unwrap();
        let config_path = extract.join("柒豪4K.json");

        let source = {
            let url = reqwest::Url::from_file_path(&source_file).unwrap();
            url.to_string()
        };
        let loaded = replace_in_place(&directory.join("extract"), &config_path, &source).unwrap();
        assert!(loaded.source.starts_with("file:"));
        assert_eq!(fs::read(extract.join("jar").join("柒豪.jar")).unwrap(), b"new-jar");
        assert_eq!(
            fs::read(source_dir.join("jar").join("柒豪.jar")).unwrap(),
            b"new-jar"
        );
        fs::remove_dir_all(&directory).unwrap();
    }
}
