use std::{
    collections::{BTreeMap, HashMap},
    ffi::{c_char, c_double, c_int, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use libloading::Library;
use percent_encoding::percent_decode_str;
use regex::Regex;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[cfg(windows)]
use windows::{
    core::w,
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_WNDPROC,
        HTTRANSPARENT, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_DESTROY, WM_NCHITTEST,
        WNDPROC, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE,
        WS_CHILD, WS_CLIPSIBLINGS,
    },
};

use crate::{
    config::{Parse, Site},
    spider_java::JavaParserRequest,
    state::SharedState,
};

const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_DOUBLE: c_int = 5;

type MpvHandle = c_void;
type MpvCreate = unsafe extern "C" fn() -> *mut MpvHandle;
type MpvInitialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type MpvSetOptionString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvSetPropertyString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvGetProperty =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type MpvGetPropertyString = unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> *mut c_char;
type MpvCommand = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
type MpvWaitEvent = unsafe extern "C" fn(*mut MpvHandle, c_double) -> *const MpvEvent;
type MpvTerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
type MpvErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type MpvFree = unsafe extern "C" fn(*mut c_void);
type MpvClientApiVersion = unsafe extern "C" fn() -> u64;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_IPC_NAME: &str = "webhtv-mpv-ipc";
const WEB_SNIFF_TIMEOUT: Duration = Duration::from_secs(20);
const WEB_SNIFF_SCRIPT: &str = r#"
(() => {
  if (window.__webhtvSniffInstalled) return;
  window.__webhtvSniffInstalled = true;
  window.__webhtvMedia = [];
  const mediaPattern = /\.(?:mp4|m3u8|m3u|mpd)(?:[?#]|$)/i;
  const remember = (value, contentType = '') => {
    if (typeof value !== 'string' || !value || value.startsWith('blob:') || value.startsWith('data:')) return;
    if (mediaPattern.test(value) || /^(?:video|audio)\//i.test(contentType) || /mpegurl|dash\+xml/i.test(contentType)) {
      if (!window.__webhtvMedia.includes(value)) window.__webhtvMedia.push(value);
    }
  };
  const scan = () => {
    try {
      document.querySelectorAll('video, audio, source').forEach((item) => {
        remember(item.currentSrc || item.src || item.getAttribute('src') || '');
      });
      performance.getEntriesByType('resource').forEach((entry) => remember(entry.name));
    } catch (_) {}
  };
  const originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = (...args) => originalFetch.apply(window, args).then((response) => {
      try { remember(response.url, response.headers.get('content-type') || ''); } catch (_) {}
      return response;
    });
  }
  const originalOpen = XMLHttpRequest.prototype.open;
  const originalSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    this.__webhtvUrl = String(url || '');
    return originalOpen.call(this, method, url, ...rest);
  };
  XMLHttpRequest.prototype.send = function(...args) {
    this.addEventListener('load', () => {
      try { remember(this.responseURL || this.__webhtvUrl || '', this.getResponseHeader('content-type') || ''); } catch (_) {}
    });
    return originalSend.apply(this, args);
  };
  window.open = (url) => {
    if (url) window.location.href = String(url);
    return window;
  };
  scan();
  window.setInterval(scan, 250);
})();
"#;
const WEB_SNIFF_PROBE: &str = r#"
(() => {
  try {
    const values = [...(window.__webhtvMedia || [])];
    const mediaPattern = /\.(?:mp4|m3u8|m3u|mpd)(?:[?#]|$)/i;
    performance.getEntriesByType('resource').forEach((entry) => {
      if (mediaPattern.test(entry.name)) values.push(entry.name);
    });
    document.querySelectorAll('video, audio, source').forEach((item) => values.push(item.currentSrc || item.src || ''));
    return JSON.stringify([...new Set(values)].filter((value) => typeof value === 'string' && value && !value.startsWith('blob:') && !value.startsWith('data:')));
  } catch (error) {
    return JSON.stringify({ error: String(error), values: [] });
  }
})()
"#;
static WEB_SNIFF_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerOpenRequest {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) start: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerControlRequest {
    pub(crate) command: String,
    pub(crate) value: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatus {
    pub ready: bool,
    pub idle: bool,
    pub paused: bool,
    pub external: bool,
    pub title: String,
    pub url: String,
    pub time: f64,
    pub duration: f64,
    pub volume: f64,
    pub speed: f64,
    pub api_version: String,
    pub dll_path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerResolveRequest {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) parse: i32,
    #[serde(default)]
    pub(crate) jx: i32,
    #[serde(default)]
    pub(crate) play_url: String,
    #[serde(default)]
    pub(crate) flag: String,
    #[serde(default)]
    pub(crate) site_key: String,
    #[serde(default)]
    pub(crate) click: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerResolveResponse {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProgressKey {
    pub(crate) site_key: String,
    pub(crate) vod_id: String,
    pub(crate) episode_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProgressUpdate {
    pub(crate) site_key: String,
    pub(crate) vod_id: String,
    pub(crate) episode_url: String,
    pub(crate) position: f64,
    pub(crate) duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProgress {
    pub position: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSurfaceBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default = "default_true")]
    pub visible: bool,
}

#[derive(Clone)]
struct PlayerHandle {
    sender: SyncSender<PlayerCommand>,
}

enum PlayerCommand {
    Open {
        request: PlayerOpenRequest,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    Control {
        request: PlayerControlRequest,
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    Status {
        reply: mpsc::Sender<Result<PlayerStatus, String>>,
    },
    Shutdown,
}

struct MpvApi {
    _library: Library,
    create: MpvCreate,
    initialize: MpvInitialize,
    set_option_string: MpvSetOptionString,
    set_property_string: MpvSetPropertyString,
    get_property: MpvGetProperty,
    get_property_string: MpvGetPropertyString,
    command: MpvCommand,
    wait_event: MpvWaitEvent,
    terminate_destroy: MpvTerminateDestroy,
    error_string: MpvErrorString,
    free: MpvFree,
    client_api_version: MpvClientApiVersion,
}

struct PlayerEngine {
    api: MpvApi,
    handle: *mut MpvHandle,
    dll_path: PathBuf,
}

pub struct PlayerManager {
    handle: Mutex<Option<PlayerHandle>>,
    surface: Mutex<Option<isize>>,
    external: Mutex<Option<u32>>,
    external_ipc: Mutex<Option<String>>,
}

impl Default for PlayerManager {
    fn default() -> Self {
        Self {
            handle: Mutex::new(None),
            surface: Mutex::new(None),
            external: Mutex::new(None),
            external_ipc: Mutex::new(None),
        }
    }
}

impl PlayerManager {
    pub fn open(&self, request: PlayerOpenRequest) -> Result<PlayerStatus, String> {
        let request = unwrap_legacy_pvideo_request(request)?;
        if let Err(error) = probe_proxy_text_error(&request) {
            return Err(error);
        }
        if locate_mpv_exe().is_ok() {
            return self.open_external(request);
        }
        let handle = self.ensure_handle()?;
        let (reply, receiver) = mpsc::channel();
        handle
            .sender
            .send(PlayerCommand::Open { request, reply })
            .map_err(|_| "player worker is no longer available".to_string())?;
        receiver
            .recv()
            .map_err(|_| "player worker stopped before replying".to_string())?
    }

    pub fn control(&self, request: PlayerControlRequest) -> Result<PlayerStatus, String> {
        if self.external_pid().is_some() {
            return Ok(self.external_status());
        }
        let handle = self
            .handle
            .lock()
            .map_err(|_| "player registry is unavailable".to_string())?
            .clone();
        let Some(handle) = handle else {
            return Ok(PlayerStatus::default());
        };
        let (reply, receiver) = mpsc::channel();
        if handle
            .sender
            .send(PlayerCommand::Control { request, reply })
            .is_err()
        {
            self.invalidate_handle();
            return Ok(PlayerStatus::default());
        }
        match receiver.recv() {
            Ok(result) => result,
            Err(_) => {
                self.invalidate_handle();
                Ok(PlayerStatus::default())
            }
        }
    }

    pub fn status(&self) -> Result<PlayerStatus, String> {
        if self.external_pid().is_some() {
            return Ok(self.external_status());
        }
        let handle = self
            .handle
            .lock()
            .map_err(|_| "player registry is unavailable".to_string())?
            .clone();
        let Some(handle) = handle else {
            return Ok(PlayerStatus::default());
        };
        let (reply, receiver) = mpsc::channel();
        if handle.sender.send(PlayerCommand::Status { reply }).is_err() {
            self.invalidate_handle();
            return Ok(PlayerStatus::default());
        }
        match receiver.recv() {
            Ok(result) => result,
            Err(_) => {
                self.invalidate_handle();
                Ok(PlayerStatus::default())
            }
        }
    }

    pub fn close(&self) {
        self.close_external();
        if let Ok(mut current) = self.handle.lock() {
            if let Some(handle) = current.take() {
                let _ = handle.sender.send(PlayerCommand::Shutdown);
            }
        }
    }

    fn open_external(&self, request: PlayerOpenRequest) -> Result<PlayerStatus, String> {
        if self.external_pid().is_some() && self.ipc_send_open(&request) {
            return Ok(PlayerStatus {
                ready: true,
                external: true,
                title: request.title,
                url: request.url,
                ..PlayerStatus::default()
            });
        }
        self.close_external();
        let exe = locate_mpv_exe()?;
        let mut command = std::process::Command::new(&exe);
        command
            .arg("--no-config")
            .arg("--no-terminal")
            .arg("--force-window=immediate")
            .arg("--keep-open=yes")
            .arg("--save-position-on-quit=yes")
            .arg("--title=WebHomeTV Player")
            .arg(format!("--input-ipc-server={MPV_IPC_NAME}"));
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data).join("webhtv-desktop");
            let watch_later = base.join("watch_later");
            command.arg(format!("--watch-later-dir={}", watch_later.display()));
            let log_file = base.join("mpv.log");
            command.arg(format!("--log-file={}", log_file.display()));
            command.arg("--msg-level=all=info");
        }
        if !request.title.is_empty() {
            command.arg(format!("--force-media-title={}", request.title));
        }
        if let Some(start) = request.start.filter(|start| start.is_finite() && *start > 0.0) {
            command.arg(format!("--start={start}"));
        }
        let headers: Vec<String> = request
            .headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect();
        if !headers.is_empty() {
            command.arg(format!("--http-header-fields={}", headers.join(", ")));
        }
        command.arg(&request.url);
        let child = command
            .spawn()
            .map_err(|error| format!("unable to start {}: {error}", exe.display()))?;
        let pid = child.id();
        if let Ok(mut current) = self.external.lock() {
            *current = Some(pid);
        }
        if let Ok(mut current) = self.external_ipc.lock() {
            *current = Some(MPV_IPC_NAME.to_string());
        }
        Ok(PlayerStatus {
            ready: true,
            external: true,
            title: request.title,
            url: request.url,
            ..PlayerStatus::default()
        })
    }

    fn ipc_send_open(&self, request: &PlayerOpenRequest) -> bool {
        let ipc_name = match self.external_ipc.lock() {
            Ok(current) => match current.clone() {
                Some(name) => name,
                None => return false,
            },
            Err(_) => return false,
        };
        let (sender, receiver) = mpsc::channel();
        let request = request.clone();
        let _ = thread::Builder::new()
            .name("webhtv-mpv-ipc".to_string())
            .spawn(move || {
                let _ = sender.send(ipc_write_commands(&ipc_name, &request));
            });
        match receiver.recv_timeout(Duration::from_millis(4000)) {
            Ok(Ok(())) => true,
            _ => false,
        }
    }

    fn external_pid(&self) -> Option<u32> {
        let pid = self.external.lock().ok().and_then(|external| *external)?;
        if process_is_running(pid) {
            return Some(pid);
        }
        if let Ok(mut current) = self.external.lock() {
            *current = None;
        }
        if let Ok(mut current) = self.external_ipc.lock() {
            *current = None;
        }
        None
    }

    fn external_status(&self) -> PlayerStatus {
        PlayerStatus {
            ready: true,
            external: true,
            ..PlayerStatus::default()
        }
    }

    fn close_external(&self) {
        let pid = self.external.lock().ok().and_then(|external| *external);
        if let Some(pid) = pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Ok(mut current) = self.external.lock() {
                *current = None;
            }
        }
        if let Ok(mut current) = self.external_ipc.lock() {
            *current = None;
        }
    }

    fn ensure_handle(&self) -> Result<PlayerHandle, String> {
        let mut current = self
            .handle
            .lock()
            .map_err(|_| "player registry is unavailable".to_string())?;
        if let Some(handle) = current.as_ref() {
            if handle.is_alive() {
                return Ok(handle.clone());
            }
            *current = None;
        }
        let surface = self.surface.lock().ok().and_then(|surface| *surface);
        let created = PlayerHandle::spawn(locate_mpv_dll()?, surface)?;
        *current = Some(created.clone());
        Ok(created)
    }

    fn set_surface(&self, surface: Option<isize>) {
        self.close();
        if let Ok(mut current) = self.surface.lock() {
            *current = surface;
        }
    }

    fn surface(&self) -> Option<isize> {
        self.surface.lock().ok().and_then(|surface| *surface)
    }

    fn invalidate_handle(&self) {
        if let Ok(mut current) = self.handle.lock() {
            *current = None;
        }
    }
}

impl Drop for PlayerManager {
    fn drop(&mut self) {
        self.close();
    }
}

impl PlayerHandle {
    fn spawn(dll_path: PathBuf, surface: Option<isize>) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(16);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("webhtv-mpv".to_string())
            .spawn(
                move || match PlayerEngine::new_with_surface(dll_path, surface) {
                    Ok(engine) => {
                        if ready_sender.send(Ok(())).is_ok() {
                            run_worker(engine, receiver);
                        }
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                    }
                },
            )
            .map_err(|error| format!("unable to start player worker: {error}"))?;
        ready_receiver
            .recv()
            .map_err(|_| "player worker stopped during initialization".to_string())??;
        Ok(Self { sender })
    }

    fn is_alive(&self) -> bool {
        let (reply, receiver) = mpsc::channel();
        self.sender.send(PlayerCommand::Status { reply }).is_ok() && receiver.recv().is_ok()
    }
}

impl PlayerEngine {
    #[cfg(test)]
    fn new(dll_path: PathBuf) -> Result<Self, String> {
        Self::new_with_surface(dll_path, None)
    }

    fn new_with_surface(dll_path: PathBuf, surface: Option<isize>) -> Result<Self, String> {
        let api = MpvApi::load(&dll_path)?;
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create returned null".to_string());
        }
        let mut engine = Self {
            api,
            handle,
            dll_path,
        };
        for (name, value) in [
            ("config", "no"),
            ("terminal", "no"),
            ("input-default-bindings", "yes"),
            ("input-vo-keyboard", "yes"),
            (
                "force-window",
                if surface.is_some() {
                    "yes"
                } else {
                    "immediate"
                },
            ),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("hwdec", "auto-safe"),
            ("title", "WebHomeTV Player"),
        ] {
            engine.set_option(name, value)?;
        }
        if let Some(surface) = surface {
            engine.set_option("wid", &surface.to_string())?;
        }
        let result = unsafe { (engine.api.initialize)(engine.handle) };
        engine.check(result, "mpv_initialize")?;
        Ok(engine)
    }

    fn open(&mut self, request: PlayerOpenRequest) -> Result<PlayerStatus, String> {
        let url = request.url.trim();
        if url.is_empty() {
            return Err("player URL cannot be empty".to_string());
        }
        self.set_property("force-media-title", request.title.trim())?;
        let headers = request
            .headers
            .iter()
            .filter(|(name, value)| !name.trim().is_empty() && !value.trim().is_empty())
            .map(|(name, value)| format!("{}: {}", name.trim(), value.trim()))
            .collect::<Vec<_>>()
            .join(",");
        self.set_property("http-header-fields", &headers)?;
        self.command(&["loadfile", url, "replace"])?;
        if let Some(start) = request
            .start
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            self.command(&["seek", &start.to_string(), "absolute"])?;
        }
        self.status()
    }

    fn control(&mut self, request: PlayerControlRequest) -> Result<PlayerStatus, String> {
        match request.command.as_str() {
            "togglePause" => self.command(&["cycle", "pause"]),
            "pause" => self.set_property("pause", "yes"),
            "resume" => self.set_property("pause", "no"),
            "stop" => self.command(&["stop"]),
            "fullscreen" => self.command(&["cycle", "fullscreen"]),
            "seek" => {
                let value = finite_value(request.value, "seek")?;
                self.command(&["seek", &value.to_string(), "relative"])
            }
            "volume" => {
                let value = finite_value(request.value, "volume")?.clamp(0.0, 130.0);
                self.set_property("volume", &value.to_string())
            }
            "speed" => {
                let value = finite_value(request.value, "speed")?.clamp(0.25, 4.0);
                self.set_property("speed", &value.to_string())
            }
            other => Err(format!("unsupported player command `{other}`")),
        }?;
        self.status()
    }

    fn status(&self) -> Result<PlayerStatus, String> {
        let version = unsafe { (self.api.client_api_version)() };
        let major = version >> 16;
        let minor = version & 0xffff;
        Ok(PlayerStatus {
            ready: true,
            idle: self.flag_property("idle-active").unwrap_or(true),
            paused: self.flag_property("pause").unwrap_or(false),
            external: false,
            title: self.string_property("media-title").unwrap_or_default(),
            url: self.string_property("path").unwrap_or_default(),
            time: self.double_property("time-pos").unwrap_or(0.0),
            duration: self.double_property("duration").unwrap_or(0.0),
            volume: self.double_property("volume").unwrap_or(100.0),
            speed: self.double_property("speed").unwrap_or(1.0),
            api_version: format!("{major}.{minor}"),
            dll_path: self.dll_path.to_string_lossy().into_owned(),
        })
    }

    fn shutdown_requested(&self) -> bool {
        loop {
            let event = unsafe { (self.api.wait_event)(self.handle, 0.0) };
            if event.is_null() {
                return false;
            }
            match unsafe { (*event).event_id } {
                MPV_EVENT_SHUTDOWN => return true,
                MPV_EVENT_NONE => return false,
                _ => {}
            }
        }
    }

    fn set_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        let name = cstring(name)?;
        let value = cstring(value)?;
        let result =
            unsafe { (self.api.set_option_string)(self.handle, name.as_ptr(), value.as_ptr()) };
        self.check(result, "mpv_set_option_string")
            .map_err(|error| format!("option `{name:?}`: {error}"))
    }

    fn set_property(&self, name: &str, value: &str) -> Result<(), String> {
        let name = cstring(name)?;
        let value = cstring(value)?;
        let result =
            unsafe { (self.api.set_property_string)(self.handle, name.as_ptr(), value.as_ptr()) };
        self.check(result, "mpv_set_property_string")
    }

    fn command(&self, arguments: &[&str]) -> Result<(), String> {
        let values = arguments
            .iter()
            .map(|value| cstring(value))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers = values
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        pointers.push(std::ptr::null());
        let result = unsafe { (self.api.command)(self.handle, pointers.as_ptr()) };
        self.check(result, "mpv_command")
    }

    fn flag_property(&self, name: &str) -> Result<bool, String> {
        let name = cstring(name)?;
        let mut value: c_int = 0;
        let result = unsafe {
            (self.api.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                (&mut value as *mut c_int).cast(),
            )
        };
        self.check(result, "mpv_get_property")?;
        Ok(value != 0)
    }

    fn double_property(&self, name: &str) -> Result<f64, String> {
        let name = cstring(name)?;
        let mut value: c_double = 0.0;
        let result = unsafe {
            (self.api.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                (&mut value as *mut c_double).cast(),
            )
        };
        self.check(result, "mpv_get_property")?;
        Ok(value)
    }

    fn string_property(&self, name: &str) -> Result<String, String> {
        let name = cstring(name)?;
        let value = unsafe { (self.api.get_property_string)(self.handle, name.as_ptr()) };
        if value.is_null() {
            return Err("mpv property is unavailable".to_string());
        }
        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.api.free)(value.cast()) };
        Ok(text)
    }

    fn check(&self, result: c_int, operation: &str) -> Result<(), String> {
        if result >= 0 {
            return Ok(());
        }
        let message = unsafe {
            let value = (self.api.error_string)(result);
            if value.is_null() {
                "unknown mpv error".to_string()
            } else {
                CStr::from_ptr(value).to_string_lossy().into_owned()
            }
        };
        Err(format!("{operation} failed: {message} ({result})"))
    }
}

impl Drop for PlayerEngine {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.api.terminate_destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

impl MpvApi {
    fn load(path: &Path) -> Result<Self, String> {
        let library = unsafe { Library::new(path) }
            .map_err(|error| format!("unable to load {}: {error}", path.display()))?;
        unsafe {
            Ok(Self {
                create: load_symbol(&library, b"mpv_create\0")?,
                initialize: load_symbol(&library, b"mpv_initialize\0")?,
                set_option_string: load_symbol(&library, b"mpv_set_option_string\0")?,
                set_property_string: load_symbol(&library, b"mpv_set_property_string\0")?,
                get_property: load_symbol(&library, b"mpv_get_property\0")?,
                get_property_string: load_symbol(&library, b"mpv_get_property_string\0")?,
                command: load_symbol(&library, b"mpv_command\0")?,
                wait_event: load_symbol(&library, b"mpv_wait_event\0")?,
                terminate_destroy: load_symbol(&library, b"mpv_terminate_destroy\0")?,
                error_string: load_symbol(&library, b"mpv_error_string\0")?,
                free: load_symbol(&library, b"mpv_free\0")?,
                client_api_version: load_symbol(&library, b"mpv_client_api_version\0")?,
                _library: library,
            })
        }
    }
}

fn run_worker(mut engine: PlayerEngine, receiver: mpsc::Receiver<PlayerCommand>) {
    loop {
        if engine.shutdown_requested() {
            break;
        }
        let command = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match command {
            PlayerCommand::Open { request, reply } => {
                let _ = reply.send(engine.open(request));
            }
            PlayerCommand::Control { request, reply } => {
                let _ = reply.send(engine.control(request));
            }
            PlayerCommand::Status { reply } => {
                let _ = reply.send(engine.status());
            }
            PlayerCommand::Shutdown => break,
        }
    }
}

unsafe fn load_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|error| {
            format!(
                "libmpv symbol {} is unavailable: {error}",
                String::from_utf8_lossy(name).trim_end_matches('\0')
            )
        })
}

fn locate_mpv_exe() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("WEBHTV_MPV_EXE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err("WEBHTV_MPV_EXE does not point to a file".to_string());
    }
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.extend([
                directory.join("mpv.exe"),
                directory.join("mpv/mpv.exe"),
                directory.join("resources/mpv/mpv.exe"),
            ]);
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mpv/mpv.exe"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "mpv.exe was not found. Set WEBHTV_MPV_EXE or run scripts/fetch_mpv.ps1.".to_string()
        })
}

fn process_is_running(pid: u32) -> bool {
    let handle = match unsafe {
        windows::Win32::System::Threading::OpenProcess(
            windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    } {
        Ok(handle) => handle,
        Err(_) => return false,
    };
    let mut exit_code = 0u32;
    let running = unsafe {
        windows::Win32::System::Threading::GetExitCodeProcess(handle, &mut exit_code).is_ok()
            && exit_code == 259
    };
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(handle);
    }
    running
}

fn locate_mpv_dll() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("WEBHTV_MPV_DLL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err("WEBHTV_MPV_DLL does not point to a file".to_string());
    }
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            candidates.extend([
                directory.join("libmpv-2.dll"),
                directory.join("mpv/libmpv-2.dll"),
                directory.join("resources/mpv/libmpv-2.dll"),
            ]);
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/mpv/libmpv-2.dll"));
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "libmpv-2.dll was not found. Set WEBHTV_MPV_DLL or run scripts/fetch_mpv.ps1."
                .to_string()
        })
}

fn ipc_write_commands(ipc_name: &str, request: &PlayerOpenRequest) -> Result<(), String> {
    use std::io::Write as IoWrite;

    let pipe = format!(r"\\.\pipe\{ipc_name}");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&pipe)
        .map_err(|error| format!("unable to open mpv IPC pipe {pipe}: {error}"))?;
    let mut commands: Vec<serde_json::Value> = Vec::new();
    if !request.title.is_empty() {
        commands.push(serde_json::json!({
            "command": ["set_property", "force-media-title", request.title],
            "request_id": 1,
        }));
    }
    let headers: Vec<String> = request
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    if !headers.is_empty() {
        commands.push(serde_json::json!({
            "command": ["set_property", "http-header-fields", headers.join(", ")],
            "request_id": 2,
        }));
    }
    commands.push(serde_json::json!({
        "command": ["loadfile", request.url, "replace"],
        "request_id": 3,
    }));
    if let Some(start) = request.start.filter(|start| start.is_finite() && *start > 0.0) {
        commands.push(serde_json::json!({
            "command": ["seek", start.to_string(), "absolute"],
            "request_id": 4,
        }));
    }
    for command in commands {
        let mut line = command.to_string();
        line.push('\n');
        file.write_all(line.as_bytes())
            .map_err(|error| format!("unable to write mpv IPC command: {error}"))?;
    }
    Ok(())
}

fn finite_value(value: Option<f64>, name: &str) -> Result<f64, String> {
    value
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("player command `{name}` requires a finite value"))
}

fn probe_proxy_text_error(request: &PlayerOpenRequest) -> Result<(), String> {
    let url = request.url.trim();
    let is_proxy = url.starts_with("http://127.0.0.1:9978/proxy")
        || url.starts_with("http://localhost:9978/proxy")
        || url.starts_with("http://[::1]:9978/proxy");
    if !is_proxy {
        return Ok(());
    }
    let mut command = std::process::Command::new("curl");
    command.arg("-sS").arg("-m").arg("8").arg("-I");
    for (name, value) in &request.headers {
        command.arg("-H").arg(format!("{name}: {value}"));
    }
    command.arg(url);
    let output = match command.output() {
        Ok(output) => output,
        Err(_) => return Ok(()),
    };
    if !output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let content_type = stdout
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
        .and_then(|line| line.splitn(2, ':').nth(1))
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.is_empty() {
        return Ok(());
    }
    let is_media = content_type.starts_with("video/")
        || content_type.contains("octet-stream")
        || content_type.contains("oct-stream")
        || content_type.contains("mpegurl")
        || content_type.contains("dash+xml")
        || content_type.contains("mp2t")
        || content_type.contains("mp4");
    if content_type.starts_with("text/") && !is_media {
        return Err("播放源返回的不是视频流（多为网盘未登录、未转存或链接已失效）".to_string());
    }
    Ok(())
}

fn unwrap_legacy_pvideo_request(
    mut request: PlayerOpenRequest,
) -> Result<PlayerOpenRequest, String> {
    let Ok(url) = reqwest::Url::parse(request.url.trim()) else {
        return Ok(request);
    };
    let host = url.host_str().unwrap_or_default();
    if url.port() != Some(1314)
        || !(host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost"))
    {
        return Ok(request);
    }

    let mut upstream = String::new();
    let mut encoded_headers = String::new();
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "url" if upstream.is_empty() => upstream = value.into_owned(),
            "header" if encoded_headers.is_empty() => encoded_headers = value.into_owned(),
            _ => {}
        }
    }
    request.url = decode_pvideo_url(&upstream).ok_or_else(|| {
        "cloud-drive playback proxy did not contain a valid upstream media URL".to_string()
    })?;
    for (name, value) in decode_pvideo_headers(&encoded_headers) {
        if let Some(existing) = request
            .headers
            .keys()
            .find(|existing| existing.eq_ignore_ascii_case(&name))
            .cloned()
        {
            request.headers.remove(&existing);
        }
        request.headers.insert(name, value);
    }
    Ok(request)
}

fn decode_pvideo_url(value: &str) -> Option<String> {
    let mut current = value.trim().to_string();
    for _ in 0..3 {
        if reqwest::Url::parse(&current)
            .ok()
            .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        {
            return Some(current);
        }
        let decoded = percent_decode_str(&current)
            .decode_utf8_lossy()
            .into_owned();
        if decoded == current {
            break;
        }
        current = decoded;
    }
    None
}

fn decode_pvideo_headers(value: &str) -> BTreeMap<String, String> {
    let mut current = value.trim().to_string();
    for _ in 0..3 {
        if let Ok(value) = serde_json::from_str::<Value>(&current) {
            match value {
                Value::Object(headers) => {
                    return headers
                        .into_iter()
                        .filter_map(|(name, value)| {
                            value.as_str().and_then(|value| {
                                let name = name.trim();
                                let value = value.trim();
                                (!name.is_empty() && !value.is_empty())
                                    .then(|| (name.to_string(), value.to_string()))
                            })
                        })
                        .collect();
                }
                Value::String(value) => {
                    current = value;
                    continue;
                }
                _ => return BTreeMap::new(),
            }
        }
        let decoded = percent_decode_str(&current)
            .decode_utf8_lossy()
            .into_owned();
        if decoded == current {
            break;
        }
        current = decoded;
    }
    BTreeMap::new()
}

fn default_true() -> bool {
    true
}

#[cfg(windows)]
static SURFACE_WNDPROCS: OnceLock<Mutex<HashMap<isize, WNDPROC>>> = OnceLock::new();

#[cfg(windows)]
fn surface_wndprocs() -> &'static Mutex<HashMap<isize, WNDPROC>> {
    SURFACE_WNDPROCS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
unsafe extern "system" fn player_surface_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    if msg == WM_DESTROY {
        if let Ok(mut procs) = surface_wndprocs().lock() {
            procs.remove(&(hwnd.0 as isize));
        }
    }
    let previous = surface_wndprocs()
        .lock()
        .ok()
        .and_then(|procs| procs.get(&(hwnd.0 as isize)).copied().flatten());
    if let Some(previous) = previous {
        unsafe { CallWindowProcW(Some(previous), hwnd, msg, wparam, lparam) }
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

type PlayerSurfaceProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[cfg(windows)]
fn create_player_surface(window: &WebviewWindow) -> Result<isize, String> {
    let parent = window
        .hwnd()
        .map_err(|error| format!("unable to access the main window handle: {error}"))?;
    let child = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            None,
            None,
        )
    }
    .map_err(|error| format!("unable to create the embedded player surface: {error}"))?;
    let hwnd = HWND(child.0 as *mut _);
    let previous = unsafe {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, (player_surface_proc as PlayerSurfaceProc) as usize as isize)
    };
    let previous = if previous == 0 {
        None
    } else {
        Some(unsafe { std::mem::transmute::<isize, PlayerSurfaceProc>(previous) })
    };
    if let Ok(mut procs) = surface_wndprocs().lock() {
        procs.insert(child.0 as isize, previous);
    }
    Ok(child.0 as isize)
}

#[cfg(windows)]
fn update_player_surface(surface: isize, bounds: &PlayerSurfaceBounds) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;

    let hwnd = HWND(surface as *mut _);
    if !bounds.visible || bounds.width <= 0 || bounds.height <= 0 {
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        return Ok(());
    }
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            bounds.x,
            bounds.y,
            bounds.width.max(1),
            bounds.height.max(1),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    }
    .map_err(|error| format!("unable to position the embedded player surface: {error}"))?;
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    Ok(())
}

#[cfg(windows)]
fn destroy_player_surface(surface: isize) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;

    unsafe { DestroyWindow(HWND(surface as *mut _)) }
        .map_err(|error| format!("unable to destroy the embedded player surface: {error}"))
}

fn cstring(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "mpv argument contains an embedded NUL byte".to_string())
}

async fn resolve_page_url(
    client: &reqwest::Client,
    request: PlayerResolveRequest,
) -> Result<PlayerResolveResponse, String> {
    let html = fetch_text(client, &request.url, &request.headers).await?;
    let candidate = extract_media_url(&request.url, &html)?;
    if !media_response(client, &candidate, &request.headers).await {
        return Err("parser candidate did not return a video or stream response".to_string());
    }
    Ok(PlayerResolveResponse {
        kind: media_kind(&candidate).to_string(),
        url: candidate,
        headers: request.headers,
    })
}

async fn fetch_text(
    client: &reqwest::Client,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut builder = client.get(url);
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid playback header name `{name}`"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| format!("invalid playback header `{name}` value"))?;
        builder = builder.header(name, value);
    }
    let response = builder
        .send()
        .await
        .map_err(|error| format!("unable to fetch parser page: {error}"))?
        .error_for_status()
        .map_err(|error| format!("parser page returned an error: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > 8 * 1024 * 1024)
    {
        return Err("parser response is larger than 8 MiB".to_string());
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("unable to read parser response: {error}"))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > 8 * 1024 * 1024 {
            return Err("parser response is larger than 8 MiB".to_string());
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn resolve_with_configuration(
    client: &reqwest::Client,
    request: PlayerResolveRequest,
    parsers: &[Parse],
    active_parser: &str,
) -> Result<PlayerResolveResponse, String> {
    if let Some(parser_url) = temporary_json_parser_url(&request.play_url) {
        let parser_url = parser_target_url(parser_url, &request.url);
        return resolve_json_parser(client, &parser_url, &request.headers).await;
    }
    let parser_name = parser_name(&request, active_parser);
    let Some(parser_name) = parser_name else {
        return resolve_page_url(client, request).await;
    };
    let parser = parsers
        .iter()
        .find(|parser| parser.name == parser_name)
        .ok_or_else(|| format!("parser `{parser_name}` is not present in the active config"))?;
    let headers = merge_parser_headers(&request.headers, parser);
    let parser_url = parser_target_url(&parser.url, &request.url);
    match parser.parse_type {
        0 => {
            resolve_page_url(
                client,
                PlayerResolveRequest {
                    url: parser_url,
                    headers,
                    ..request
                },
            )
            .await
        }
        1 => resolve_json_parser(client, &parser_url, &headers).await,
        4 => resolve_aggregate_parser(client, request, parsers).await,
        other => Err(format!(
            "parser `{parser_name}` type {other} requires the active Java Spider runtime"
        )),
    }
}

async fn resolve_json_parser(
    client: &reqwest::Client,
    parser_url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<PlayerResolveResponse, String> {
    let body = fetch_text(client, parser_url, headers).await?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("JSON parser returned invalid JSON: {error}"))?;
    let candidate = json_url(&value)
        .ok_or_else(|| "JSON parser response did not contain a media URL".to_string())?;
    let json_headers = json_headers(&value);
    let response_headers = merge_header_value(headers, &json_headers);
    if !is_media_url(&candidate) {
        return resolve_page_url(
            client,
            PlayerResolveRequest {
                url: candidate,
                headers: response_headers,
                ..Default::default()
            },
        )
        .await;
    }
    if !media_response(client, &candidate, &response_headers).await {
        return Err("JSON parser URL did not return a video or stream response".to_string());
    }
    Ok(PlayerResolveResponse {
        kind: media_kind(&candidate).to_string(),
        url: candidate,
        headers: response_headers,
    })
}

async fn resolve_aggregate_parser(
    client: &reqwest::Client,
    request: PlayerResolveRequest,
    parsers: &[Parse],
) -> Result<PlayerResolveResponse, String> {
    let mut errors = Vec::new();
    for parser in parsers_for_flag(parsers, &request.flag) {
        let headers = merge_parser_headers(&request.headers, parser);
        let target = parser_target_url(&parser.url, &request.url);
        let result = match parser.parse_type {
            0 => {
                resolve_page_url(
                    client,
                    PlayerResolveRequest {
                        url: target,
                        headers,
                        ..request.clone()
                    },
                )
                .await
            }
            1 => resolve_json_parser(client, &target, &headers).await,
            _ => continue,
        };
        match result {
            Ok(result) => return Ok(result),
            Err(error) => errors.push(format!("{}: {error}", parser.name)),
        }
    }

    match resolve_page_url(client, request).await {
        Ok(result) => Ok(result),
        Err(error) if errors.is_empty() => Err(error),
        Err(error) => Err(format!(
            "all aggregate parsers failed: {}; direct page: {error}",
            errors.join("; ")
        )),
    }
}

async fn resolve_jar_parser(
    app: &AppHandle,
    state: &SharedState,
    config_id: i64,
    site: Site,
    request: PlayerResolveRequest,
    parser: &Parse,
    parsers: &[Parse],
) -> Result<PlayerResolveResponse, String> {
    let headers = merge_parser_headers(&request.headers, parser);
    let parser_map = if parser.parse_type == 2 {
        json_parser_map(parsers)
    } else {
        mix_parser_map(parsers)
    };
    let value = state
        .spiders
        .parse(
            config_id,
            site,
            JavaParserRequest {
                parser_type: parser.parse_type,
                parser_key: parser.url.clone(),
                parser_name: parser.name.clone(),
                flag: request.flag.clone(),
                url: request.url.clone(),
                parsers: parser_map,
            },
        )
        .await?;
    let candidate = json_url(&value)
        .ok_or_else(|| "JAR parser response did not contain a media URL".to_string())?;
    let response_headers = merge_header_value(&headers, &json_headers(&value));
    if !is_media_url(&candidate) {
        let page_request = PlayerResolveRequest {
            url: candidate,
            headers: response_headers,
            click: parser.click.clone(),
            ..Default::default()
        };
        return match resolve_page_url(&state.http, page_request.clone()).await {
            Ok(result) => Ok(result),
            Err(error) => sniff_webview(app, &state.http, page_request)
                .await
                .map_err(|sniff_error| format!("{error}; WebView fallback: {sniff_error}")),
        };
    }
    if !media_response(&state.http, &candidate, &response_headers).await {
        return Err("JAR parser URL did not return a video or stream response".to_string());
    }
    Ok(PlayerResolveResponse {
        kind: media_kind(&candidate).to_string(),
        url: candidate,
        headers: response_headers,
    })
}

fn json_parser_map(parsers: &[Parse]) -> Value {
    let mut map = serde_json::Map::new();
    for parser in parsers.iter().filter(|parser| parser.parse_type == 1) {
        map.insert(parser.name.clone(), Value::String(parser_ext_url(parser)));
    }
    Value::Object(map)
}

fn mix_parser_map(parsers: &[Parse]) -> Value {
    let mut map = serde_json::Map::new();
    for parser in parsers {
        let mut item = serde_json::Map::new();
        item.insert(
            "type".to_string(),
            Value::String(parser.parse_type.to_string()),
        );
        item.insert("ext".to_string(), Value::String(json_text(&parser.ext)));
        item.insert("url".to_string(), Value::String(parser.url.clone()));
        map.insert(parser.name.clone(), Value::Object(item));
    }
    Value::Object(map)
}

fn parser_ext_url(parser: &Parse) -> String {
    if parser.ext.is_null()
        || parser
            .ext
            .as_object()
            .is_some_and(|object| object.is_empty())
    {
        return parser.url.clone();
    }
    let Some(index) = parser.url.find('?') else {
        return parser.url.clone();
    };
    let encoded = URL_SAFE.encode(json_text(&parser.ext).as_bytes());
    format!(
        "{}?cat_ext={encoded}&{}",
        &parser.url[..index],
        &parser.url[index + 1..]
    )
}

fn json_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn merge_parser_headers(
    base: &BTreeMap<String, String>,
    parser: &Parse,
) -> BTreeMap<String, String> {
    let mut headers = merge_header_value(base, &parser.header);
    if let Some(value) = parser.ext.get("header") {
        headers = merge_header_value(&headers, value);
    }
    headers
}

fn parsers_for_flag<'a>(parsers: &'a [Parse], flag: &str) -> Vec<&'a Parse> {
    let candidates = parsers
        .iter()
        .filter(|parser| parser.parse_type == 0 || parser.parse_type == 1)
        .collect::<Vec<_>>();
    if flag.trim().is_empty() {
        return candidates;
    }
    let filtered = candidates
        .iter()
        .copied()
        .filter(|parser| parser_flags(parser).iter().any(|item| item == flag))
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        candidates
    } else {
        filtered
    }
}

fn parser_flags(parser: &Parse) -> Vec<String> {
    parser
        .ext
        .get("flag")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn parser_name(request: &PlayerResolveRequest, active_parser: &str) -> Option<String> {
    let play_url = request.play_url.trim();
    if let Some(name) = play_url
        .strip_prefix("parse:")
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return Some(name.to_string());
    }
    if request.parse != 0 || request.jx != 0 {
        return (!active_parser.trim().is_empty()).then(|| active_parser.trim().to_string());
    }
    None
}

fn temporary_json_parser_url(play_url: &str) -> Option<&str> {
    play_url
        .strip_prefix("json:")
        .map(str::trim)
        .filter(|url| !url.is_empty())
}

fn parser_target_url(parser_url: &str, target: &str) -> String {
    if parser_url.contains("{url}") {
        return parser_url.replace("{url}", target);
    }
    if parser_url.contains("{}") {
        return parser_url.replace("{}", target);
    }
    format!("{parser_url}{target}")
}

fn json_url(value: &Value) -> Option<String> {
    for key in ["url", "playUrl", "play_url"] {
        if let Some(url) = value
            .get(key)
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
        {
            return Some(url.trim().to_string());
        }
    }
    if let Some(data) = value.get("data") {
        if let Some(url) = json_url(data) {
            return Some(url);
        }
        if let Some(text) = data.as_str() {
            if let Ok(nested) = serde_json::from_str::<Value>(text) {
                return json_url(&nested);
            }
        }
    }
    value.get("result").and_then(json_url)
}

fn json_headers(value: &Value) -> Value {
    value
        .get("headers")
        .or_else(|| value.get("header"))
        .or_else(|| value.get("data").and_then(|data| data.get("headers")))
        .cloned()
        .unwrap_or(Value::Null)
}

fn merge_header_value(base: &BTreeMap<String, String>, value: &Value) -> BTreeMap<String, String> {
    let mut headers = base.clone();
    let value = if let Value::String(text) = value {
        serde_json::from_str::<Value>(text).unwrap_or(Value::Null)
    } else {
        value.clone()
    };
    if let Value::Object(object) = value {
        for (name, value) in object {
            if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
                headers.insert(name, value.to_string());
            }
        }
    }
    headers
}

async fn media_response(
    client: &reqwest::Client,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> bool {
    let mut builder = client.head(url);
    for (name, value) in headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_str(value) else {
            continue;
        };
        builder = builder.header(name, value);
    }
    let Ok(response) = builder.send().await else {
        return is_media_url(url);
    };
    if !response.status().is_success() {
        return is_media_url(url);
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("text/html") {
        return false;
    }
    content_type.starts_with("video/")
        || content_type.contains("mpegurl")
        || content_type.contains("dash+xml")
        || is_media_url(url)
}

fn extract_media_url(page_url: &str, html: &str) -> Result<String, String> {
    let absolute =
        Regex::new(r#"(?i)(?:https?:)?//[^"'<>\s]+?\.(?:mp4|m3u8|mpd)(?:\?[^"'<>\s]*)?"#)
            .map_err(|error| format!("media URL matcher failed: {error}"))?;
    let attribute =
        Regex::new(r#"(?i)(?:video|file|src|url|source|playurl)\s*[:=]\s*["']([^"']+)["']"#)
            .map_err(|error| format!("media attribute matcher failed: {error}"))?;
    let mut candidates = Vec::new();
    candidates.extend(
        absolute
            .find_iter(html)
            .map(|match_| match_.as_str().to_string()),
    );
    candidates.extend(
        attribute
            .captures_iter(html)
            .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_string())),
    );
    for raw in candidates {
        let raw = raw
            .replace("\\/", "/")
            .replace("\\u0026", "&")
            .replace("&amp;", "&")
            .trim_matches(|character: char| "'\"),;]}".contains(character))
            .to_string();
        let url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw
        } else if raw.starts_with("//") {
            format!("https:{raw}")
        } else {
            reqwest::Url::parse(page_url)
                .and_then(|base| base.join(&raw))
                .map_err(|_| "parser returned an invalid relative media URL".to_string())?
                .to_string()
        };
        if is_media_url(&url) {
            return Ok(url);
        }
    }
    Err("parser page did not expose an mp4, m3u8, or mpd media URL".to_string())
}

async fn sniff_webview(
    app: &AppHandle,
    client: &reqwest::Client,
    request: PlayerResolveRequest,
) -> Result<PlayerResolveResponse, String> {
    let url =
        tauri::Url::parse(&request.url).map_err(|_| "Web parser URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Web parser requires an http or https URL".to_string());
    }
    let label = format!(
        "webhtv-sniff-{}",
        WEB_SNIFF_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("WebHomeTV parser")
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .skip_taskbar(true)
        .inner_size(1.0, 1.0)
        .resizable(false)
        .initialization_script_for_all_frames(WEB_SNIFF_SCRIPT)
        .build()
        .map_err(|error| format!("unable to create hidden parser WebView: {error}"))?;

    let deadline = Instant::now() + WEB_SNIFF_TIMEOUT;
    let mut click_applied = false;
    let result = 'poll: loop {
        if Instant::now() >= deadline {
            break Err("WebView parser timed out without finding a media URL".to_string());
        }
        let (sender, receiver) = mpsc::channel();
        if window
            .eval_with_callback(WEB_SNIFF_PROBE, move |value| {
                let _ = sender.send(value);
            })
            .is_err()
        {
            continue;
        }
        let probe = tauri::async_runtime::spawn_blocking(move || {
            receiver.recv_timeout(Duration::from_millis(350)).ok()
        })
        .await
        .map_err(|error| format!("WebView probe task failed: {error}"))?;
        if !click_applied && !request.click.trim().is_empty() {
            let click = request.click.trim();
            let script = format!("try {{ {click} }} catch (_) {{}};");
            let _ = window.eval(script);
            click_applied = true;
        }
        if let Some(probe) = probe {
            for candidate in browser_probe_urls(&probe) {
                if !media_response(client, &candidate, &request.headers).await
                    && is_media_url(&candidate)
                {
                    continue;
                }
                break 'poll Ok(PlayerResolveResponse {
                    kind: media_kind(&candidate).to_string(),
                    url: candidate,
                    headers: request.headers.clone(),
                });
            }
        }
        let _ = tauri::async_runtime::spawn_blocking(|| {
            thread::sleep(Duration::from_millis(100));
        })
        .await;
    };
    let _ = window.close();
    result
}

fn browser_probe_urls(value: &str) -> Vec<String> {
    let mut parsed = serde_json::from_str::<Value>(value).ok();
    for _ in 0..2 {
        let Some(Value::String(text)) = parsed else {
            break;
        };
        parsed = serde_json::from_str(&text).ok();
    }
    match parsed {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .collect(),
        _ => Vec::new(),
    }
}

fn is_media_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    [".mp4", ".m3u8", ".m3u", ".mpd", "type=mpd", "format=mpd"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn media_kind(url: &str) -> &str {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".mpd") || lower.contains("type=mpd") || lower.contains("format=mpd") {
        "dash"
    } else if lower.contains(".m3u") {
        "hls"
    } else {
        "file"
    }
}

fn web_fallback_request(
    request: &PlayerResolveRequest,
    parsers: &[Parse],
    active_parser: &str,
) -> Option<PlayerResolveRequest> {
    if temporary_json_parser_url(&request.play_url).is_some() {
        return None;
    }
    let selected = parser_name(request, active_parser)
        .and_then(|name| parsers.iter().find(|parser| parser.name == name));
    match selected.map(|parser| parser.parse_type) {
        Some(0) => {
            let parser = selected.expect("selected parser must exist");
            Some(PlayerResolveRequest {
                url: parser_target_url(&parser.url, &request.url),
                click: parser.click.clone(),
                ..request.clone()
            })
        }
        Some(4) => {
            let web_parser = parsers_for_flag(parsers, &request.flag)
                .into_iter()
                .find(|parser| parser.parse_type == 0);
            web_parser.map_or_else(
                || Some(request.clone()),
                |parser| {
                    Some(PlayerResolveRequest {
                        url: parser_target_url(&parser.url, &request.url),
                        click: parser.click.clone(),
                        ..request.clone()
                    })
                },
            )
        }
        None => Some(request.clone()),
        Some(1..=3) => None,
        Some(_) => None,
    }
}

#[tauri::command]
pub async fn player_resolve(
    app: AppHandle,
    request: PlayerResolveRequest,
    state: State<'_, SharedState>,
) -> Result<PlayerResolveResponse, String> {
    let active = state.database.active_config()?;
    if let Some(active) = active {
        if let Some(parser) = parser_name(&request, &active.document.parse).and_then(|name| {
            active
                .document
                .parses
                .iter()
                .find(|parser| parser.name == name)
        }) {
            if parser.parse_type == 2 || parser.parse_type == 3 {
                let site_key = if request.site_key.trim().is_empty() {
                    active.summary.home_key.as_str()
                } else {
                    request.site_key.trim()
                };
                let site = active
                    .document
                    .sites
                    .iter()
                    .find(|site| site.key == site_key)
                    .cloned()
                    .ok_or_else(|| {
                        format!("site `{site_key}` is not in the active configuration")
                    })?;
                return resolve_jar_parser(
                    &app,
                    state.inner(),
                    active.summary.id,
                    site,
                    request,
                    parser,
                    &active.document.parses,
                )
                .await;
            }
        }
        let fallback =
            web_fallback_request(&request, &active.document.parses, &active.document.parse);
        return match resolve_with_configuration(
            &state.http,
            request,
            &active.document.parses,
            &active.document.parse,
        )
        .await
        {
            Ok(result) => Ok(result),
            Err(error) => match fallback {
                Some(request) => sniff_webview(&app, &state.http, request)
                    .await
                    .map_err(|sniff_error| format!("{error}; WebView fallback: {sniff_error}")),
                None => Err(error),
            },
        };
    }
    let original = request.clone();
    match resolve_page_url(&state.http, request).await {
        Ok(result) => Ok(result),
        Err(error) => sniff_webview(&app, &state.http, original)
            .await
            .map_err(|sniff_error| format!("{error}; WebView fallback: {sniff_error}")),
    }
}

#[tauri::command]
pub fn player_progress_get(
    request: PlayerProgressKey,
    state: State<'_, SharedState>,
) -> Result<Option<PlayerProgress>, String> {
    let key = progress_cache_key(&request.site_key, &request.vod_id, &request.episode_url)?;
    let value = state.database.cache_get(&key)?;
    if value.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&value)
        .map(Some)
        .map_err(|error| format!("stored player progress is invalid: {error}"))
}

#[tauri::command]
pub fn player_progress_set(
    request: PlayerProgressUpdate,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    if !request.position.is_finite()
        || !request.duration.is_finite()
        || request.position < 0.0
        || request.duration <= 0.0
    {
        return Err(
            "player progress must contain finite non-negative position and positive duration"
                .to_string(),
        );
    }
    let key = progress_cache_key(&request.site_key, &request.vod_id, &request.episode_url)?;
    let value = serde_json::to_string(&PlayerProgress {
        position: request.position.min(request.duration),
        duration: request.duration,
    })
    .map_err(|error| format!("unable to encode player progress: {error}"))?;
    state.database.cache_set(&key, &value)
}

fn progress_cache_key(site_key: &str, vod_id: &str, episode_url: &str) -> Result<String, String> {
    let site_key = site_key.trim();
    let vod_id = vod_id.trim();
    let episode_url = episode_url.trim();
    if site_key.is_empty() || vod_id.is_empty() || episode_url.is_empty() {
        return Err("player progress requires site_key, vod_id, and episode_url".to_string());
    }
    let identity = format!("{site_key}\0{vod_id}\0{episode_url}");
    Ok(format!(
        "player-progress:{}",
        URL_SAFE_NO_PAD.encode(identity.as_bytes())
    ))
}

#[tauri::command]
pub fn player_open(
    request: PlayerOpenRequest,
    state: State<'_, SharedState>,
) -> Result<PlayerStatus, String> {
    state.player.open(request)
}

#[tauri::command]
pub fn player_surface_attach(
    window: WebviewWindow,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        if state.player.surface().is_none() {
            state
                .player
                .set_surface(Some(create_player_surface(&window)?));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (window, state);
        Err("embedded playback is currently supported on Windows only".to_string())
    }
}

#[tauri::command]
pub fn player_surface_update(
    bounds: PlayerSurfaceBounds,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let surface = state
            .player
            .surface()
            .ok_or_else(|| "the embedded player surface is not attached".to_string())?;
        update_player_surface(surface, &bounds)
    }
    #[cfg(not(windows))]
    {
        let _ = (bounds, state);
        Err("embedded playback is currently supported on Windows only".to_string())
    }
}

#[tauri::command]
pub fn player_surface_detach(state: State<'_, SharedState>) -> Result<(), String> {
    state.player.close();
    let surface = state.player.surface();
    state.player.set_surface(None);
    #[cfg(windows)]
    if let Some(surface) = surface {
        destroy_player_surface(surface)?;
    }
    Ok(())
}

#[tauri::command]
pub fn player_control(
    request: PlayerControlRequest,
    state: State<'_, SharedState>,
) -> Result<PlayerStatus, String> {
    state.player.control(request)
}

#[tauri::command]
pub fn player_status(state: State<'_, SharedState>) -> Result<PlayerStatus, String> {
    state.player.status()
}

#[tauri::command]
pub fn player_close(state: State<'_, SharedState>) {
    state.player.close();
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn control_values_must_be_finite() {
        assert_eq!(finite_value(Some(12.0), "seek").unwrap(), 12.0);
        assert!(finite_value(Some(f64::NAN), "seek").is_err());
        assert!(finite_value(None, "volume").is_err());
    }

    #[test]
    fn legacy_pvideo_urls_are_unwrapped_with_cloud_headers() {
        let mut headers = BTreeMap::from([
            ("User-Agent".to_string(), "site-agent".to_string()),
            ("X-Site".to_string(), "keep".to_string()),
        ]);
        let request = PlayerOpenRequest {
            url: concat!(
                "http://127.0.0.1:1314/?url=",
                "https%3A%2F%2Fdl-pc-zb.drive.quark.cn%2Ffile%3Ftoken%3Dabc%252Fdef",
                "&header=%7B%22Cookie%22%3A%22__puus%3Dsecret%252Fvalue%22%2C",
                "%22Referer%22%3A%22https%3A%2F%2Fpan.quark.cn%2F%22%2C",
                "%22user-agent%22%3A%22quark-agent%22%7D&thread=16"
            )
            .to_string(),
            title: "Quark test".to_string(),
            headers: std::mem::take(&mut headers),
            start: None,
        };

        let resolved = unwrap_legacy_pvideo_request(request).unwrap();

        assert_eq!(
            resolved.url,
            "https://dl-pc-zb.drive.quark.cn/file?token=abc%2Fdef"
        );
        assert_eq!(
            resolved.headers.get("Cookie").map(String::as_str),
            Some("__puus=secret%2Fvalue")
        );
        assert_eq!(
            resolved.headers.get("Referer").map(String::as_str),
            Some("https://pan.quark.cn/")
        );
        assert_eq!(
            resolved.headers.get("user-agent").map(String::as_str),
            Some("quark-agent")
        );
        assert_eq!(
            resolved.headers.get("X-Site").map(String::as_str),
            Some("keep")
        );
        assert!(!resolved.headers.contains_key("User-Agent"));
    }

    #[test]
    fn invalid_legacy_pvideo_urls_are_rejected_before_player_start() {
        let error = unwrap_legacy_pvideo_request(PlayerOpenRequest {
            url: "http://localhost:1314/?thread=16".to_string(),
            ..Default::default()
        })
        .unwrap_err();
        assert!(error.contains("upstream media URL"));

        let ordinary = PlayerOpenRequest {
            url: "http://127.0.0.1:9978/proxy?siteKey=drive".to_string(),
            ..Default::default()
        };
        assert_eq!(
            unwrap_legacy_pvideo_request(ordinary.clone()).unwrap().url,
            ordinary.url
        );
    }

    #[test]
    fn control_without_a_player_is_a_no_op() {
        let manager = PlayerManager::default();
        let status = manager
            .control(PlayerControlRequest {
                command: "pause".to_string(),
                value: None,
            })
            .unwrap();

        assert!(!status.ready);
        assert!(manager.handle.lock().unwrap().is_none());
    }

    #[test]
    fn progress_cache_keys_are_stable_and_reject_empty_identity() {
        let first = progress_cache_key("site", "vod", "episode-1").unwrap();
        assert_eq!(
            first,
            progress_cache_key("site", "vod", "episode-1").unwrap()
        );
        assert_ne!(
            first,
            progress_cache_key("site", "vod", "episode-2").unwrap()
        );
        assert!(progress_cache_key("", "vod", "episode-1").is_err());
    }

    #[test]
    fn parser_page_media_url_is_extracted_from_script_source() {
        let html = r#"<script>var videoObject = { video: 'https://media.example.test/trailer.mp4' };</script>"#;
        let url = extract_media_url("https://page.example.test/show/1", html).unwrap();

        assert_eq!(url, "https://media.example.test/trailer.mp4");
        assert_eq!(media_kind(&url), "file");
    }

    #[test]
    fn parser_page_rejects_html_without_media_candidate() {
        let result =
            extract_media_url("https://page.example.test/show/1", "<html>no player</html>");

        assert!(result.is_err());
    }

    #[test]
    fn browser_probe_accepts_nested_webview_callback_json() {
        let value = serde_json::json!(["https://media.example.test/stream"]);
        let callback = serde_json::to_string(&value.to_string()).unwrap();
        assert_eq!(
            browser_probe_urls(&callback),
            vec!["https://media.example.test/stream"]
        );
    }

    #[test]
    fn web_parser_fallback_uses_configured_web_endpoint() {
        let parser = Parse {
            name: "Web".to_string(),
            parse_type: 0,
            url: "https://parser.example/?url=".to_string(),
            ..serde_json::from_value(serde_json::json!({})).unwrap()
        };
        let request = PlayerResolveRequest {
            url: "https://page.example.test/show/1".to_string(),
            parse: 1,
            ..Default::default()
        };
        let fallback = web_fallback_request(&request, &[parser], "Web").unwrap();
        assert_eq!(
            fallback.url,
            "https://parser.example/?url=https://page.example.test/show/1"
        );
    }

    #[test]
    fn json_parser_fields_and_named_parser_urls_are_supported() {
        let value = serde_json::json!({
            "data": {
                "url": "https://media.example.test/movie.m3u8",
                "headers": {"User-Agent": "desktop-test"}
            }
        });
        let json_headers = json_headers(&value);
        let headers = merge_header_value(&BTreeMap::new(), &json_headers);

        assert_eq!(
            json_url(&value).as_deref(),
            Some("https://media.example.test/movie.m3u8")
        );
        assert_eq!(
            headers.get("User-Agent").map(String::as_str),
            Some("desktop-test")
        );
        assert_eq!(
            parser_target_url("https://parser.example/?url=", "https://page.example/show"),
            "https://parser.example/?url=https://page.example/show"
        );
        assert_eq!(
            temporary_json_parser_url("json:https://parser.example/?url="),
            Some("https://parser.example/?url=")
        );
        assert_eq!(
            parser_name(
                &PlayerResolveRequest {
                    play_url: "json:https://parser.example/?url=".to_string(),
                    parse: 1,
                    ..Default::default()
                },
                "configured",
            ),
            Some("configured".to_string())
        );
    }

    #[test]
    fn jar_parser_maps_include_upstream_extension_shape() {
        let parsers = vec![
            Parse {
                name: "JSON".to_string(),
                parse_type: 1,
                url: "https://parser.example/?url=".to_string(),
                ext: serde_json::json!({"flag":["demo"]}),
                ..serde_json::from_value(serde_json::json!({})).unwrap()
            },
            Parse {
                name: "JAR".to_string(),
                parse_type: 2,
                url: "Demo".to_string(),
                ..serde_json::from_value(serde_json::json!({})).unwrap()
            },
        ];
        let json_map = json_parser_map(&parsers);
        assert!(json_map["JSON"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://parser.example/?cat_ext=")));
        let mix_map = mix_parser_map(&parsers);
        assert_eq!(mix_map["JAR"]["type"].as_str(), Some("2"));
        assert_eq!(mix_map["JAR"]["url"].as_str(), Some("Demo"));
        assert!(parser_flags(&parsers[0]).contains(&"demo".to_string()));
    }

    #[test]
    fn json_parser_resolves_nested_media_url_and_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                if request.starts_with("HEAD /media.m3u8") {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: 0\r\n\r\n",
                        )
                        .unwrap();
                } else {
                    let body = format!(
                        r#"{{"data":{{"url":"http://127.0.0.1:{port}/media.m3u8","headers":{{"User-Agent":"parser-test"}}}}}}"#
                    );
                    stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        )
                        .unwrap();
                }
            }
        });
        let client = reqwest::Client::builder().build().unwrap();
        let parser = Parse {
            name: "json-parser".to_string(),
            parse_type: 1,
            url: format!("http://127.0.0.1:{port}/parse?url="),
            ..serde_json::from_value(serde_json::json!({})).unwrap()
        };
        let result = tauri::async_runtime::block_on(resolve_with_configuration(
            &client,
            PlayerResolveRequest {
                url: "https://page.example/show/1".to_string(),
                parse: 1,
                ..Default::default()
            },
            &[parser],
            "json-parser",
        ))
        .unwrap();
        server.join().unwrap();

        assert_eq!(result.kind, "hls");
        assert_eq!(
            result.headers.get("User-Agent").map(String::as_str),
            Some("parser-test")
        );
    }

    #[test]
    fn configured_mpv_library_reports_api_version() {
        let Ok(path) = std::env::var("WEBHTV_MPV_DLL") else {
            return;
        };
        let api = MpvApi::load(Path::new(&path)).unwrap();
        let version = unsafe { (api.client_api_version)() };
        assert!(version >> 16 >= 2);
    }

    #[test]
    fn configured_mpv_engine_initializes_and_stops() {
        let Ok(path) = std::env::var("WEBHTV_MPV_DLL") else {
            return;
        };
        let engine = PlayerEngine::new(PathBuf::from(path)).unwrap();
        let status = engine.status().unwrap();
        assert!(status.ready);
        assert!(!status.api_version.is_empty());
    }

    #[test]
    fn configured_mpv_engine_loads_a_lavfi_test_source() {
        let Ok(path) = std::env::var("WEBHTV_MPV_DLL") else {
            return;
        };
        let mut engine = PlayerEngine::new(PathBuf::from(path)).unwrap();
        let status = engine
            .open(PlayerOpenRequest {
                url: "av://lavfi:testsrc2=size=320x240:rate=30".to_string(),
                title: "WebHomeTV libmpv smoke test".to_string(),
                headers: BTreeMap::new(),
                start: None,
            })
            .unwrap();
        assert!(status.ready);
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(engine.status().is_ok());
    }

    #[test]
    fn configured_player_manager_controls_a_running_engine() {
        if std::env::var("WEBHTV_MPV_DLL").is_err() {
            return;
        }
        std::env::set_var("WEBHTV_MPV_EXE", "C:\\webhtv\\does-not-exist\\mpv.exe");
        let manager = PlayerManager::default();
        let opened = manager
            .open(PlayerOpenRequest {
                url: "av://lavfi:testsrc2=size=320x240:rate=30".to_string(),
                title: "WebHomeTV player manager test".to_string(),
                headers: BTreeMap::new(),
                start: None,
            })
            .unwrap();
        assert!(opened.ready);
        let paused = manager
            .control(PlayerControlRequest {
                command: "pause".to_string(),
                value: None,
            })
            .unwrap();
        assert!(paused.paused);
        let sped = manager
            .control(PlayerControlRequest {
                command: "speed".to_string(),
                value: Some(1.5),
            })
            .unwrap();
        assert!((sped.speed - 1.5).abs() < 0.05);
        manager.close();
        assert!(!manager.status().unwrap().ready);
    }

    #[test]
    fn stale_player_worker_is_reported_as_not_running() {
        if std::env::var("WEBHTV_MPV_DLL").is_err() {
            return;
        }
        std::env::set_var("WEBHTV_MPV_EXE", "C:\\webhtv\\does-not-exist\\mpv.exe");
        let manager = PlayerManager::default();
        manager
            .open(PlayerOpenRequest {
                url: "av://lavfi:testsrc2=size=320x240:rate=30".to_string(),
                title: "WebHomeTV stale worker test".to_string(),
                headers: BTreeMap::new(),
                start: None,
            })
            .unwrap();
        let sender = manager
            .handle
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .sender
            .clone();
        sender.send(PlayerCommand::Shutdown).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(!manager.status().unwrap().ready);
    }
}
