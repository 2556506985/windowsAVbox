use std::{collections::HashMap, sync::Mutex, time::Duration};

use serde::Deserialize;
use serde_json::{Map, Value};

use std::path::PathBuf;

use crate::{
    config::Site,
    spider_java::{java_work_dir, JavaHandle, JavaParserRequest, JavaSpec},
    spider_quickjs::{QuickJsHandle, QuickJsSpec},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiderRuntimeKind {
    QuickJs,
    Python,
    Java,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputKind {
    Json,
    Text,
    Bool,
}

#[derive(Debug, Clone)]
pub struct SpiderCall {
    method: String,
    js_method: &'static str,
    arguments: Vec<Value>,
    output: OutputKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum RawOutput {
    Missing,
    Undefined,
    Value { value: Value },
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RuntimeKey {
    config_id: i64,
    site_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeIdentity {
    api: String,
    ext: String,
    jar: String,
    timeout: i32,
}

enum RuntimeHandle {
    QuickJs(QuickJsHandle),
    Java(JavaHandle),
}

struct RuntimeEntry {
    identity: RuntimeIdentity,
    handle: RuntimeHandle,
}

pub struct SpiderManager {
    entries: Mutex<HashMap<RuntimeKey, RuntimeEntry>>,
    work_dir: PathBuf,
}

impl Default for SpiderManager {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            work_dir: std::env::temp_dir().join("webhtv-desktop-spiders"),
        }
    }
}

impl SpiderManager {
    pub fn with_work_dir(work_dir: PathBuf) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            work_dir,
        }
    }
}

impl SpiderCall {
    pub fn parse(method: &str, args: Value) -> Result<Self, String> {
        let args = args
            .as_object()
            .ok_or_else(|| "Spider arguments must be a JSON object".to_string())?;
        let call = match method {
            "homeContent" => Self::new(
                method,
                "home",
                vec![Value::Bool(optional_bool(args, "filter", false)?)],
                OutputKind::Json,
            ),
            "homeVideoContent" => Self::new(method, "homeVod", vec![], OutputKind::Json),
            "categoryContent" => Self::new(
                method,
                "category",
                vec![
                    Value::String(required_string(args, "tid")?),
                    Value::String(optional_string(args, "page", "1")?),
                    Value::Bool(optional_bool(args, "filter", false)?),
                    optional_object(args, "extend")?,
                ],
                OutputKind::Json,
            ),
            "detailContent" => Self::new(
                method,
                "detail",
                vec![Value::String(first_string(args, "ids")?)],
                OutputKind::Json,
            ),
            "searchContent" => {
                let mut arguments = vec![
                    Value::String(required_string(args, "key")?),
                    Value::Bool(optional_bool(args, "quick", false)?),
                ];
                if let Some(page) = optional_present_string(args, "page")? {
                    arguments.push(Value::String(page));
                }
                Self::new(method, "search", arguments, OutputKind::Json)
            }
            "playerContent" => Self::new(
                method,
                "play",
                vec![
                    Value::String(required_string(args, "flag")?),
                    Value::String(required_string(args, "id")?),
                    string_array(args, "vipFlags")?,
                ],
                OutputKind::Json,
            ),
            "liveContent" => Self::new(
                method,
                "live",
                vec![Value::String(required_string(args, "url")?)],
                OutputKind::Text,
            ),
            "manualVideoCheck" => Self::new(method, "sniffer", vec![], OutputKind::Bool),
            "isVideoFormat" => Self::new(
                method,
                "isVideo",
                vec![Value::String(required_string(args, "url")?)],
                OutputKind::Bool,
            ),
            "action" => Self::new(
                method,
                "action",
                vec![Value::String(required_string(args, "action")?)],
                OutputKind::Json,
            ),
            "configSet" => Self::new(
                method,
                "config",
                vec![
                    Value::String(required_string(args, "key")?.to_string()),
                    Value::String(required_string(args, "value")?.to_string()),
                ],
                OutputKind::Json,
            ),
            "configGet" => Self::new(
                method,
                "config",
                vec![Value::String(required_string(args, "key")?.to_string())],
                OutputKind::Text,
            ),
            "authStart" | "authClear" | "authStatus" => Self::new(
                method,
                "auth",
                vec![Value::String(required_string(args, "provider")?)],
                OutputKind::Json,
            ),
            "authPoll" | "authCancel" => Self::new(
                method,
                "auth",
                vec![Value::String(required_string(args, "sessionId")?)],
                OutputKind::Json,
            ),
            _ => return Err(format!("unsupported Spider method `{method}`")),
        };
        Ok(call)
    }

    fn new(
        method: &str,
        js_method: &'static str,
        arguments: Vec<Value>,
        output: OutputKind,
    ) -> Self {
        Self {
            method: method.to_string(),
            js_method,
            arguments,
            output,
        }
    }

    pub(super) fn method(&self) -> &str {
        &self.method
    }

    pub(super) fn js_method(&self) -> &'static str {
        self.js_method
    }

    pub(super) fn arguments(&self) -> &[Value] {
        &self.arguments
    }

    pub(super) fn normalize(&self, output: RawOutput) -> Result<Value, String> {
        let value = match output {
            RawOutput::Missing | RawOutput::Undefined => None,
            RawOutput::Value { value } => Some(value),
        };
        match self.output {
            OutputKind::Json => normalize_json(value, &self.method),
            OutputKind::Text => match value {
                None | Some(Value::Null) => Ok(Value::String(String::new())),
                Some(Value::String(text)) => Ok(Value::String(text)),
                Some(other) => Err(format!(
                    "Spider method `{}` returned {}, expected a string",
                    self.method,
                    json_type(&other)
                )),
            },
            OutputKind::Bool => match value {
                None | Some(Value::Null) => Ok(Value::Bool(false)),
                Some(Value::Bool(value)) => Ok(Value::Bool(value)),
                Some(other) => Err(format!(
                    "Spider method `{}` returned {}, expected a boolean",
                    self.method,
                    json_type(&other)
                )),
            },
        }
    }
}

impl SpiderManager {
    pub async fn invoke(
        &self,
        config_id: i64,
        site: Site,
        call: SpiderCall,
    ) -> Result<Value, String> {
        let kind = runtime_kind(&site);
        match kind {
            SpiderRuntimeKind::QuickJs | SpiderRuntimeKind::Java => {}
            SpiderRuntimeKind::Python => {
                return Err(format!(
                    "Python Spider runtime is not implemented for site `{}`",
                    site.key
                ));
            }
            SpiderRuntimeKind::Unsupported => {
                return Err(format!("site `{}` does not use a Spider runtime", site.key));
            }
        }

        let (key, identity, handle) = self.ensure_runtime(config_id, &site, kind).await?;

        let worker_site = site.clone();
        let worker_call = call.clone();
        let worker_handle = handle.clone();
        let invocation =
            tauri::async_runtime::spawn_blocking(move || worker_handle.invoke(worker_site, worker_call))
                .await
                .map_err(|error| format!("Spider invocation task failed: {error}"))?;
        let output = match invocation {
            Ok(output) => output,
            Err(error) => {
                // Only drop broken runtimes for transport/process-level failures.
                // Business errors from the site or auth timeouts must not kill the shared JVM.
                if is_transport_failure(&error) {
                    self.remove_handle(&key, &identity, &handle);
                }
                return Err(format!(
                    "site `{}` method `{}` failed: {error}",
                    site.key, call.method
                ));
            }
        };
        call.normalize(output)
    }

    pub async fn parse(
        &self,
        config_id: i64,
        site: Site,
        request: JavaParserRequest,
    ) -> Result<Value, String> {
        if runtime_kind(&site) != SpiderRuntimeKind::Java {
            return Err(format!(
                "site `{}` does not use a Java Spider runtime",
                site.key
            ));
        }
        let (key, identity, handle) = self
            .ensure_runtime(config_id, &site, SpiderRuntimeKind::Java)
            .await?;
        let worker_site = site.clone();
        let worker_handle = handle.clone();
        let invocation =
            tauri::async_runtime::spawn_blocking(move || worker_handle.parse(worker_site, request))
                .await
                .map_err(|error| format!("Java parser task failed: {error}"))?;
        let output = match invocation {
            Ok(output) => output,
            Err(error) => {
                if is_transport_failure(&error) {
                    self.remove_handle(&key, &identity, &handle);
                }
                return Err(format!("site `{}` parser failed: {error}", site.key));
            }
        };
        let value = match output {
            RawOutput::Missing | RawOutput::Undefined => None,
            RawOutput::Value { value } => Some(value),
        };
        normalize_json(value, "JAR parser")
    }

    async fn ensure_runtime(
        &self,
        config_id: i64,
        site: &Site,
        kind: SpiderRuntimeKind,
    ) -> Result<(RuntimeKey, RuntimeIdentity, RuntimeHandle), String> {
        let key = runtime_key(config_id, site, kind);
        let identity = RuntimeIdentity::for_runtime(site, kind);
        let (cached, stale) = self.cached_handle(&key, &identity)?;
        if let Some(handle) = stale {
            handle.shutdown();
        }
        let handle = if let Some(handle) = cached {
            handle
        } else {
            let created = match kind {
                SpiderRuntimeKind::QuickJs => {
                    let spec = QuickJsSpec::from_site(site);
                    let created =
                        tauri::async_runtime::spawn_blocking(move || QuickJsHandle::spawn(spec))
                            .await
                            .map_err(|error| format!("QuickJS worker task failed: {error}"))??;
                    RuntimeHandle::QuickJs(created)
                }
                SpiderRuntimeKind::Java => {
                    let work_dir = java_work_dir(&self.work_dir, config_id, &site.key);
                    let spec = JavaSpec::from_site(site, work_dir)?;
                    let created =
                        tauri::async_runtime::spawn_blocking(move || JavaHandle::spawn(spec))
                            .await
                            .map_err(|error| format!("Java worker task failed: {error}"))??;
                    RuntimeHandle::Java(created)
                }
                SpiderRuntimeKind::Python | SpiderRuntimeKind::Unsupported => {
                    unreachable!("runtime kind already filtered")
                }
            };
            self.install_handle(key.clone(), identity.clone(), created)?
        };
        Ok((key, identity, handle))
    }

    pub fn invalidate_all(&self) {
        let handles = match self.entries.lock() {
            Ok(mut entries) => entries
                .drain()
                .map(|(_, entry)| entry.handle)
                .collect::<Vec<_>>(),
            Err(poisoned) => poisoned
                .into_inner()
                .drain()
                .map(|(_, entry)| entry.handle)
                .collect::<Vec<_>>(),
        };
        for handle in handles {
            handle.shutdown();
        }
    }

    fn cached_handle(
        &self,
        key: &RuntimeKey,
        identity: &RuntimeIdentity,
    ) -> Result<(Option<RuntimeHandle>, Option<RuntimeHandle>), String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Spider runtime registry is unavailable".to_string())?;
        if entries
            .get(key)
            .is_some_and(|entry| entry.identity == *identity)
        {
            return Ok((entries.get(key).map(|entry| entry.handle.clone()), None));
        }
        Ok((None, entries.remove(key).map(|entry| entry.handle)))
    }

    fn install_handle(
        &self,
        key: RuntimeKey,
        identity: RuntimeIdentity,
        created: RuntimeHandle,
    ) -> Result<RuntimeHandle, String> {
        let (selected, discarded) = {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| "Spider runtime registry is unavailable".to_string())?;
            if let Some(entry) = entries.get(&key).filter(|entry| entry.identity == identity) {
                (entry.handle.clone(), Some(created))
            } else {
                let selected = created.clone();
                let replaced = entries
                    .insert(
                        key,
                        RuntimeEntry {
                            identity,
                            handle: created,
                        },
                    )
                    .map(|entry| entry.handle);
                (selected, replaced)
            }
        };
        if let Some(handle) = discarded {
            handle.shutdown();
        }
        Ok(selected)
    }

    fn remove_handle(&self, key: &RuntimeKey, identity: &RuntimeIdentity, handle: &RuntimeHandle) {
        let removed = self.entries.lock().ok().and_then(|mut entries| {
            entries
                .get(key)
                .filter(|entry| {
                    entry.identity == *identity && entry.handle.same_instance(handle)
                })
                .is_some()
                .then(|| entries.remove(key))
                .flatten()
        });
        if let Some(entry) = removed {
            if entry.handle.same_instance(handle) {
                entry.handle.shutdown();
            }
        }
    }
}

impl RuntimeHandle {
    fn same_instance(&self, other: &RuntimeHandle) -> bool {
        match (self, other) {
            (Self::QuickJs(a), Self::QuickJs(b)) => a.same_instance(b),
            (Self::Java(a), Self::Java(b)) => a.same_instance(b),
            _ => false,
        }
    }
}

impl RuntimeHandle {
    fn invoke(&self, site: Site, call: SpiderCall) -> Result<RawOutput, String> {
        match self {
            Self::QuickJs(handle) => {
                let timeout = Duration::from_secs(site.timeout.max(5) as u64);
                handle.invoke(call, timeout)
            }
            Self::Java(handle) => handle.invoke(site, call),
        }
    }

    fn shutdown(&self) {
        match self {
            Self::QuickJs(handle) => handle.shutdown(),
            Self::Java(handle) => handle.shutdown(),
        }
    }

    fn parse(&self, site: Site, request: JavaParserRequest) -> Result<RawOutput, String> {
        match self {
            Self::Java(handle) => handle.parse(site, request),
            Self::QuickJs(_) => Err("JAR parser requires a Java Spider runtime".to_string()),
        }
    }
}

impl Clone for RuntimeHandle {
    fn clone(&self) -> Self {
        match self {
            Self::QuickJs(handle) => Self::QuickJs(handle.clone()),
            Self::Java(handle) => Self::Java(handle.clone()),
        }
    }
}

impl RuntimeIdentity {
    fn for_runtime(site: &Site, kind: SpiderRuntimeKind) -> Self {
        if kind == SpiderRuntimeKind::Java {
            return Self {
                api: "csp_shared_java".to_string(),
                ext: String::new(),
                jar: String::new(),
                timeout: 0,
            };
        }
        Self {
            api: site.api.clone(),
            ext: serde_json::to_string(&site.ext).unwrap_or_default(),
            jar: site.jar.clone(),
            timeout: site.timeout,
        }
    }
}

fn runtime_key(config_id: i64, site: &Site, kind: SpiderRuntimeKind) -> RuntimeKey {
    RuntimeKey {
        config_id,
        site_key: if kind == SpiderRuntimeKind::Java {
            "__shared_java__".to_string()
        } else {
            site.key.clone()
        },
    }
}

pub fn runtime_kind(site: &Site) -> SpiderRuntimeKind {
    if site.site_type != 3 {
        SpiderRuntimeKind::Unsupported
    } else if site.api.contains(".py") {
        SpiderRuntimeKind::Python
    } else if site.api.contains(".js") {
        SpiderRuntimeKind::QuickJs
    } else if site.api.starts_with("csp_") {
        SpiderRuntimeKind::Java
    } else {
        SpiderRuntimeKind::Unsupported
    }
}

fn is_transport_failure(error: &str) -> bool {
    error.contains("no longer available")
        || error.contains("stopped before replying")
        || error.contains("stopped during initialization")
        || error.contains("timed out after")
        || error.contains("engine is no longer running")
}

fn normalize_json(value: Option<Value>, method: &str) -> Result<Value, String> {
    match value {
        None | Some(Value::Null) => Ok(Value::Object(Map::new())),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Some(Value::String(text)) => serde_json::from_str(&text)
            .map_err(|error| format!("Spider method `{method}` returned invalid JSON: {error}")),
        Some(value @ (Value::Object(_) | Value::Array(_))) => Ok(value),
        Some(other) => Err(format!(
            "Spider method `{method}` returned {}, expected JSON text or a JSON value",
            json_type(&other)
        )),
    }
}

fn required_string(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Spider argument `{key}` must be a non-empty string"))
}

fn optional_present_string(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("Spider argument `{key}` must be a string")),
    }
}

fn optional_string(args: &Map<String, Value>, key: &str, default: &str) -> Result<String, String> {
    Ok(optional_present_string(args, key)?.unwrap_or_else(|| default.to_string()))
}

fn optional_bool(args: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("Spider argument `{key}` must be a boolean")),
    }
}

fn optional_object(args: &Map<String, Value>, key: &str) -> Result<Value, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Value::Object(Map::new())),
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        Some(_) => Err(format!("Spider argument `{key}` must be an object")),
    }
}

fn first_string(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Spider argument `{key}` must contain a non-empty string"))
}

fn string_array(args: &Map<String, Value>, key: &str) -> Result<Value, String> {
    let values = match args.get(key) {
        None | Some(Value::Null) => return Ok(Value::Array(Vec::new())),
        Some(Value::Array(values)) => values,
        Some(_) => {
            return Err(format!(
                "Spider argument `{key}` must be an array of strings"
            ))
        }
    };
    if values.iter().all(Value::is_string) {
        Ok(Value::Array(values.clone()))
    } else {
        Err(format!(
            "Spider argument `{key}` must be an array of strings"
        ))
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use serde_json::json;

    use super::*;

    fn site(site_type: i32, api: &str) -> Site {
        serde_json::from_value(json!({
            "key": "test",
            "name": "Test",
            "type": site_type,
            "api": api
        }))
        .unwrap()
    }

    #[test]
    fn runtime_selection_matches_upstream_order() {
        assert_eq!(
            runtime_kind(&site(3, "demo.py.js")),
            SpiderRuntimeKind::Python
        );
        assert_eq!(
            runtime_kind(&site(3, "demo.js")),
            SpiderRuntimeKind::QuickJs
        );
        assert_eq!(runtime_kind(&site(3, "csp_Demo")), SpiderRuntimeKind::Java);
        assert_eq!(
            runtime_kind(&site(1, "https://example.com/demo.js")),
            SpiderRuntimeKind::Unsupported
        );
    }

    #[test]
    fn java_sites_share_one_runtime_per_config() {
        let mut first = site(3, "csp_Douban");
        first.key = "first".to_string();
        let mut second = site(3, "csp_Wogg");
        second.key = "second".to_string();

        assert_eq!(
            runtime_key(7, &first, SpiderRuntimeKind::Java),
            runtime_key(7, &second, SpiderRuntimeKind::Java)
        );
        assert_eq!(
            RuntimeIdentity::for_runtime(&first, SpiderRuntimeKind::Java),
            RuntimeIdentity::for_runtime(&second, SpiderRuntimeKind::Java)
        );
        assert_ne!(
            runtime_key(7, &first, SpiderRuntimeKind::Java),
            runtime_key(8, &first, SpiderRuntimeKind::Java)
        );
    }

    #[test]
    fn canonical_calls_map_to_quickjs_arguments() {
        let call = SpiderCall::parse(
            "categoryContent",
            json!({"tid":"movie","page":"2","filter":true,"extend":{"year":"2026"}}),
        )
        .unwrap();
        assert_eq!(call.js_method(), "category");
        assert_eq!(
            call.arguments(),
            &[
                json!("movie"),
                json!("2"),
                json!(true),
                json!({"year":"2026"})
            ]
        );

        let detail = SpiderCall::parse("detailContent", json!({"ids":["vod-1","vod-2"]})).unwrap();
        assert_eq!(detail.js_method(), "detail");
        assert_eq!(detail.arguments(), &[json!("vod-1")]);

        let config = SpiderCall::parse(
            "configSet",
            json!({"key":"quarkQuality","value":"夸克无限"}),
        )
        .unwrap();
        assert_eq!(config.js_method(), "config");
        assert_eq!(
            config.arguments(),
            &[json!("quarkQuality"), json!("夸克无限")]
        );

        let auth = SpiderCall::parse("authStart", json!({"provider":"quark"})).unwrap();
        assert_eq!(auth.js_method(), "auth");
        assert_eq!(auth.arguments(), &[json!("quark")]);

        let poll = SpiderCall::parse("authPoll", json!({"sessionId":"session-1"})).unwrap();
        assert_eq!(poll.js_method(), "auth");
        assert_eq!(poll.arguments(), &[json!("session-1")]);
    }

    #[test]
    fn results_are_normalized_by_contract() {
        let call = SpiderCall::parse("homeContent", json!({})).unwrap();
        assert_eq!(
            call.normalize(RawOutput::Value {
                value: json!("{\"class\":[]}")
            })
            .unwrap(),
            json!({"class":[]})
        );
        assert_eq!(call.normalize(RawOutput::Missing).unwrap(), json!({}));
        assert!(call
            .normalize(RawOutput::Value {
                value: json!("not-json")
            })
            .unwrap_err()
            .contains("invalid JSON"));
    }

    #[test]
    fn manager_loads_http_modules_and_invokes_worker() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let body = if request.starts_with("GET /dep.js ") {
                    "export const value = 'worker-ok';"
                } else {
                    "import { value } from './dep.js'; export default { home() { return JSON.stringify({ value }); } };"
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });

        let manager = SpiderManager::default();
        let call = SpiderCall::parse("homeContent", json!({})).unwrap();
        let result = tauri::async_runtime::block_on(manager.invoke(
            1,
            site(3, &format!("http://{address}/main.js")),
            call,
        ))
        .unwrap();
        assert_eq!(result, json!({"value":"worker-ok"}));
        manager.invalidate_all();
        server.join().unwrap();
    }
}
