use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::Site,
    spider::{RawOutput, SpiderCall},
};

#[derive(Clone)]
pub(super) struct JavaHandle {
    sender: SyncSender<WorkerCommand>,
    child: Arc<Mutex<Child>>,
}

pub(super) struct JavaSpec {
    work_dir: PathBuf,
    sidecar_jar: PathBuf,
    java_bin: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct JavaParserRequest {
    pub(crate) parser_type: i32,
    pub(crate) parser_key: String,
    pub(crate) parser_name: String,
    pub(crate) flag: String,
    pub(crate) url: String,
    pub(crate) parsers: Value,
}

enum WorkerCommand {
    Invoke {
        site: Site,
        call: SpiderCall,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    },
    Parse {
        site: Site,
        request: JavaParserRequest,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    },
    Shutdown,
}

struct JavaEngine {
    child: Arc<Mutex<Child>>,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<Result<RawOutput, String>>>>>,
    next_id: AtomicU64,
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const MAX_INVOCATION_TIMEOUT_SECONDS: i32 = 120;

const AUTH_INVOCATION_TIMEOUT_SECONDS: u64 = 45;

fn is_auth_method(method: &str) -> bool {
    matches!(
        method,
        "authStart" | "authPoll" | "authCancel" | "authClear" | "authStatus"
    )
}

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    id: Option<String>,
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
}

impl JavaSpec {
    pub(super) fn from_site(site: &Site, work_dir: PathBuf) -> Result<Self, String> {
        if site.jar.trim().is_empty() {
            return Err(format!(
                "Java Spider site `{}` is missing a jar path",
                site.key
            ));
        }
        if !site.api.starts_with("csp_") {
            return Err(format!(
                "Java Spider site `{}` api must start with csp_",
                site.key
            ));
        }
        Ok(Self {
            work_dir,
            sidecar_jar: locate_sidecar_jar()?,
            java_bin: locate_java()?,
        })
    }
}

impl JavaHandle {
    pub(super) fn spawn(spec: JavaSpec) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(16);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("webhtv-java".to_string())
            .spawn(move || match JavaEngine::new(spec) {
                Ok(engine) => {
                    let child = engine.child.clone();
                    if ready_sender.send(Ok(child)).is_ok() {
                        run_worker(engine, receiver);
                    }
                }
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|error| format!("unable to start Java worker: {error}"))?;
        ready_receiver
            .recv()
            .map_err(|_| "Java worker stopped during initialization".to_string())?
            .map(|child| Self { sender, child })
    }

    pub(super) fn invoke(&self, site: Site, call: SpiderCall) -> Result<RawOutput, String> {
        let method = call.method().to_string();
        let auth = is_auth_method(&method);
        let timeout = if auth {
            Duration::from_secs(AUTH_INVOCATION_TIMEOUT_SECONDS)
        } else {
            invocation_timeout(&site)
        };
        let (reply, receiver) = mpsc::channel();
        self.sender
            .send(WorkerCommand::Invoke { site, call, reply })
            .map_err(|_| "Java worker is no longer available".to_string())?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Disconnected) => {
                Err("Java worker stopped before replying".to_string())
            }
            Err(RecvTimeoutError::Timeout) => {
                if !auth {
                    self.terminate();
                }
                Err(format!(
                    "Java Spider method `{method}` timed out after {} seconds",
                    timeout.as_secs()
                ))
            }
        }
    }

    pub(super) fn parse(
        &self,
        site: Site,
        request: JavaParserRequest,
    ) -> Result<RawOutput, String> {
        let timeout = invocation_timeout(&site);
        let (reply, receiver) = mpsc::channel();
        self.sender
            .send(WorkerCommand::Parse {
                site,
                request,
                reply,
            })
            .map_err(|_| "Java worker is no longer available".to_string())?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Disconnected) => {
                Err("Java worker stopped before replying".to_string())
            }
            Err(RecvTimeoutError::Timeout) => {
                self.terminate();
                Err(format!(
                    "Java Spider parser timed out after {} seconds",
                    timeout.as_secs()
                ))
            }
        }
    }

    pub(super) fn shutdown(&self) {
        let _ = self.sender.try_send(WorkerCommand::Shutdown);
        self.terminate();
    }

    fn terminate(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl JavaEngine {
    fn new(spec: JavaSpec) -> Result<Self, String> {
        std::fs::create_dir_all(&spec.work_dir)
            .map_err(|error| format!("unable to create Java work dir: {error}"))?;
        let mut command = hidden_command(&spec.java_bin);
        command
            .arg("-Xverify:none")
            .arg("-Dfile.encoding=UTF-8")
            .arg("-Dstdout.encoding=UTF-8")
            .arg("-Dstderr.encoding=UTF-8")
            .arg(format!("-Dwebhtv.java.workDir={}", spec.work_dir.display()));
        if let Ok(port) = std::env::var("WEBHTV_JAVA_PROXY_PORT") {
            command.arg(format!("-Dwebhtv.proxy.port={port}"));
        }
        command
            .arg("-jar")
            .arg(&spec.sidecar_jar)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("JAVA_TOOL_OPTIONS", "-Dfile.encoding=UTF-8");
        let mut child = command
            .spawn()
            .map_err(|error| {
                format!(
                    "unable to start Java sidecar with `{}`: {error}",
                    spec.java_bin.display()
                )
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Java sidecar stdin is unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Java sidecar stdout is unavailable".to_string())?;
        if let Some(stderr) = child.stderr.take() {
            thread::Builder::new()
                .name("webhtv-java-stderr".to_string())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        eprintln!("[java-sidecar] {line}");
                    }
                })
                .ok();
        }
        let child = Arc::new(Mutex::new(child));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let engine = Self {
            child: child.clone(),
            stdin: Mutex::new(stdin),
            pending: pending.clone(),
            next_id: AtomicU64::new(1),
        };
        thread::Builder::new()
            .name("webhtv-java-pump".to_string())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(response) = serde_json::from_str::<SidecarResponse>(trimmed) else {
                        continue;
                    };
                    let Some(id) = response.id.as_deref() else {
                        continue;
                    };
                    let reply = pending
                        .lock()
                        .ok()
                        .and_then(|mut registry| registry.remove(id));
                    let Some(reply) = reply else {
                        continue;
                    };
                    let result = if response.ok {
                        match response.data {
                            None | Some(Value::Null) => Ok(RawOutput::Undefined),
                            Some(value) => serde_json::from_value(value).map_err(|error| {
                                format!("Java sidecar returned an invalid result envelope: {error}")
                            }),
                        }
                    } else {
                        Err(response
                            .error
                            .unwrap_or_else(|| "Java sidecar invocation failed".to_string()))
                    };
                    let _ = reply.send(result);
                }
                let waiters: Vec<_> = pending
                    .lock()
                    .map(|mut registry| registry.drain().map(|(_, sender)| sender).collect())
                    .unwrap_or_default();
                for sender in waiters {
                    let _ = sender.send(Err("Java sidecar stopped".to_string()));
                }
            })
            .ok();
        engine.ping()?;
        Ok(engine)
    }

    fn ping(&self) -> Result<(), String> {
        self.request(json!({
            "type": "ping"
        }))
    }

    fn dispatch(
        &self,
        mut payload: Value,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    ) -> Result<(), String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        payload["id"] = Value::String(id.clone());
        let mut line = serde_json::to_string(&payload)
            .map_err(|error| format!("unable to encode Java request: {error}"))?;
        line.push('\n');
        if let Ok(mut registry) = self.pending.lock() {
            registry.insert(id.clone(), reply.clone());
        } else {
            let _ = reply.send(Err("Java sidecar pending registry is unavailable".to_string()));
            return Err("Java sidecar pending registry is unavailable".to_string());
        }
        let written = {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| "Java sidecar stdin is unavailable".to_string())?;
            stdin
                .write_all(line.as_bytes())
                .map_err(|error| format!("unable to write Java request: {error}"))?;
            stdin
                .flush()
                .map_err(|error| format!("unable to flush Java request: {error}"))
        };
        if let Err(error) = written {
            if let Ok(mut registry) = self.pending.lock() {
                registry.remove(&id);
            }
            let _ = reply.send(Err(error.clone()));
            return Err(error);
        }
        Ok(())
    }

    fn request(&self, payload: Value) -> Result<(), String> {
        let (reply, receiver) = mpsc::channel();
        self.dispatch(payload, reply)?;
        receiver
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| "Java sidecar did not respond in time".to_string())?
            .map(|_| ())
    }

    fn invoke(
        &self,
        site: &Site,
        call: &SpiderCall,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    ) -> Result<(), String> {
        let request_type = match call.method() {
            "configSet" | "configGet" => "config",
            "authStart" | "authPoll" | "authCancel" | "authClear" | "authStatus" => "auth",
            _ => "invoke",
        };
        let operation = match call.method() {
            "configGet" => "get",
            "configSet" => "set",
            "authStart" => "start",
            "authPoll" => "poll",
            "authCancel" => "cancel",
            "authClear" => "clear",
            "authStatus" => "status",
            _ => "",
        };
        let auth_value = call
            .arguments()
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.dispatch(
            json!({
                "type": request_type,
                "operation": operation,
                "siteKey": site.key,
                "api": site.api,
                "ext": site.ext,
                "jar": site.jar,
                "method": call.method(),
                "args": java_arguments(call),
                "provider": if matches!(call.method(), "authStart" | "authClear" | "authStatus") { auth_value } else { "" },
                "sessionId": if matches!(call.method(), "authPoll" | "authCancel") { auth_value } else { "" },
            }),
            reply,
        )
    }

    fn parse(
        &self,
        site: &Site,
        request: &JavaParserRequest,
        reply: mpsc::Sender<Result<RawOutput, String>>,
    ) -> Result<(), String> {
        self.dispatch(
            json!({
                "type": "parse",
                "siteKey": site.key,
                "api": site.api,
                "ext": site.ext,
                "jar": site.jar,
                "parserType": request.parser_type,
                "parserKey": request.parser_key,
                "parserName": request.parser_name,
                "flag": request.flag,
                "url": request.url,
                "parsers": request.parsers,
            }),
            reply,
        )
    }

    fn shutdown(&self) {
        let _ = self.request(json!({
            "type": "shutdown"
        }));
        self.stop_child();
    }

    fn stop_child(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for JavaEngine {
    fn drop(&mut self) {
        self.stop_child();
    }
}

fn run_worker(engine: JavaEngine, receiver: mpsc::Receiver<WorkerCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            WorkerCommand::Invoke { site, call, reply } => {
                let _ = engine.invoke(&site, &call, reply);
            }
            WorkerCommand::Parse {
                site,
                request,
                reply,
            } => {
                let _ = engine.parse(&site, &request, reply);
            }
            WorkerCommand::Shutdown => break,
        }
    }
    engine.shutdown();
}

fn java_arguments(call: &SpiderCall) -> Value {
    match call.method() {
        "detailContent" => {
            let id = call
                .arguments()
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!([[id]])
        }
        "playerContent" => {
            let flag = call
                .arguments()
                .first()
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let id = call
                .arguments()
                .get(1)
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let flags = call
                .arguments()
                .get(2)
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            json!([flag, id, flags])
        }
        _ => Value::Array(call.arguments().to_vec()),
    }
}

fn invocation_timeout(site: &Site) -> Duration {
    Duration::from_secs(site.timeout.clamp(1, MAX_INVOCATION_TIMEOUT_SECONDS) as u64)
}

fn locate_java() -> Result<PathBuf, String> {
    if let Some(path) = bundled_java().filter(|path| java_is_supported(path)) {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("WEBHTV_JAVA") {
        let candidate = PathBuf::from(path);
        if java_is_supported(&candidate) {
            return Ok(candidate);
        }
    }
    if let Ok(home) = std::env::var("JAVA_HOME") {
        for executable in java_exe_names() {
            let candidate = Path::new(&home).join("bin").join(executable);
            if java_is_supported(&candidate) {
                return Ok(candidate);
            }
        }
    }
    which_java().ok_or_else(|| {
        "The bundled Java 17 runtime is missing or damaged. Reinstall WebHomeTV Desktop."
            .to_string()
    })
}

fn bundled_java() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.extend(
                java_exe_names()
                    .iter()
                    .map(|name| directory.join("resources/java-runtime/bin").join(name)),
            );
        }
    }
    candidates.extend(
        java_exe_names()
            .iter()
            .map(|name| manifest.join("resources/java-runtime/bin").join(name)),
    );
    candidates.into_iter().find(|path| path.is_file())
}

fn which_java() -> Option<PathBuf> {
    let command = if cfg!(windows) { "where" } else { "which" };
    let output = hidden_command(Path::new(command))
        .arg("java")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| java_is_supported(path))
}

fn java_exe_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["javaw.exe", "java.exe"]
    } else {
        &["java"]
    }
}

fn java_is_supported(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Ok(output) = hidden_command(path).arg("-version").output() else {
        return false;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    java_major(&text).is_some_and(|major| major >= 17)
}

fn hidden_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn java_major(version_output: &str) -> Option<u32> {
    let marker = "version \"";
    let start = version_output.find(marker)? + marker.len();
    let version = version_output[start..].split('"').next()?;
    let mut parts = version.split(['.', '-', '+']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn locate_sidecar_jar() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("WEBHTV_JAVA_SIDECAR") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.extend([
                directory.join("java-spider-sidecar-0.1.0.jar"),
                directory.join("resources/java-spider-sidecar-0.1.0.jar"),
            ]);
        }
    }
    candidates.extend([
        manifest.join("resources/java-spider-sidecar-0.1.0.jar"),
        manifest.join("../java-sidecar/target/java-spider-sidecar-0.1.0.jar"),
        manifest.join("java-spider-sidecar-0.1.0.jar"),
    ]);
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Java sidecar jar not found. Build java-sidecar or set WEBHTV_JAVA_SIDECAR.".to_string()
        })
}

pub(super) fn java_work_dir(base: &Path, config_id: i64, _site_key: &str) -> PathBuf {
    base.join("java-spiders")
        .join(config_id.to_string())
        .join("shared")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detail_arguments_are_wrapped_for_java_lists() {
        let call = SpiderCall::parse("detailContent", json!({"ids":["vod-1"]})).unwrap();
        assert_eq!(java_arguments(&call), json!([["vod-1"]]));
    }

    #[test]
    fn home_arguments_pass_through() {
        let call = SpiderCall::parse("homeContent", json!({"filter":true})).unwrap();
        assert_eq!(java_arguments(&call), json!([true]));
    }

    #[test]
    fn java_version_parser_handles_legacy_and_modern_versions() {
        assert_eq!(java_major("openjdk version \"1.8.0_472\""), Some(8));
        assert_eq!(java_major("openjdk version \"17.0.12\""), Some(17));
        assert_eq!(java_major("openjdk version \"25.0.2\""), Some(25));
    }

    #[test]
    fn java_invocation_timeout_honors_site_value_and_cap() {
        let default_site: Site = serde_json::from_value(json!({})).unwrap();
        let short_site: Site = serde_json::from_value(json!({"timeout": 4})).unwrap();
        let long_site: Site = serde_json::from_value(json!({"timeout": 60})).unwrap();

        assert_eq!(invocation_timeout(&default_site), Duration::from_secs(15));
        assert_eq!(invocation_timeout(&short_site), Duration::from_secs(4));
        assert_eq!(invocation_timeout(&long_site), Duration::from_secs(60));
    }

    #[test]
    fn auth_methods_are_recognized_for_longer_invocations() {
        for method in [
            "authStart",
            "authPoll",
            "authCancel",
            "authClear",
            "authStatus",
        ] {
            assert!(is_auth_method(method), "{method} should be an auth method");
        }
        for method in ["homeContent", "categoryContent", "playerContent", "configSet"] {
            assert!(!is_auth_method(method), "{method} should not be an auth method");
        }
    }

    #[test]
    fn java_sidecar_loads_local_dex_and_returns_home_content() {
        std::env::set_var("WEBHTV_JAVA_PROXY_PORT", "0");
        let jar = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../柒豪/jar/柒豪.jar");
        if !jar.is_file() || locate_java().is_err() || locate_sidecar_jar().is_err() {
            return;
        }
        let work_dir = std::env::temp_dir().join("webhtv-java-test-home");
        let site: Site = serde_json::from_value(json!({
            "key": "db",
            "name": "Douban",
            "type": 3,
            "api": "csp_Douban",
            "jar": format!("file:///{}", jar.display().to_string().replace('\\', "/")),
            "timeout": 30
        }))
        .unwrap();
        let manager = crate::spider::SpiderManager::with_work_dir(work_dir);
        let call = SpiderCall::parse("homeContent", json!({"filter": false})).unwrap();
        let result = tauri::async_runtime::block_on(manager.invoke(9, site, call)).unwrap();
        assert!(result.get("class").and_then(Value::as_array).is_some());
        assert!(result.get("list").and_then(Value::as_array).is_some());
        manager.invalidate_all();
    }

    #[test]
    fn timed_out_java_detail_is_terminated_and_runtime_recovers() {
        std::env::set_var("WEBHTV_JAVA_PROXY_PORT", "0");
        let jar = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../柒豪/jar/柒豪.jar");
        if !jar.is_file() || locate_java().is_err() || locate_sidecar_jar().is_err() {
            return;
        }
        let jar = format!("file:///{}", jar.display().to_string().replace('\\', "/"));
        let work_dir = std::env::temp_dir().join("webhtv-java-test-timeout");
        let stalled: Site = serde_json::from_value(json!({
            "key": "stalled",
            "name": "Stalled",
            "type": 3,
            "api": "csp_PanWebShare",
            "ext": {"site": ["http://192.0.2.1"]},
            "jar": jar,
            "timeout": 1
        }))
        .unwrap();
        let manager = crate::spider::SpiderManager::with_work_dir(work_dir);
        let detail = SpiderCall::parse("detailContent", json!({"ids":["vod-1"]})).unwrap();
        let error =
            tauri::async_runtime::block_on(manager.invoke(10, stalled, detail)).unwrap_err();
        assert!(error.contains("timed out after 1 seconds"));

        let healthy: Site = serde_json::from_value(json!({
            "key": "db",
            "name": "Douban",
            "type": 3,
            "api": "csp_Douban",
            "jar": jar,
            "timeout": 30
        }))
        .unwrap();
        let home = SpiderCall::parse("homeContent", json!({"filter": false})).unwrap();
        let result = tauri::async_runtime::block_on(manager.invoke(10, healthy, home)).unwrap();
        assert!(result.get("class").and_then(Value::as_array).is_some());
        manager.invalidate_all();
    }
}
