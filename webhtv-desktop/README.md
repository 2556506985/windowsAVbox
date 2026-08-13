# WebHomeTV Desktop

Windows 10/11 desktop port of WebHomeTV. The project preserves the upstream
Spider, configuration, WebHome Bridge, extension, and local HTTP contracts while
replacing Android platform services with Rust and Tauri implementations.

## Current Status

The current desktop core provides the Tauri shell, Android-compatible Bridge
transport, compatible Vod configuration import, SQLite persistence, constrained
QuickJS workers, and an out-of-process Java DEX sidecar. The React shell can
browse home/category/search/detail results, persist keep/history records, and
load live sources from M3U, TVBox TXT, embedded JSON, or Spider `liveContent`.
Episode and live-channel actions use a dynamically loaded libmpv runtime in an
embedded native child surface inside the main application window. Full CatVod helper parity, Python, online
parse/proxy chains, local HTTP routes, and the isolated WebHome extension view
remain later work.

The Java configuration center supports desktop QR-code login for Quark, UC, and
Baidu. Login tokens are polled in the sidecar and the resulting credentials are
stored only in the per-configuration `external-storage/TVBox` directory expected
by the Spider package. The sidecar keeps Spider `/proxy` routes on loopback. The
official Android `pvideo` Go proxy has no Windows binary, so the native player
unwraps its legacy loopback URL and sends the signed upstream URL and headers
directly through libmpv.

## Prerequisites

- Node.js 20 or newer
- Rust stable MSVC toolchain
- Visual Studio 2022 Build Tools with Desktop development with C++
- Microsoft Edge WebView2 Runtime
- Java 17 or newer for building the Java sidecar; release builds fetch and bundle
  the pinned Temurin runtime used by Java DEX Spider packages
- Maven sidecar builds use `D:\MR\App\Maven` with local repository
  `D:\MR\Data\Maven_repository`
- libmpv for native playback: run `npm run mpv:fetch`, or set `WEBHTV_MPV_DLL`
  during development

## Commands

```powershell
npm install
npm run build
npm test
npm run rust:test
npm run mpv:fetch
npm run java:build
npm run tauri dev
npm run tauri -- build
```

Build the Java sidecar explicitly when its sources change:

```powershell
$env:JAVA_HOME = "D:\MR\App\IntelliJ IDEA 2025.1\jbr"
& "D:\MR\App\Maven\bin\mvn.cmd" -Dmaven.repo.local="D:\MR\Data\Maven_repository" -DskipTests package
```

The release installer is written to
`src-tauri/target/release/bundle/nsis/`. Run `npm run mpv:fetch` before bundling
so the installer includes the pinned Windows x64 libmpv runtime.

Upstream monitoring:

```powershell
npm run upstream:init
npm run upstream:check
npm run upstream:sync-assets
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for contract boundaries and phase gates.
