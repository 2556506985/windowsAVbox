# 全项目 Bug 审计与整改清单

审计时间：2026-10-02。范围：Java sidecar / Rust 后端 / React 前端。
标注：[已修复] / [待修复] / [观察项]

---

## 一、Java Sidecar

| # | 级别 | 位置 | 问题 | 修复方案 | 状态 |
|---|------|------|------|----------|------|
| J1 | High | ProxyServer.java:116-125 | 可缓存响应超 512KB 时 `body.close();body=null` 后继续落入流式分支，必然 NPE，且响应头已发送导致客户端收到"有头无体"的截断响应 | 超限后改为"前缀+继续流式转发"，且必须 return | |
| J2 | High | Init.java:26 | 线程池默认工厂产生非守护线程，宿主崩溃/stdin EOF 后 JVM 无法退出，孤儿进程占用 9978 端口导致下次启动 BindException | 线程工厂 setDaemon(true) | |
| J3 | Medium | ProxyServer.java:180 | `cacheable()` 查 `params.get("range")`（小写）但 Headers 规范化键为 `"Range"`，该判断是死代码，Range 请求永远被判为可缓存 → 播放器 seek 语义被破坏 | 用 `exchange.getRequestHeaders().getFirst("Range")` 判断，cacheable 去掉该参数 | |
| J4 | Medium | ProxyServer.java:88 | `buildCacheKey(url,...)` 在 url 为 null 时 NPE（catvod 部分 proxy 路由无 url 参数），直接把这类路由打成 502 | url null 保护，且把 key 计算移入 cacheable 分支内 | |
| J5 | Medium | JarRuntime.java:194 | `invoke()` 未加 synchronized，与类内其他方法的单线程假设矛盾；同一 Spider 实例被并发调用（catvod Spider 有可变字段，无线程安全保证） | `invoke` 加 synchronized | |
| J6 | Medium | CloudAuthManager.java 两处 (pollQuark/pollUc) | `response.optJSONObject("data").optJSONObject("members")...` 链式调用在 data/members 缺失时 NPE，且 NPE 计入 failures 加速把登录会话置为 error | 分级判空（参照 membersToken 写法） | |
| J7 | Medium | CloudAuthManager baidu extractJsonString | 用字符串定界符解析 JSON：数值/布尔字段（如 `"code":0`）会被解析成下一个键名；`"code":0` 时 error 信息变成 `code=bduss` | 优先 JSONObject 解析，失败才回退启发式 | |
| J8 | Low | ProxyServer buildCacheKey | `cookie.hashCode()` 32 位碰撞可致跨账号串号 | 改为拼接原值 | |
| J9 | Medium | CloudAuthManager UC TV token 交换 | 明文 HTTP 传 access_token/refresh_token | 改 https，且不跟随降级跳转 | |
| J10 | Low | CloudAuthManager 日志 | 明文打印完整 BDUSS Cookie，且经 TeeStream 落盘 sidecar-stderr.log | 只打长度/有无标记 | |
| J11 | Low | CloudAuthManager credentialPath | `uc` 与 `uctv` 映射到同一 uc_cookie.txt，凭据互相覆盖、status 误报、clear 误删 | 分文件存储 | |
| J12 | Low | SidecarMain/JarRuntime rootMessage | cause 链无环路防护，循环 cause 时死循环占满 CPU | 加 `current.getCause() != current` 判断 | |
| J13 | Low | SidecarMain 日志 | requests.log 与 sidecar-stderr.log 无上限增长 | 超 8MB 滚动为 .1 备份 | |

## 二、Rust 后端

| # | 级别 | 位置 | 问题 | 修复方案 | 状态 |
|---|------|------|------|----------|------|
| R1 | High | spider.rs:277-286 | 任何 spider 级失败（含业务性 Err、auth 45s 超时）都 `remove_handle` 杀掉整个共享 JVM，auth 的"超时不杀 JVM"保护失效；一个站点业务错误连坐杀死整个 config 所有站点 JVM 及并发请求 | 区分错误类别：仅传输层/进程级错误才 remove_handle；auth 方法不杀 | |
| R2 | High | spider.rs:434-445 | `remove_handle` 仅按 identity 比较，竞态下会误杀并发刚重建的健康 JVM | 实例级判等（Java：Arc::ptr_eq(child)）后再 remove | |
| R3 | Medium | player.rs:1779-1784 | sniff_webview 过滤条件写反：已证明是非媒体的候选反而被返回给 mpv 播放；带 .m3u8 但 content-type 误标的反而被跳过 | `if !media_response && !is_media_url { continue }` | |
| R4 | Medium | spider_java.rs shutdown | `engine.shutdown()` 等 sidecar 应答最坏阻塞 worker 线程 120s（恰好常在 JVM 卡死时调用） | shutdown 请求不等待应答 + 立即 stop_child | |
| R5 | Medium | image_proxy.rs:31-35 | 每个图片请求裸 spawn 无线程数上限，海报墙/上游慢时线程爆炸 | 固定线程池 + 有界队列（满则 503） | |
| R6 | Medium | market.rs:249-277 | zip 解压无总大小上限 → zip bomb 磁盘耗尽 | 累计字节数超上限即报错并清理 | |
| R7 | Medium | bridge.rs:71-82 | `inline_results` 只写不读永远增长；功能上是死数据通道 | 消费端 take 语义取走 | |
| R8 | Medium | image_proxy.rs:23-41 | 绑定失败时不 manage State，后端 `State<ImageProxyPort>` 读取直接 panic | 绑定失败也 manage（port=None） | |
| R9 | Medium | player.rs:2001 | `player_open` 是同步 Tauri 命令在主线程执行，内部同步 curl -m 8 + 等 mpv 加载 → UI 冻结最长 8s+ | 改 async 命令 + spawn_blocking | |
| R10 | Low | player.rs 多处 | `recv()` 无超时，mpv worker 卡死则永久阻塞 | 统一 recv_timeout | |
| R11 | Low | spider_java.rs terminate | kill 后不 wait，非 Windows 累积僵尸进程 | kill 后紧跟 wait | |
| R12 | Low | spider_quickjs.rs invoke | `recv()` 无超时兜底，JS 层 deadline 不覆盖 native host 阻塞 | recv_timeout(site timeout + 30s) | |
| R13 | Low | lib.rs | 退出/app_restart 无子进程清理钩子 → java/mpv 孤儿累积 | RunEvent::Exit 里 invalidate_all + player.close | |
| R14 | Low | player.rs close_external | 裸 PID taskkill，PID 复用时会误杀无关进程 | 用进程句柄校验后再杀 | |
| R15 | Low | spider_quickjs.rs | 每次 JS HTTP 调用新建 reqwest blocking Client | OnceLock 共享 | |
| R16 | Low | bridge.rs cache_key | `cache_{rule}_{key}` 下划线拼接前缀歧义，互相覆盖 | 用 `\0` 分隔（与 progress_cache_key 一致） | |
| R17 | Low | player.rs sniff 轮询 | eval 失败时 continue 无 sleep，20s 内满速空转烧 CPU | continue 前 sleep 100ms | |

## 三、前端

| # | 级别 | 位置 | 问题 | 修复方案 | 状态 |
|---|------|------|------|----------|------|
| F1 | Medium | spider-api.ts + ErrorBoundary.tsx | `isNetworkError` 只认 Error 实例/Response/code 字段，但 Tauri invoke 的错误是**字符串**，重试形同虚设（所有调用永远不 retry） | isNetworkError 里加 string 判断（匹配 timeout/timed out/network/failed 等关键词） | |
| F2 | Medium | request-cache.ts SWR | `swr:true` 的缓存命中（内存或持久）每次都触发后台 refresh，refresh 完成后下一个调用又触发 → 高频调用场景下反复刷新源站 | refreshStale 前判断最近 refresh 间隔（如 60s 内不重复刷新） | |
| F3 | Low | request-cache.ts SWR | 内存缓存命中刷新后，UI 展示的还是旧值（用户无感知） | 观察项：当前 SWR 语义就是"先旧后新"，UI 无刷新通知机制。暂不改 | |
| F4 | Low | hooks/useClickHandler.ts | `[callback, delay...]` 依赖导致每次 render 重建 debouncedFn，且重建不进行 flush，pending 的 trailing 调用被静默丢弃 | useRef 存 callback 最新引用 | |
| F5 | Low | App.tsx PlayerView | useEffect deps `[session]` 且 cleanup 里 closePlayer — React StrictMode（dev）下会 open→close→open 闪烁；生产无影响 | 观察项，仅在 dev StrictMode | |

## 四、验证记录

- 前端 vitest：16/16 通过（修复前基线）
- Rust cargo test：56/56 通过（修复前基线）
