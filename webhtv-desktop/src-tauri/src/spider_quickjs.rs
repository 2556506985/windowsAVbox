use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::{
    blocking::Client,
    header::{HeaderName, HeaderValue},
    redirect::Policy,
    Url,
};
use rquickjs::{
    loader::{ImportAttributes, Loader, Resolver},
    CatchResultExt, Context, Ctx, Error, Function, Module, Promise, Runtime,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    config::Site,
    spider::{RawOutput, SpiderCall},
};

const MAX_MODULE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_HTTP_BYTES: u64 = 16 * 1024 * 1024;
const QUICKJS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const QUICKJS_STACK_LIMIT: usize = 512 * 1024;

const HOST_PRELUDE: &str = r#"
globalThis.console = globalThis.console || {
  log() {}, info() {}, warn() {}, error() {}, debug() {}
};

for (const name of ['global', 'window', 'self']) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  if (!descriptor || descriptor.configurable) {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      enumerable: true,
      get() { return globalThis; },
      set() {}
    });
  }
}

globalThis._http = function (url, options = {}) {
  const result = JSON.parse(__webhtvHttp(String(url), JSON.stringify(options || {})));
  if (typeof options.complete === 'function') {
    options.complete(result);
    return null;
  }
  return result;
};

globalThis.http = function (url, options = {}) {
  if (options && options.async === false) return globalThis._http(url, options);
  return new Promise((resolve) => {
    globalThis._http(url, Object.assign({}, options, { complete: resolve }));
  });
};

globalThis.req = function (url, options) {
  return globalThis.http(url, Object.assign({ async: false }, options));
};

globalThis.joinUrl = function (parent, child) {
  return __webhtvJoinUrl(String(parent), String(child));
};

const __webhtvLocalValues = new Map();
globalThis.local = {
  get(rule, key) {
    return __webhtvLocalValues.get(`${rule || ''}\u0000${key || ''}`) || '';
  },
  set(rule, key, value) {
    __webhtvLocalValues.set(`${rule || ''}\u0000${key || ''}`, String(value || ''));
  },
  delete(rule, key) {
    __webhtvLocalValues.delete(`${rule || ''}\u0000${key || ''}`);
  }
};
"#;

const BOOTSTRAP_MODULE: &str = r#"
import * as spider from __WEBHTV_SPECIFIER__;

if (!globalThis.__JS_SPIDER__) {
  if (typeof spider.__jsEvalReturn === 'function') {
    globalThis.req = globalThis.http;
    globalThis.__JS_SPIDER__ = spider.__jsEvalReturn();
  } else if ('default' in spider) {
    globalThis.__JS_SPIDER__ = typeof spider.default === 'function'
      ? spider.default()
      : spider.default;
  }
}
"#;

const DISPATCH_PRELUDE: &str = r#"
globalThis.__WEBHTV_CALL__ = async function (name, args) {
  const target = globalThis.__JS_SPIDER__;
  if (!target || (typeof target !== 'object' && typeof target !== 'function')) {
    throw new Error('Spider module did not provide an object');
  }
  const fn = target[name];
  if (typeof fn !== 'function') return JSON.stringify({ kind: 'missing' });
  const value = await fn.apply(target, args);
  if (value === undefined) return JSON.stringify({ kind: 'undefined' });
  return JSON.stringify({ kind: 'value', value });
};
"#;

pub(super) struct QuickJsSpec {
    site_key: String,
    api: String,
    ext: Value,
    timeout: Duration,
    sources: HashMap<String, String>,
}

#[derive(Clone)]
pub(super) struct QuickJsHandle {
    sender: SyncSender<WorkerCommand>,
}

enum WorkerCommand {
    Invoke {
        call: SpiderCall,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    },
    Shutdown,
}

struct QuickJsEngine {
    context: Context,
    _runtime: Runtime,
    deadline: Arc<Mutex<Option<Instant>>>,
    timeout: Duration,
}

struct SpiderResolver;

struct SpiderLoader {
    client: Client,
    sources: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpOptions {
    method: Option<String>,
    headers: BTreeMap<String, Value>,
    data: Option<Value>,
    body: Option<String>,
    post_type: Option<String>,
    timeout: Option<u64>,
    redirect: Option<i32>,
    buffer: Option<i32>,
}

impl QuickJsSpec {
    pub(super) fn from_site(site: &Site) -> Self {
        let seconds = if site.timeout <= 0 {
            15
        } else {
            site.timeout.clamp(1, 30)
        };
        Self {
            site_key: site.key.clone(),
            api: site.api.clone(),
            ext: site.ext.clone(),
            timeout: Duration::from_secs(seconds as u64),
            sources: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn for_test(
        site_key: &str,
        api: &str,
        ext: Value,
        timeout: Duration,
        sources: HashMap<String, String>,
    ) -> Self {
        Self {
            site_key: site_key.to_string(),
            api: api.to_string(),
            ext,
            timeout,
            sources,
        }
    }
}

impl QuickJsHandle {
    pub(super) fn spawn(spec: QuickJsSpec) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(16);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("webhtv-quickjs".to_string())
            .spawn(move || match QuickJsEngine::new(spec) {
                Ok(engine) => {
                    if ready_sender.send(Ok(())).is_ok() {
                        run_worker(engine, receiver);
                    }
                }
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|error| format!("unable to start QuickJS worker: {error}"))?;
        ready_receiver
            .recv()
            .map_err(|_| "QuickJS worker stopped during initialization".to_string())??;
        Ok(Self { sender })
    }

    pub(super) fn invoke(&self, call: SpiderCall) -> Result<RawOutput, String> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .send(WorkerCommand::Invoke { call, reply })
            .map_err(|_| "QuickJS worker is no longer available".to_string())?;
        receiver
            .recv()
            .map_err(|_| "QuickJS worker stopped before replying".to_string())?
    }

    pub(super) fn shutdown(&self) {
        let _ = self.sender.try_send(WorkerCommand::Shutdown);
    }
}

impl QuickJsEngine {
    fn new(mut spec: QuickJsSpec) -> Result<Self, String> {
        if spec.api.trim().is_empty() {
            return Err("QuickJS Spider API cannot be empty".to_string());
        }
        let client = module_client()?;
        let main_source = match spec.sources.get(&spec.api) {
            Some(source) => source.clone(),
            None => fetch_module(&client, &spec.api, MAX_MODULE_BYTES)?,
        };
        let cat_mode = main_source.contains("__jsEvalReturn");
        spec.sources.insert(spec.api.clone(), main_source);

        let runtime =
            Runtime::new().map_err(|error| format!("unable to create QuickJS: {error}"))?;
        runtime.set_memory_limit(QUICKJS_MEMORY_LIMIT);
        runtime.set_max_stack_size(QUICKJS_STACK_LIMIT);
        let deadline = Arc::new(Mutex::new(None::<Instant>));
        let interrupt_deadline = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupt_deadline
                .lock()
                .ok()
                .and_then(|deadline| *deadline)
                .is_some_and(|deadline| Instant::now() >= deadline)
        })));
        runtime.set_loader(
            SpiderResolver,
            SpiderLoader {
                client,
                sources: spec.sources.clone(),
            },
        );
        let context = Context::full(&runtime)
            .map_err(|error| format!("unable to create QuickJS context: {error}"))?;
        let engine = Self {
            context,
            _runtime: runtime,
            deadline,
            timeout: spec.timeout,
        };
        engine.initialize(&spec, cat_mode)?;
        Ok(engine)
    }

    fn initialize(&self, spec: &QuickJsSpec, cat_mode: bool) -> Result<(), String> {
        let specifier = serde_json::to_string(&spec.api)
            .map_err(|error| format!("unable to encode Spider module name: {error}"))?;
        let bootstrap = BOOTSTRAP_MODULE.replace("__WEBHTV_SPECIFIER__", &specifier);
        let ext = initialization_ext(&spec.site_key, &spec.ext, cat_mode);
        self.with_deadline(|ctx| {
            install_host_functions(&ctx)?;
            js_result(&ctx, ctx.eval::<(), _>(HOST_PRELUDE))?;
            let promise = js_result(
                &ctx,
                Module::evaluate(ctx.clone(), "webhtv:bootstrap", bootstrap),
            )?;
            js_result(&ctx, promise.finish::<()>())?;
            js_result(&ctx, ctx.eval::<(), _>(DISPATCH_PRELUDE))?;
            dispatch(&ctx, "init", &[ext]).map(|_| ())
        })
    }

    fn invoke(&self, call: &SpiderCall) -> Result<RawOutput, String> {
        self.with_deadline(|ctx| dispatch(&ctx, call.js_method(), call.arguments()))
    }

    fn destroy(&self) {
        let _ = self.with_deadline(|ctx| dispatch(&ctx, "destroy", &[]).map(|_| ()));
    }

    fn with_deadline<T, F>(&self, operation: F) -> Result<T, String>
    where
        F: for<'js> FnOnce(Ctx<'js>) -> Result<T, String>,
    {
        let deadline = Instant::now() + self.timeout;
        *self
            .deadline
            .lock()
            .map_err(|_| "QuickJS deadline state is unavailable".to_string())? = Some(deadline);
        let result = self.context.with(operation);
        let expired = Instant::now() >= deadline;
        if let Ok(mut current) = self.deadline.lock() {
            *current = None;
        }
        if result.is_err() && expired {
            Err("QuickJS execution timed out".to_string())
        } else {
            result
        }
    }
}

impl Resolver for SpiderResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if Url::parse(name).is_ok() {
            return Ok(name.to_string());
        }
        let base_url = Url::parse(base).map_err(|_| {
            Error::new_resolving_message(base, name, "base module is not an absolute URL")
        })?;
        base_url
            .join(name)
            .map(|url| url.to_string())
            .map_err(|_| Error::new_resolving_message(base, name, "invalid relative module URL"))
    }
}

impl Loader for SpiderLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let source = match self.sources.get(name) {
            Some(source) => source.clone(),
            None => {
                let source = fetch_module(&self.client, name, MAX_MODULE_BYTES)
                    .map_err(|error| Error::new_loading_message(name, error))?;
                self.sources.insert(name.to_string(), source.clone());
                source
            }
        };
        Module::declare(
            ctx.clone(),
            name,
            source.replace("__JS_SPIDER__", "globalThis.__JS_SPIDER__"),
        )
    }
}

fn run_worker(engine: QuickJsEngine, receiver: mpsc::Receiver<WorkerCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Invoke { call, reply } => {
                let _ = reply.send(engine.invoke(&call));
            }
            WorkerCommand::Shutdown => break,
        }
    }
    engine.destroy();
}

fn dispatch(ctx: &Ctx<'_>, method: &str, arguments: &[Value]) -> Result<RawOutput, String> {
    let function: Function = js_result(ctx, ctx.globals().get("__WEBHTV_CALL__"))?;
    let arguments = serde_json::to_string(arguments)
        .map_err(|error| format!("unable to encode Spider arguments: {error}"))?;
    let arguments = js_result(ctx, ctx.json_parse(arguments))?;
    let promise: Promise = js_result(ctx, function.call((method, arguments)))?;
    let envelope: String = js_result(ctx, promise.finish())?;
    serde_json::from_str(&envelope)
        .map_err(|error| format!("QuickJS returned an invalid result envelope: {error}"))
}

fn js_result<'js, T>(ctx: &Ctx<'js>, result: rquickjs::Result<T>) -> Result<T, String> {
    result.catch(ctx).map_err(|error| error.to_string())
}

fn initialization_ext(site_key: &str, ext: &Value, cat_mode: bool) -> Value {
    let ext = match ext {
        Value::Null => Value::String(String::new()),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::String(text.clone())),
        Value::Object(_) => ext.clone(),
        Value::Array(_) | Value::Bool(_) | Value::Number(_) => Value::String(ext.to_string()),
    };
    if cat_mode {
        json!({"stype": 3, "skey": site_key, "ext": ext})
    } else {
        ext
    }
}

fn install_host_functions(ctx: &Ctx<'_>) -> Result<(), String> {
    let globals = ctx.globals();
    let http = js_result(ctx, Function::new(ctx.clone(), host_http))?;
    let join_url = js_result(ctx, Function::new(ctx.clone(), host_join_url))?;
    js_result(ctx, globals.set("__webhtvHttp", http))?;
    js_result(ctx, globals.set("__webhtvJoinUrl", join_url))?;
    Ok(())
}

fn module_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("WebHomeTV-Desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .redirect(Policy::limited(10))
        .build()
        .map_err(|error| format!("unable to initialize Spider HTTP client: {error}"))
}

fn fetch_module(client: &Client, name: &str, limit: u64) -> Result<String, String> {
    let url = Url::parse(name).map_err(|_| "module URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only HTTP(S) Spider modules are supported".to_string());
    }
    let response = client
        .get(url)
        .send()
        .map_err(|_| "module request failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "module request returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = read_limited(response, limit)?;
    String::from_utf8(bytes).map_err(|_| "Spider module is not valid UTF-8".to_string())
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("unable to read HTTP response: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("HTTP response exceeds {limit} bytes"));
    }
    Ok(bytes)
}

fn host_http(url: String, options: String) -> String {
    let result = execute_http(&url, &options).unwrap_or_else(|_| http_error());
    serde_json::to_string(&result)
        .unwrap_or_else(|_| r#"{"code":"","headers":{},"content":""}"#.to_string())
}

fn execute_http(url: &str, options: &str) -> Result<Value, String> {
    let url = Url::parse(url).map_err(|_| "invalid request URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only HTTP(S) requests are supported".to_string());
    }
    let options: HttpOptions = serde_json::from_str(options).unwrap_or_default();
    let timeout = Duration::from_millis(options.timeout.unwrap_or(10_000).clamp(100, 30_000));
    let redirect = if options.redirect.unwrap_or(1) == 1 {
        Policy::limited(10)
    } else {
        Policy::none()
    };
    let client = Client::builder()
        .user_agent(concat!("WebHomeTV-Desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .redirect(redirect)
        .build()
        .map_err(|error| format!("unable to initialize HTTP request: {error}"))?;

    let method = options
        .method
        .as_deref()
        .unwrap_or("get")
        .to_ascii_lowercase();
    let mut request = match method.as_str() {
        "post" => client.post(url),
        "header" | "head" => client.head(url),
        _ => client.get(url),
    };
    for (name, value) in &options.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "request contains an invalid header name".to_string())?;
        let value = header_value(value)
            .ok_or_else(|| "request contains an invalid header value".to_string())?;
        let value = HeaderValue::from_str(&value)
            .map_err(|_| "request contains an invalid header value".to_string())?;
        request = request.header(name, value);
    }
    if method == "post" {
        let post_type = options.post_type.as_deref().unwrap_or("json");
        if let Some(body) = options.body {
            request = request.body(body);
        } else if let Some(data) = options.data {
            match post_type {
                "form" => {
                    request = request
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(encode_form(&data)?);
                }
                "form-data" => {
                    return Err("multipart Spider requests are not implemented".to_string());
                }
                _ => {
                    request = request
                        .header("content-type", "application/json; charset=utf-8")
                        .body(data.to_string());
                }
            }
        }
    }

    let response = request
        .send()
        .map_err(|_| "Spider HTTP request failed".to_string())?;
    let code = response.status().as_u16();
    let headers = response_headers(response.headers());
    let bytes = read_limited(response, MAX_HTTP_BYTES)?;
    let content = match options.buffer.unwrap_or(0) {
        1 | 3 => json!(bytes),
        2 => Value::String(BASE64.encode(bytes)),
        _ => Value::String(String::from_utf8_lossy(&bytes).into_owned()),
    };
    Ok(json!({"code": code, "headers": headers, "content": content}))
}

fn header_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Array(values) => values.first().and_then(header_value),
        Value::Null | Value::Object(_) => None,
    }
}

fn response_headers(headers: &reqwest::header::HeaderMap) -> Value {
    let mut result = Map::new();
    for name in headers.keys() {
        let values = headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect::<Vec<_>>();
        let value = match values.as_slice() {
            [] => continue,
            [value] => Value::String(value.clone()),
            _ => Value::Array(values.into_iter().map(Value::String).collect()),
        };
        result.insert(name.as_str().to_string(), value);
    }
    Value::Object(result)
}

fn encode_form(value: &Value) -> Result<String, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "form request data must be an object".to_string())?;
    let mut url = Url::parse("http://localhost/").expect("static URL must parse");
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in object {
            let value = header_value(value)
                .ok_or_else(|| "form request values must be scalar".to_string())?;
            pairs.append_pair(key, &value);
        }
    }
    Ok(url.query().unwrap_or_default().to_string())
}

fn http_error() -> Value {
    json!({"code": "", "headers": {}, "content": ""})
}

fn host_join_url(parent: String, child: String) -> String {
    Url::parse(&parent)
        .and_then(|url| url.join(&child))
        .map(|url| url.to_string())
        .unwrap_or(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(engine: &QuickJsEngine, method: &str, args: Value) -> Value {
        let call = SpiderCall::parse(method, args).unwrap();
        call.normalize(engine.invoke(&call).unwrap()).unwrap()
    }

    #[test]
    fn default_factory_supports_relative_modules_state_and_promises() {
        let api = "memory://fixture/main.js";
        let mut sources = HashMap::new();
        sources.insert(
            api.to_string(),
            include_str!("../tests/fixtures/quickjs-spider.js").to_string(),
        );
        sources.insert(
            "memory://fixture/quickjs-spider-dep.js".to_string(),
            include_str!("../tests/fixtures/quickjs-spider-dep.js").to_string(),
        );
        let engine = QuickJsEngine::new(QuickJsSpec::for_test(
            "fixture",
            api,
            json!({"token":"ok"}),
            Duration::from_secs(2),
            sources,
        ))
        .unwrap();

        assert_eq!(
            call(&engine, "homeContent", json!({"filter":true})),
            json!({
                "class":[{"type_id":"fixture","type_name":"QuickJS Fixture"}],
                "meta":{
                    "calls":1,
                    "ext":{"token":"ok"},
                    "filter":true,
                    "moduleName":"relative-module"
                }
            })
        );
        assert_eq!(
            call(&engine, "manualVideoCheck", json!({})),
            Value::Bool(true)
        );
    }

    #[test]
    fn catvod_export_receives_wrapped_extension() {
        let api = "memory://fixture/cat.js";
        let mut sources = HashMap::new();
        sources.insert(
            api.to_string(),
            r#"
                export function __jsEvalReturn() {
                  let received;
                  return {
                    init(ext) { received = ext; },
                    home() { return JSON.stringify(received); }
                  };
                }
            "#
            .to_string(),
        );
        let engine = QuickJsEngine::new(QuickJsSpec::for_test(
            "cat-site",
            api,
            json!("plain"),
            Duration::from_secs(2),
            sources,
        ))
        .unwrap();

        assert_eq!(
            call(&engine, "homeContent", json!({})),
            json!({"stype":3,"skey":"cat-site","ext":"plain"})
        );
    }

    #[test]
    fn non_object_extensions_match_android_string_conversion() {
        assert_eq!(
            initialization_ext("site", &json!(["a", "b"]), false),
            json!("[\"a\",\"b\"]")
        );
        assert_eq!(
            initialization_ext("site", &json!(true), false),
            json!("true")
        );
        assert_eq!(
            initialization_ext("site", &json!("{\"token\":1}"), false),
            json!({"token":1})
        );
    }

    #[test]
    fn interrupt_handler_stops_infinite_scripts() {
        let api = "memory://fixture/timeout.js";
        let mut sources = HashMap::new();
        sources.insert(
            api.to_string(),
            "export default { home() { while (true) {} } };".to_string(),
        );
        let engine = QuickJsEngine::new(QuickJsSpec::for_test(
            "timeout",
            api,
            Value::Null,
            Duration::from_millis(50),
            sources,
        ))
        .unwrap();
        let call = SpiderCall::parse("homeContent", json!({})).unwrap();
        assert!(engine.invoke(&call).unwrap_err().contains("timed out"));
    }
}
