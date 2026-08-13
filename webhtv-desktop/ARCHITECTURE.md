# WebHomeTV Desktop Architecture

> Reviewed: 2026-07-24  
> Upstream baseline: `Silent1566/webhtv@7a717d03`  
> Target: Windows 10/11, x64, Tauri 2, React, TypeScript, Rust

## Goals

The desktop client reimplements platform services while preserving WebHomeTV's
cross-platform contracts:

- CatVod Spider method names and JSON response shapes.
- Vod configuration, Site, Vod, Parse, and related JSON fields.
- The `fongmiBridge` / `fongmiNative` callback protocol and injected `fm` SDK.
- Local HTTP routes used by management pages, playback status, resource proxying,
  sync, and remote control.
- Existing WebHome HTML, CSS, JavaScript, templates, and extension manifests.

Android Activities, Services, Room, ExoPlayer, Android native libraries, and
mobile/leanback layouts are references, not code to compile on Windows.

## System Boundaries

```text
React application shell (local, privileged)
  -> Tauri invoke/events
Rust core
  -> configuration and SQLite
  -> Spider runtime adapters (QuickJS / Python / JVM)
  -> local axum HTTP server
  -> player process/window

Remote WebHome webview (untrusted)
  -> narrow fongmiBridge only
  -> no shell, filesystem, updater, or unrestricted Tauri API
```

The local application shell and remote WebHome content must not share an
unrestricted Tauri capability. Remote WebHome permissions are designed in its
own phase after the local bridge contract is tested.

## Project Layout

```text
webhtv-desktop/
  src/                         React application shell
    config-api.ts              Typed configuration commands
    native.ts                  Typed direct Tauri commands
    spider-api.ts              Typed Spider invocation command
    shims/fongmi-bridge.ts     Android-compatible bridge adapter
  src-tauri/
    src/bridge.rs              IPC commands and bridge dispatch
    src/config.rs              Vod configuration models and loading
    src/config_commands.rs     Configuration IPC commands
    src/database.rs            SQLite schema and repositories
    src/library_commands.rs    Keep and history IPC commands
    src/live.rs                Live source loading and parsing
    src/player.rs              Dynamic libmpv worker and native window
    src/spider.rs              Runtime selection and common dispatch
    src/spider_commands.rs     Local Spider IPC command
    src/spider_quickjs.rs      QuickJS worker and host functions
    src/spider_java.rs         Java DEX sidecar worker
    src/state.rs               Shared core state
  java-sidecar/                JVM process for Android DEX Spider packages
  scripts/upstream_sync.py     Upstream contract and asset monitor
  shared/                      Auto-synced upstream web assets (generated)
```

Future Rust modules are added when their owning phase starts:

```text
src-tauri/src/
  spider/      common trait and runtime adapters
  server/      LAN/local HTTP routes
  player/      player process, state, and native window integration
  webhome/     isolated webview and extension registry
```

## Verified Upstream Contracts

### Spider

Source: `catvod/src/main/java/com/github/catvod/crawler/Spider.java`

```text
init(extend)
destroy()
homeContent(filter)
homeVideoContent()
categoryContent(tid, page, filter, extend)
detailContent(ids)
searchContent(key, quick[, page])
playerContent(flag, id, vipFlags)
liveContent(url)
manualVideoCheck()
isVideoFormat(url)
proxy(params)
action(action)
```

Content methods return JSON strings. The Windows adapters normalize each
runtime's output to the same JSON contract before passing it to the core.

Runtime selection follows upstream `BaseLoader` and is based on `Site.api`:

- contains `.py`: Python runtime
- contains `.js`: QuickJS runtime
- starts with `csp_`: Java JAR runtime
- otherwise: null/protocol implementation

`homePage` is an additional WebHome capability. It must not be inferred from
`Site.type` alone.

Only type-3 sites enter Spider dispatch. Phase 3A provides QuickJS. Phase 3B
provides a Java DEX sidecar for `csp_*` sites. Python still returns an explicit
not-implemented error until its adapter is available.

### WebHome Bridge

Sources:

- `app/src/main/java/com/fongmi/android/tv/web/HomeWebBridge.java`
- `app/src/main/java/com/fongmi/android/tv/web/HomeWebController.java`

Android exposes synchronous host methods but completes normal invocations with
callbacks:

```text
fongmiBridge.invoke(requestId, method, JSON.stringify(payload)) -> void
window.fongmiNative.resolve(requestId, data)
window.fongmiNative.reject(requestId, error)
```

The Tauri shim preserves this behavior over Tauri's Promise-based `invoke` API.
`fongmiBridge.resourceUrl()` remains synchronous by using a local-server base URL
cached during bootstrap. Large Tauri IPC values do not need Android WebView's
12 KB workaround; `resultLength`, `resultChunk`, and `clearResult` remain present
for SDK compatibility.

Upstream bridge methods currently include:

```text
net.request                 net.resourceUrl
player.playUrl              player.playVod
player.playVodInline        player.preloadArtwork
player.control              player.status
app.search                  app.openVod
app.openLive                app.openKeep
app.openSetting             app.history
pan.check                   pan.play
cache.get                   cache.set
cache.del                   device.info
site.info                   config.info
ext.info                    ext.log
ext.toast                   ui.setToolbar
ui.setChrome                ui.restoreChrome
ui.getViewport              navigation.back
navigation.reload
```

Phase 2 implements the bridge transport plus `device.info`, SQLite-backed
`config.info`, `site.info`, and `cache.*`. Player URL/control/status and
database-backed history are now implemented by their owning subsystems;
`ext.info` remains a placeholder.

### JSON Naming

- `Vod` uses CatVod snake_case fields such as `vod_id` and `vod_name`.
- `Site` primarily uses camelCase fields such as `playUrl`, `homePage`,
  `chromeMode`, `webHomeChrome`, and `quickSearch`.
- `homePage` also accepts upstream aliases `home_page`, `webHome`, and `web_home`.
- Root configuration uses fields such as `spider`, `sites`, `parses`, `hlsRules`,
  and `webHomeExtensions`.

Do not apply one global Serde rename policy to all models.

## Phase 1 Baseline

Phase 1 intentionally has a small dependency surface.

Rust:

```toml
[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

Frontend:

```text
React, React DOM, TypeScript, Vite, @tauri-apps/api, lucide-react
Vitest and jsdom for the bridge shim tests
```

Phase 1 acceptance criteria:

- Tauri application compiles and launches on Windows.
- React shell receives typed bootstrap state from Rust.
- Android-style Bridge callbacks resolve and reject correctly.
- `resourceUrl()` is synchronous.
- Rust and TypeScript bridge tests pass.
- Remote content has no Tauri capability yet.

## Phase 2 Configuration

Phase 2 adds compatible configuration parsing and persistent application state:

- Direct JSON, local-file package, and HTTP URL imports, including depot URL
  selection. Local relative resources resolve from the selected JSON file.
- Configuration HTTP requests use the upstream OkHttp user agent and resolve
  relative resources from the final response URL after redirects.
- Upstream field aliases, mixed JSON naming, relative resource URL resolution,
  site de-duplication, and global Spider JAR inheritance.
- SQLite schema v1 for saved configurations and WebHome cache values.
- Persistent active configuration and selected home site.
- React workflows for import, activation, deletion, and home-site selection.

Acceptance is covered by parser fixtures, database lifecycle tests, a real
file-backed database reopen test, and a Windows application restart check.

## Phase 3A QuickJS

QuickJS runs in one bounded worker thread per `(configuration, site)` identity.
Calls for one site are serialized while different sites can run independently.
Configuration import, activation, and deletion invalidate workers; changing the
home-site pointer does not discard initialized Spider state.

The current adapter supports:

- HTTP(S) ES modules and relative imports with a 5 MiB per-module limit.
- Existing `__JS_SPIDER__`, default object/factory, and `__jsEvalReturn` exports.
- Upstream method mapping, Promise completion, object/string extension semantics,
  and common JSON/text/boolean result normalization.
- Synchronous and Promise-style `req`/`http`, GET/HEAD/POST JSON or form bodies,
  redirects, headers, response buffer modes, and a 16 MiB response limit.
- A 32 MiB runtime memory limit, 512 KiB stack limit, and per-call interrupt
  deadline derived from the Site timeout.

QuickJS executes remote code in-process and is not a security boundary. It has
no filesystem or shell host API, and `spider_invoke` remains available only to
the privileged local shell. A sidecar process with Windows restrictions remains
the isolation option if hostile scripts must be supported.

Phase 3A does not yet provide the complete `cat.js`/Cheerio/parser/crypto helper
surface, timers, persistent `local.*`, Spider proxy responses, optional JAR-added
JavaScript functions, or Python. Those capabilities must be implemented and
tested before claiming full upstream Spider compatibility.

## Phase 3B Java DEX

Java Spiders (`Site.api` starts with `csp_`) run in an out-of-process JVM sidecar:

- The original package is Android DEX (often a signed jar that only contains
  `classes.dex`). The sidecar converts it with dex2jar and loads the result with
  a parent-first classloader.
- Host shims supply `com.github.catvod.crawler.Spider`, a desktop
  `com.github.catvod.spider.Init`, and minimal Android types such as `Context`,
  `Application`, `TextUtils`, and `Environment`.
- Runtime libraries include OkHttp, Gson, and org.json. Full Android UI, WebView,
  floating-ball, and Android Go proxy launch paths are intentionally stubbed or
  skipped. Quark, UC, and Baidu QR login are host-managed sidecar sessions rather
  than emulated Android dialogs.
- IPC is one JSON object per line over stdin/stdout. Rust keeps one worker process
  per active configuration, and all Java sites in that configuration reuse its
  JVM, converted JAR, and Spider instances. QuickJS remains isolated per site.
- Release builds bundle a pinned Java 17 runtime and sidecar jar. Development can
  override discovery with `WEBHTV_JAVA`, `JAVA_HOME`, `PATH`, or
  `WEBHTV_JAVA_SIDECAR`.
- Java workers share a per-configuration storage directory so the config-center
  Spider and cloud-drive Spiders see the same legacy `config.json`, current
  `配置.json`, and `external-storage/TVBox` login files.
- Provider credentials never cross into React. The shell receives only the
  short-lived QR payload/image and session state; successful credentials are
  validated and written by the loaded Spider JAR inside the Java sidecar.
- The sidecar exposes a loopback `/proxy` endpoint on ports 9978-9999. This is
  required by cloud-drive Spiders whose `playerContent` result is a local proxy
  URL rather than a public media URL.
- The package's `127.0.0.1:1314` `pvideo` downloads are Android/Linux executables
  and have no Windows build. Before playback, the desktop player unwraps these
  legacy URLs and applies their signed upstream URL and encoded request headers
  directly to libmpv. `proxyMode` remains pinned to `Java多线程` for package
  compatibility until a protocol-compatible native Windows proxy is available.

The compatibility tests use the local 柒豪 package `csp_Douban.homeContent`
end-to-end, and the desktop adapter also handles Wogg endpoint selection.
Sites that depend on deep Android UI, broken dex2jar classes, proprietary native
code, or unimplemented host APIs can still fail and surface explicit errors.

## State Ownership

Tauri manages an `Arc<CoreState>` because the same core state will later be
shared with axum. Locks are split by subsystem. A synchronous standard-library
lock is used for short in-memory access; async locks are introduced only for
resources whose guards must cross `await` points.

## Player Architecture

The current implementation dynamically loads `libmpv-2.dll`, creates a dedicated
native child HWND owned by the main Tauri window, and passes that child handle to
libmpv through `wid`. It never passes the WebView2 HWND itself. React reports the
video region in physical pixels, while Rust updates child position, visibility,
resize, and teardown. The controller exposes load, pause/resume, stop, relative
seek, volume, fullscreen, close, and status commands.

`scripts/fetch_mpv.ps1` pins the official shinchiro build
`mpv-dev-x86_64-20251012-git-ad59ff1`, validates the archive and DLL SHA-256,
and stages `libmpv-2.dll` into the Tauri resource directory. During development,
`WEBHTV_MPV_DLL` overrides the bundled path.

`player_resolve` handles `parse=1` HTML pages by extracting and checking mp4,
m3u8, and mpd candidates before libmpv receives them. It also consumes the
active config's type-0 web and type-1 JSON parsers, including nested
`data.url`/header responses and `parse:`/`json:` selectors. Type-2 and type-3
JAR parsers are dispatched through the active Java Spider worker, while type-4
tries matching type-0/type-1 parsers before the direct page. Failed web parsing
falls back to a hidden WebView2 that observes media elements, fetch/XHR
responses, and resource timing entries with a bounded timeout.

The child-HWND path has automated libmpv load and control coverage. Release
acceptance still needs repeated open/close, DPI, fullscreen, subtitles, hardware
decode, z-order, and OS-level screenshot checks on representative Windows hosts.

## Upstream Maintenance

`scripts/upstream_sync.py` monitors only contracts and reusable assets. It does
not automatically accept a changed snapshot.

```powershell
npm run upstream:init
npm run upstream:check
npm run upstream:sync-assets
python scripts/upstream_sync.py --accept
```

`--check` never pulls or edits the Android repository. Use explicit `--pull`
when a fast-forward update is intended. Protocol changes are reviewed and ported
before `--accept` updates the snapshot.

Change classes:

- `CRITICAL`: Spider, Bridge SDK/dispatch, Site/VodConfig, protocol docs.
- `WARNING`: local server handlers, WebCall, extension registry, player API.
- `ASSET`: HTML/CSS/JS, devkit examples/templates/docs, LUT presets.
- Android UI/services/native assets are not monitored by the desktop port.

## Delivery Phases

1. Tauri shell and local Bridge. Complete.
2. Configuration models, fixtures, and SQLite persistence. Complete.
3. Spider runtime compatibility: QuickJS core complete; Java DEX sidecar proof
     complete for load/init/content, including the common Context package and
     SharedPreferences helpers; broader helper parity and Python remain.
4. libmpv standalone native-window PoC is implemented; child-HWND versus
    `mpv_render_context` comparison and playback hardening remain.
5. Local HTTP server and reusable management assets.
6. Isolated WebHome webview and extension system.
7. Content workflows: home/category/search/detail, SQLite-backed keep/history,
     live source/channel browsing, and native playback actions are implemented;
     parser chains are implemented through type-4 aggregation and the Java
     sidecar; resume workflows remain.
8. DLNA, sync, remote relay, diagnostics, packaging, and updater.
