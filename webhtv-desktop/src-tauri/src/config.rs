use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use reqwest::Url;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

const DEFAULT_TIMEOUT_SECONDS: i32 = 15;
const MAX_CONFIG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_DEPOT_DEPTH: usize = 4;
const CONFIG_USER_AGENT: &str = "okhttp/4.12.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VodConfigDocument {
    #[serde(default)]
    pub spider: String,
    #[serde(default)]
    pub sites: Vec<Site>,
    #[serde(default)]
    pub parses: Vec<Parse>,
    #[serde(default)]
    pub lives: Value,
    #[serde(default)]
    pub doh: Value,
    #[serde(default)]
    pub proxy: Value,
    #[serde(default)]
    pub headers: Value,
    #[serde(default)]
    pub rules: Value,
    #[serde(default)]
    pub hls_rules: Value,
    #[serde(default)]
    pub group_rules: Value,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    pub hosts: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    pub ads: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    pub flags: Vec<String>,
    #[serde(default)]
    pub wallpaper: String,
    #[serde(default)]
    pub logo: String,
    #[serde(default)]
    pub notice: String,
    #[serde(default)]
    pub danmaku: String,
    #[serde(default)]
    pub home: String,
    #[serde(default)]
    pub parse: String,
    #[serde(default)]
    pub web_home_extensions: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default, deserialize_with = "deserialize_i32")]
    pub site_type: i32,
    #[serde(default)]
    pub api: String,
    #[serde(default)]
    pub ext: Value,
    #[serde(default)]
    pub jar: String,
    #[serde(default)]
    pub click: String,
    #[serde(default)]
    pub play_url: String,
    #[serde(default, alias = "home_page", alias = "webHome", alias = "web_home")]
    pub home_page: String,
    #[serde(default)]
    pub chrome_mode: String,
    #[serde(default)]
    pub web_home_chrome: Value,
    #[serde(default)]
    pub extensions: Value,
    #[serde(default, deserialize_with = "deserialize_i32")]
    pub hide: i32,
    #[serde(default, deserialize_with = "deserialize_i32")]
    pub indexs: i32,
    #[serde(default = "default_timeout", deserialize_with = "deserialize_timeout")]
    pub timeout: i32,
    #[serde(default = "default_one", deserialize_with = "deserialize_i32_one")]
    pub searchable: i32,
    #[serde(default = "default_one", deserialize_with = "deserialize_i32_one")]
    pub changeable: i32,
    #[serde(default = "default_one", deserialize_with = "deserialize_i32_one")]
    pub quick_search: i32,
    #[serde(default, deserialize_with = "deserialize_string_list")]
    pub categories: Vec<String>,
    #[serde(default)]
    pub header: Value,
    #[serde(default)]
    pub style: Option<Style>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    #[serde(rename = "type", default = "default_style_type")]
    pub style_type: String,
    #[serde(default)]
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parse {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default, deserialize_with = "deserialize_i32")]
    pub parse_type: i32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub ext: Value,
    #[serde(default)]
    pub header: Value,
    #[serde(default)]
    pub click: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Depot {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone)]
pub enum ConfigPayload {
    Document(Box<VodConfigDocument>),
    Depot(Vec<Depot>),
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub source: String,
    pub name: String,
    pub document: VodConfigDocument,
}

impl VodConfigDocument {
    pub fn normalize(&mut self, source_url: Option<&Url>) -> Result<(), String> {
        self.spider = resolve_resource(source_url, &self.spider, false);
        self.wallpaper = resolve_resource(source_url, &self.wallpaper, false);
        self.logo = resolve_resource(source_url, &self.logo, false);

        let mut seen = HashSet::new();
        let global_spider = self.spider.clone();
        self.sites.retain_mut(|site| {
            site.key = site.key.trim().to_string();
            if site.key.is_empty() || !seen.insert(site.key.clone()) {
                return false;
            }
            if site.name.trim().is_empty() {
                site.name = site.key.clone();
            }
            site.api = resolve_resource(source_url, &site.api, site.api.starts_with("csp_"));
            site.jar = resolve_resource(source_url, &site.jar, false);
            if site.jar.is_empty() {
                site.jar = global_spider.clone();
            }
            site.home_page = resolve_resource(source_url, &site.home_page, false);
            if let Value::String(ext) = &mut site.ext {
                *ext = resolve_ext(source_url, ext);
            }
            true
        });

        for parser in &mut self.parses {
            if parser.parse_type <= 1 {
                parser.url = resolve_resource(source_url, &parser.url, false);
            }
        }

        normalize_lives(&mut self.lives, source_url, &global_spider);

        if self.sites.is_empty() {
            return Err("configuration does not contain a valid site".to_string());
        }
        if self.home.is_empty() || !self.sites.iter().any(|site| site.key == self.home) {
            self.home = self.sites[0].key.clone();
        }
        if !self.parses.is_empty()
            && (self.parse.is_empty()
                || !self.parses.iter().any(|parser| parser.name == self.parse))
        {
            self.parse = self.parses[0].name.clone();
        }
        Ok(())
    }

    pub fn home_site(&self) -> Option<&Site> {
        self.sites
            .iter()
            .find(|site| site.key == self.home)
            .or_else(|| self.sites.first())
    }
}

fn normalize_lives(value: &mut Value, source_url: Option<&Url>, global_spider: &str) {
    match value {
        Value::Array(items) => {
            for item in items {
                normalize_live(item, source_url, global_spider);
            }
        }
        Value::Object(_) => normalize_live(value, source_url, global_spider),
        _ => {}
    }
}

fn normalize_live(value: &mut Value, source_url: Option<&Url>, global_spider: &str) {
    let Some(live) = value.as_object_mut() else {
        return;
    };
    for key in ["url", "logo", "epg"] {
        if let Some(Value::String(text)) = live.get_mut(key) {
            *text = resolve_resource(source_url, text, false);
        }
    }
    if let Some(Value::String(api)) = live.get_mut("api") {
        *api = resolve_resource(source_url, api, api.starts_with("csp_"));
    }
    let jar = live
        .get("jar")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let jar = if jar.trim().is_empty() {
        global_spider.to_string()
    } else {
        resolve_resource(source_url, &jar, false)
    };
    if !jar.is_empty() {
        live.insert("jar".to_string(), Value::String(jar));
    }
    if let Some(Value::String(ext)) = live.get_mut("ext") {
        *ext = resolve_ext(source_url, ext);
    }
}

pub fn parse_config_payload(json: &str, source_url: Option<&Url>) -> Result<ConfigPayload, String> {
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    let value: Value = serde_json::from_str(json)
        .map_err(|error| format!("configuration JSON is invalid: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "configuration root must be a JSON object".to_string())?;

    if let Some(message) = object.get("msg") {
        let message = message
            .as_str()
            .unwrap_or("configuration returned an error");
        return Err(if message.is_empty() {
            "configuration returned an empty error message".to_string()
        } else {
            message.to_string()
        });
    }

    if let Some(urls) = object.get("urls") {
        let depots: Vec<Depot> = serde_json::from_value(urls.clone())
            .map_err(|error| format!("configuration depot is invalid: {error}"))?;
        let depots: Vec<Depot> = depots
            .into_iter()
            .filter(|item| !item.url.trim().is_empty())
            .collect();
        if depots.is_empty() {
            return Err("configuration depot does not contain a URL".to_string());
        }
        return Ok(ConfigPayload::Depot(depots));
    }

    let mut document: VodConfigDocument = serde_json::from_value(value)
        .map_err(|error| format!("configuration fields are invalid: {error}"))?;
    document.normalize(source_url)?;
    Ok(ConfigPayload::Document(Box::new(document)))
}

pub fn resolve_depot_url(base: &Url, value: &str) -> Result<Url, String> {
    Url::parse(value)
        .or_else(|_| base.join(value))
        .map_err(|error| format!("depot URL is invalid: {error}"))
}

pub async fn load_config_url(
    client: &reqwest::Client,
    source: &str,
    preferred_name: Option<&str>,
) -> Result<LoadedConfig, String> {
    let mut current = Url::parse(source.trim())
        .map_err(|error| format!("configuration URL is invalid: {error}"))?;
    if !matches!(current.scheme(), "http" | "https") {
        return Err("configuration URL must use HTTP or HTTPS".to_string());
    }
    let mut name = preferred_name.unwrap_or_default().trim().to_string();

    for _ in 0..MAX_DEPOT_DEPTH {
        let (json, response_url) = fetch_config(client, &current).await?;
        match parse_config_payload(&json, Some(&response_url))? {
            ConfigPayload::Document(document) => {
                return Ok(LoadedConfig {
                    source: current.to_string(),
                    name,
                    document: *document,
                });
            }
            ConfigPayload::Depot(items) => {
                let item = &items[0];
                if name.is_empty() && !item.name.trim().is_empty() {
                    name = item.name.trim().to_string();
                }
                current = resolve_depot_url(&response_url, &item.url)?;
                if !matches!(current.scheme(), "http" | "https") {
                    return Err("depot configuration URL must use HTTP or HTTPS".to_string());
                }
            }
        }
    }
    Err(format!(
        "configuration depot exceeded {MAX_DEPOT_DEPTH} nested levels"
    ))
}

pub fn load_config_file(
    source: &str,
    preferred_name: Option<&str>,
) -> Result<LoadedConfig, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("configuration file path cannot be empty".to_string());
    }
    let path = Path::new(source)
        .canonicalize()
        .map_err(|error| format!("unable to open configuration file: {error}"))?;
    let metadata = path
        .metadata()
        .map_err(|error| format!("unable to inspect configuration file: {error}"))?;
    if !metadata.is_file() {
        return Err("configuration path is not a file".to_string());
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("configuration is larger than 10 MB".to_string());
    }
    let json = std::fs::read_to_string(&path)
        .map_err(|error| format!("unable to read configuration file: {error}"))?;
    let source_url = Url::from_file_path(&path)
        .map_err(|_| "configuration file path cannot be converted to a URL".to_string())?;
    let ConfigPayload::Document(document) = parse_config_payload(&json, Some(&source_url))? else {
        return Err("file import requires a direct configuration, not a depot".to_string());
    };
    let preferred_name = preferred_name.unwrap_or_default().trim();
    let name = if preferred_name.is_empty() {
        path.file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Local configuration".to_string())
    } else {
        preferred_name.to_string()
    };
    Ok(LoadedConfig {
        source: source_url.to_string(),
        name,
        document: *document,
    })
}

async fn fetch_config(client: &reqwest::Client, url: &Url) -> Result<(String, Url), String> {
    let response = client
        .get(url.clone())
        .header(reqwest::header::USER_AGENT, CONFIG_USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("unable to fetch configuration: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("configuration request returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CONFIG_BYTES)
    {
        return Err("configuration is larger than 10 MB".to_string());
    }
    let response_url = response.url().clone();
    let text = response
        .text()
        .await
        .map_err(|error| format!("unable to read configuration response: {error}"))?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err("configuration is larger than 10 MB".to_string());
    }
    Ok((text, response_url))
}

fn resolve_ext(source_url: Option<&Url>, value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return value.to_string();
    }
    resolve_resource(source_url, value, false)
}

fn resolve_resource(source_url: Option<&Url>, value: &str, preserve: bool) -> String {
    let value = value.trim();
    if value.is_empty() || preserve || has_non_http_scheme(value) {
        return value.to_string();
    }

    let (path, suffix) = value.split_once(';').unwrap_or((value, ""));
    let resolved = match (source_url, Url::parse(path)) {
        (_, Ok(url)) => url.to_string(),
        (Some(base), Err(_)) => base
            .join(path)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| path.to_string()),
        (None, Err(_)) => path.to_string(),
    };
    if suffix.is_empty() {
        resolved
    } else {
        format!("{resolved};{suffix}")
    }
}

fn has_non_http_scheme(value: &str) -> bool {
    [
        "csp_", "json:", "parse:", "file:", "local:", "assets:", "data:",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn default_one() -> i32 {
    1
}

fn default_timeout() -> i32 {
    DEFAULT_TIMEOUT_SECONDS
}

fn default_style_type() -> String {
    "rect".to_string()
}

fn deserialize_i32<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(value_to_i32(&value).unwrap_or_default())
}

fn deserialize_i32_one<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(value_to_i32(&value).unwrap_or(1))
}

fn deserialize_timeout<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(value_to_i32(&value)
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .max(1))
}

fn value_to_i32(value: &Value) -> Option<i32> {
    match value {
        Value::Number(number) => number.as_i64().and_then(|value| i32::try_from(value).ok()),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let values = match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(value) => Some(value),
                Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
            .collect(),
        Value::String(value) if !value.is_empty() => vec![value],
        _ => Vec::new(),
    };
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/vod-config.json");

    #[test]
    fn parses_and_normalizes_upstream_config_fields() {
        let source = Url::parse("https://example.com/config/config.json").unwrap();
        let ConfigPayload::Document(document) =
            parse_config_payload(FIXTURE, Some(&source)).unwrap()
        else {
            panic!("expected a document");
        };

        assert_eq!(document.home, "demo");
        assert_eq!(document.sites.len(), 2);
        assert!(document.sites[0].api.starts_with("csp_"));
        assert_eq!(
            document.sites[0].jar,
            "https://example.com/config/spider.jar"
        );
        assert_eq!(
            document.sites[0].home_page,
            "https://example.com/config/home.html"
        );
        assert_eq!(
            document.sites[1].home_page,
            "https://example.com/alias.html"
        );
        assert_eq!(document.parses[0].parse_type, 1);
        assert_eq!(
            document.parses[0].url,
            "https://example.com/config/parse?url="
        );
    }

    #[test]
    fn depot_is_detected_before_document_parsing() {
        let payload =
            parse_config_payload(r#"{"urls":[{"name":"Primary","url":"./vod.json"}]}"#, None)
                .unwrap();

        let ConfigPayload::Depot(items) = payload else {
            panic!("expected depot");
        };
        assert_eq!(items[0].name, "Primary");
    }

    #[test]
    fn duplicate_and_empty_site_keys_are_removed() {
        let payload = r#"{
          "sites": [
            {"key":"same","name":"First","type":3,"api":"csp_A"},
            {"key":"same","name":"Second","type":3,"api":"csp_B"},
            {"key":"","name":"Invalid","type":3,"api":"csp_C"}
          ]
        }"#;
        let ConfigPayload::Document(document) = parse_config_payload(payload, None).unwrap() else {
            panic!("expected document");
        };

        assert_eq!(document.sites.len(), 1);
        assert_eq!(document.sites[0].name, "First");
    }

    #[test]
    fn live_resources_inherit_spider_and_resolve_relative_urls() {
        let source = Url::parse("https://example.com/config/main.json").unwrap();
        let payload = r#"{
          "spider":"./jar/spider.jar",
          "sites":[{"key":"demo","type":3,"api":"csp_Demo"}],
          "lives":[{
            "name":"Demo Live",
            "url":"./live/list.m3u",
            "api":"csp_Live",
            "ext":"./live/ext.json",
            "logo":"./live/logo.png"
          }]
        }"#;
        let ConfigPayload::Document(document) =
            parse_config_payload(payload, Some(&source)).unwrap()
        else {
            panic!("expected document");
        };
        let live = document.lives.as_array().unwrap()[0].as_object().unwrap();
        assert_eq!(
            live.get("url").and_then(Value::as_str),
            Some("https://example.com/config/live/list.m3u")
        );
        assert_eq!(live.get("api").and_then(Value::as_str), Some("csp_Live"));
        assert_eq!(
            live.get("jar").and_then(Value::as_str),
            Some("https://example.com/config/jar/spider.jar")
        );
        assert_eq!(
            live.get("ext").and_then(Value::as_str),
            Some("https://example.com/config/live/ext.json")
        );
    }

    #[test]
    fn local_file_import_resolves_package_resources() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vod-config.json");
        let loaded = load_config_file(path.to_str().unwrap(), None).unwrap();

        assert!(loaded.source.starts_with("file:"));
        assert_eq!(loaded.name, "vod-config");
        assert!(loaded.document.spider.starts_with("file:"));
        assert!(loaded.document.sites[0].jar.starts_with("file:"));
        assert!(loaded.document.sites[1].api.starts_with("file:"));
    }

    #[test]
    fn url_import_uses_upstream_agent_and_redirect_base() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for request_index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                if request_index == 0 {
                    assert!(request
                        .to_ascii_lowercase()
                        .contains("user-agent: okhttp/4.12.0"));
                    write!(
                        stream,
                        "HTTP/1.1 302 Found\r\nLocation: /nested/config.json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                } else {
                    let body = r#"{"spider":"./jar/demo.jar","sites":[{"key":"demo","type":3,"api":"csp_Demo"}]}"#;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            }
        });

        let client = reqwest::Client::builder().build().unwrap();
        let source = format!("http://{address}/entry");
        let loaded =
            tauri::async_runtime::block_on(load_config_url(&client, &source, None)).unwrap();
        server.join().unwrap();

        assert_eq!(loaded.source, source);
        assert_eq!(
            loaded.document.spider,
            format!("http://{address}/nested/jar/demo.jar")
        );
    }
}
