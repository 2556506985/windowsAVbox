import { useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import QRCode from "qrcode";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  Clock3,
  Database,
  Download,
  FileJson,
  Film,
  FolderOpen,
  Globe2,
  Heart,
  Home,
  Library,
  ListVideo,
  Maximize2,
  Pause,
  Play,
  QrCode,
  Radio,
  RefreshCw,
  RotateCcw,
  Search,
  Settings,
  Square,
  Trash2,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  activateConfig,
  deleteConfig,
  getActiveConfig,
  importConfigFile,
  importConfigJson,
  listConfigs,
  loadConfigUrl,
  selectHomeSite,
  type ConfigDetail,
  type ConfigSummary,
  type SiteConfig,
} from "./config-api";
import { getBootstrapState } from "./native";
import {
  addHistory,
  addKeep,
  clearHistory,
  isKept,
  listHistory,
  listKeeps,
  removeHistory,
  removeKeep,
  type LibraryItem,
} from "./library-api";
import {
  listLiveSources,
  loadLiveSource,
  type LiveCatalog,
  type LiveChannel,
  type LiveSourceSummary,
} from "./live-api";
import {
  closePlayer,
  controlPlayer,
  getPlayerProgress,
  openPlayer,
  playerStatus,
  resolvePlayer,
  type PlayerOpenRequest,
  type PlayerStatus,
} from "./player-api";
import {
  classifyConfigAction,
  movePanOrder,
  normalizePanOrder,
  parsePanBlock,
  togglePanBlock,
  type CloudAuthProvider,
} from "./config-center-contract";
import {
  appRestart,
  marketCatalog,
  marketInstall,
  type MarketCategory,
  type MarketInstallResult,
  type MarketItem,
} from "./market-api";
import { invokeSpider } from "./spider-api";
import { cachedInvoke, isCached } from "./request-cache";
import { Grid } from "react-window";
import { useClickHandler } from "./hooks/useClickHandler";
import { SkeletonDetail, SkeletonGrid } from "./components/Skeleton";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./App.css";

type ViewKey = "home" | "search" | "live" | "keep" | "history" | "settings";
type ConnectionState = "loading" | "ready" | "error";
type CatalogState = "idle" | "loading" | "ready" | "error";

interface VodItem {
  action?: string;
  vod_id?: string;
  vod_name?: string;
  vod_pic?: string;
  vod_remarks?: string;
}

interface ClassItem {
  type_id?: string;
  type_name?: string;
}

interface CatalogPage {
  list: VodItem[];
  class: ClassItem[];
  page?: number;
  pagecount?: number;
  total?: number;
}

interface DetailItem extends VodItem {
  type_name?: string;
  vod_year?: string;
  vod_area?: string;
  vod_actor?: string;
  vod_director?: string;
  vod_content?: string;
  vod_play_from?: string;
  vod_play_url?: string;
}

interface PlayerContentResponse {
  url?: unknown;
  parse?: number | string;
  jx?: number | string;
  playUrl?: string;
  flag?: string;
  header?: unknown;
  headers?: unknown;
  click?: string;
  msg?: string;
}

interface CloudAuthResponse {
  account?: string;
  expiresAt?: number;
  message?: string;
  provider?: CloudAuthProvider;
  qrImage?: string;
  qrText?: string;
  sessionId?: string;
  state?: "pending" | "success" | "expired" | "error" | "cancelled" | "cleared" | "authenticated" | "anonymous";
}

interface CloudAuthSession {
  message: string;
  provider: CloudAuthProvider;
  qrImage: string;
  sessionId: string;
  state: "pending" | "success" | "expired" | "error";
}

interface PlaybackSession {
  request: PlayerOpenRequest;
  siteKey?: string;
  vodId?: string;
  episodeUrl?: string;
  opened?: PlayerStatus;
}

interface NavigationItem {
  key: ViewKey;
  label: string;
  icon: LucideIcon;
}

const navigation: NavigationItem[] = [
  { key: "home", label: "首页", icon: Home },
  { key: "search", label: "搜索", icon: Search },
  { key: "live", label: "直播", icon: Radio },
  { key: "keep", label: "收藏", icon: Heart },
  { key: "history", label: "历史", icon: Clock3 },
  { key: "settings", label: "设置", icon: Settings },
];

const viewTitles: Record<ViewKey, string> = {
  home: "首页",
  search: "搜索",
  live: "直播",
  keep: "收藏",
  history: "观看历史",
  settings: "设置",
};

const cloudAuthLabels: Record<CloudAuthProvider, string> = {
  quark: "夸克",
  uc: "UC",
  uctv: "UC TV",
  baidu: "百度",
};

function statusLabel(state: ConnectionState) {
  if (state === "ready") return "已连接";
  if (state === "error") return "连接失败";
  return "连接中";
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function App() {
  const [activeView, setActiveView] = useState<ViewKey>("home");
  const [connection, setConnection] = useState<ConnectionState>("loading");
  const [configs, setConfigs] = useState<ConfigSummary[]>([]);
  const [activeConfig, setActiveConfig] = useState<ConfigDetail | null>(null);
  const [configError, setConfigError] = useState("");
  const [sourceDialogOpen, setSourceDialogOpen] = useState(false);
  const [configCenterOpen, setConfigCenterOpen] = useState(false);
  const [playback, setPlayback] = useState<PlaybackSession | null>(null);
  const [notice, setNotice] = useState<{ kind: "info" | "error"; text: string } | null>(null);
  const noticeTimer = useRef(0);
  const [imageProxyReady, setImageProxyReady] = useState(false);
  void imageProxyReady;

  async function refreshCore() {
    setConnection("loading");
    try {
      await getBootstrapState();
      setConnection("ready");
    } catch {
      setConnection("error");
    }
  }

  async function refreshConfigs() {
    setConfigError("");
    try {
      const [nextConfigs, nextActive] = await Promise.all([listConfigs(), getActiveConfig()]);
      setConfigs(nextConfigs);
      setActiveConfig(nextActive);
    } catch (error) {
      setConfigError(errorText(error));
    }
  }

  useEffect(() => {
    void initImageProxy().then(() => setImageProxyReady(true));
    void refreshCore();
    void refreshConfigs();
  }, []);

  async function chooseHome(siteKey: string) {
    if (!activeConfig) return false;
    if (activeConfig.summary.homeKey === siteKey) return true;
    try {
      const next = await selectHomeSite(activeConfig.summary.id, siteKey);
      setActiveConfig(next);
      setConfigs((current) => current.map((item) => item.id === next.summary.id ? next.summary : item));
      return true;
    } catch (error) {
      setConfigError(errorText(error));
      return false;
    }
  }

  async function handleConfigReady() {
    await refreshConfigs();
    setActiveView("home");
  }

  async function refreshAll() {
    await Promise.all([refreshCore(), refreshConfigs()]);
  }

  function showNotice(kind: "info" | "error", text: string) {
    setNotice({ kind, text });
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 5000);
  }

  async function handlePlay(session: PlaybackSession) {
    try {
      const status = await openPlayer(session.request);
      if (status.external) {
        showNotice("info", `已在独立窗口播放${session.request.title ? `《${session.request.title}》` : ""}，详情页可继续浏览`);
        return;
      }
      setPlayback({ ...session, opened: status });
    } catch (nextError) {
      showNotice("error", `播放失败：${errorText(nextError)}`);
    }
  }

  const visibleSites = activeConfig?.document.sites.filter((site) => site.hide !== 1) || [];
  const configCenterSite = activeConfig?.document.sites.find((site) => site.api === "csp_Config") || null;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true"><Film size={21} strokeWidth={1.8} /></span>
          <span className="brand-copy"><strong>WebHomeTV</strong><small>Desktop</small></span>
        </div>

        <nav className="primary-nav" aria-label="主导航">
          {navigation.map((item) => {
            const Icon = item.icon;
            return (
              <button
                className={activeView === item.key ? "nav-item active" : "nav-item"}
                key={item.key}
                onClick={() => setActiveView(item.key)}
                type="button"
              >
                <Icon size={18} strokeWidth={1.8} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="sidebar-status">
          <span className={`status-dot ${connection}`} aria-hidden="true" />
          <span>本地核心</span>
          <strong>{statusLabel(connection)}</strong>
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <span className="section-kicker">WEBHOMETV DESKTOP</span>
            <h1>{viewTitles[activeView]}</h1>
          </div>
          <div className="topbar-actions">
            <button
              className="site-selector"
              type="button"
              title="选择内容源"
              aria-haspopup="dialog"
              aria-expanded={sourceDialogOpen}
              onClick={() => setSourceDialogOpen(true)}
            >
              <Library size={16} strokeWidth={1.8} />
              <span>{activeConfig?.homeSite?.name || "选择内容源"}</span>
              <ChevronDown size={15} strokeWidth={1.8} />
            </button>
            <button className="icon-button" type="button" title="刷新" aria-label="刷新" onClick={() => void refreshAll()}>
              <RefreshCw size={18} strokeWidth={1.8} />
            </button>
          </div>
        </header>

        <div className="view-content">
          {notice && (
            <div className={`app-toast ${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>
              {notice.kind === "error" ? <X size={15} strokeWidth={1.8} /> : <Play size={15} strokeWidth={1.8} />}
              <span>{notice.text}</span>
            </div>
          )}
          {activeView === "home" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>首页加载失败</p></div>}>
              <HomeView
              activeConfig={activeConfig}
              onPlay={handlePlay}
            />
            </ErrorBoundary>
          ) : activeView === "search" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>搜索页面加载失败</p></div>}>
              <SearchView activeConfig={activeConfig} onOpenSettings={() => setActiveView("settings")} onPlay={handlePlay} />
            </ErrorBoundary>
          ) : activeView === "live" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>直播页面加载失败</p></div>}>
              <LiveView activeConfig={activeConfig} onOpenSettings={() => setActiveView("settings")} onPlay={handlePlay} />
            </ErrorBoundary>
          ) : activeView === "keep" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>收藏页面加载失败</p></div>}>
              <LibraryListView
              emptyDetail="在详情页点击收藏后会出现在这里"
              emptyTitle="暂无收藏"
              kind="keep"
              activeConfig={activeConfig}
              onOpenSettings={() => setActiveView("settings")}
              onPlay={handlePlay}
            />
            </ErrorBoundary>
          ) : activeView === "history" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>历史页面加载失败</p></div>}>
              <LibraryListView
                emptyDetail="打开影片详情后会自动记录"
                emptyTitle="暂无观看历史"
                kind="history"
                activeConfig={activeConfig}
                onOpenSettings={() => setActiveView("settings")}
                onPlay={handlePlay}
              />
            </ErrorBoundary>
          ) : activeView === "settings" ? (
            <ErrorBoundary fallback={<div className="error-boundary"><p>设置页面加载失败</p></div>}>
              <SettingsView
                activeConfig={activeConfig}
                configs={configs}
                initialError={configError}
                onChanged={() => void handleConfigReady()}
                onRefresh={() => void refreshConfigs()}
              />
            </ErrorBoundary>
          ) : null}
        </div>
      </main>
      {sourceDialogOpen && (
        <SourceDialog
          activeKey={activeConfig?.summary.homeKey || ""}
          configCenterAvailable={Boolean(configCenterSite)}
          sites={visibleSites}
          onClose={() => setSourceDialogOpen(false)}
          onOpenConfigCenter={() => {
            setSourceDialogOpen(false);
            setConfigCenterOpen(true);
          }}
          onSelect={async (site) => {
            if (await chooseHome(site.key)) {
              setSourceDialogOpen(false);
              setActiveView("home");
            }
          }}
        />
      )}
      {configCenterOpen && (
        <ConfigCenterDialog
          activeConfig={activeConfig}
          site={configCenterSite}
          onClose={() => setConfigCenterOpen(false)}
          onOpenSettings={() => {
            setConfigCenterOpen(false);
            setActiveView("settings");
          }}
        />
      )}
      {playback && (
        <PlayerScreen
          session={playback}
          onClose={() => {
            setPlayback(null);
          }}
        />
      )}
    </div>
  );
}

interface HomeViewProps {
  activeConfig: ConfigDetail | null;
  onPlay: (session: PlaybackSession) => void;
}

const PREFETCH_DETAIL_LIMIT = 15;
const PREFETCH_DETAIL_GAP_MS = 1000;
const PREFETCH_START_CONCURRENCY = 3;
let prefetchPaused = false;

function setPrefetchPaused(value: boolean): void {
  prefetchPaused = value;
}

function usePrefetchDetails(
  list: VodItem[],
  siteKey: string | null,
  enabled: boolean
): void {
  useEffect(() => {
    if (!enabled || !siteKey) return;
    const targets = list.slice(0, PREFETCH_DETAIL_LIMIT).filter((item) => item.vod_id);
    if (targets.length === 0) return;
    let cancelled = false;
    let index = 0;
    const prefetchOne = (item: VodItem): void => {
      if (!item.vod_id || prefetchPaused) return;
      const args = { ids: [item.vod_id] };
      if (isCached("detailContent", args, siteKey, 60 * 60 * 1000)) return;
      void cachedInvoke<{ list?: DetailItem[] }>({
        method: "detailContent",
        args,
        siteKey,
        priority: "low",
        dedupe: true,
        cacheMs: 60 * 60 * 1000,
        persistMs: 24 * 60 * 60 * 1000,
      }).catch(() => {
        // 后台预取失败不打扰用户
      });
    };
    for (let i = 0; i < Math.min(PREFETCH_START_CONCURRENCY, targets.length); i++) {
      prefetchOne(targets[index++]);
    }
    const timer = window.setInterval(() => {
      if (cancelled || prefetchPaused) return;
      if (index >= targets.length) {
        window.clearInterval(timer);
        return;
      }
      prefetchOne(targets[index++]);
    }, PREFETCH_DETAIL_GAP_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [list, siteKey, enabled]);
}

function HomeView({ activeConfig, onPlay }: HomeViewProps) {  const homeSite = activeConfig?.homeSite || null;
  const [catalogState, setCatalogState] = useState<CatalogState>("idle");
  const [catalogError, setCatalogError] = useState("");
  const [classes, setClasses] = useState<ClassItem[]>([]);
  const [catalog, setCatalog] = useState<CatalogPage>({ list: [], class: [] });
  const [selectedClass, setSelectedClass] = useState<ClassItem | null>(null);
  const [page, setPage] = useState(1);
  const [detail, setDetail] = useState<DetailItem | null>(null);
  const [detailError, setDetailError] = useState("");
  const [detailLoading, setDetailLoading] = useState(false);

  usePrefetchDetails(catalog.list, homeSite?.key || null, homeSite?.type === 3);

  async function loadCatalog(options?: { classItem?: ClassItem | null; page?: number }) {
    if (!homeSite || homeSite.type !== 3) {
      setCatalog({ list: [], class: [] });
      setClasses([]);
      setCatalogState("idle");
      setCatalogError("");
      return;
    }
    const nextClass = options?.classItem === undefined ? selectedClass : options.classItem;
    const nextPage = options?.page ?? page;
    setCatalogState("loading");
    setCatalogError("");
    try {
      const result = await cachedInvoke<CatalogPage>({
        method: nextClass?.type_id ? "categoryContent" : "homeContent",
        args: nextClass?.type_id
          ? { tid: nextClass.type_id, page: String(nextPage), filter: true, extend: {} }
          : { filter: true },
        siteKey: homeSite.key,
        priority: "high",
        dedupe: true,
        cacheMs: 3 * 60 * 1000,
        persistMs: 60 * 60 * 1000,
        swr: true,
      });

      if (nextClass?.type_id) {
        setCatalog({
          list: Array.isArray(result?.list) ? result.list : [],
          class: classes,
          page: typeof result?.page === "number" ? result.page : nextPage,
          pagecount: typeof result?.pagecount === "number" ? result.pagecount : undefined,
          total: typeof result?.total === "number" ? result.total : undefined,
        });
      } else {
        const nextClasses = Array.isArray(result?.class) ? result.class : [];
        setClasses(nextClasses);
        setCatalog({
          list: Array.isArray(result?.list) ? result.list : [],
          class: nextClasses,
          page: 1,
          pagecount: 1,
          total: undefined,
        });
      }
      setCatalogState("ready");
    } catch (error) {
      setCatalog((current) => ({ ...current, list: [] }));
      setCatalogState("error");
      setCatalogError(errorText(error));
    }
  }

  useEffect(() => {
    setSelectedClass(null);
    setPage(1);
    setDetail(null);
    setDetailError("");
    setClasses([]);
    if (!homeSite || homeSite.type !== 3) {
      setCatalog({ list: [], class: [] });
      setCatalogState("idle");
      setCatalogError("");
      return;
    }
    let cancelled = false;
    setCatalogState("loading");
    void cachedInvoke<CatalogPage>({
      method: "homeContent",
      args: { filter: true },
      siteKey: homeSite.key,
      priority: "high",
      cacheMs: 3 * 60 * 1000,
      persistMs: 60 * 60 * 1000,
      swr: true,
    })
      .then((result) => {
        if (cancelled) return;
        const nextClasses = Array.isArray(result?.class) ? result.class : [];
        setClasses(nextClasses);
        setCatalog({
          list: Array.isArray(result?.list) ? result.list : [],
          class: nextClasses,
          page: 1,
          pagecount: 1,
        });
        setCatalogState("ready");
      })
      .catch((error) => {
        if (cancelled) return;
        setCatalog({ list: [], class: [] });
        setCatalogState("error");
        setCatalogError(errorText(error));
      });
    return () => {
      cancelled = true;
    };
  }, [activeConfig?.summary.id, homeSite?.key, homeSite?.api, homeSite?.type]);

  async function openDetail(item: VodItem) {
    if (!homeSite || !item.vod_id) return;
    setPrefetchPaused(true);
    setDetailLoading(true);
    setDetailError("");
    setDetail({ ...item, vod_play_from: "", vod_play_url: "" });
    try {
      const result = await cachedInvoke<{ list?: DetailItem[] }>({
        method: "detailContent",
        args: { ids: [item.vod_id] },
        siteKey: homeSite.key,
        priority: "high",
        dedupe: true,
        cacheMs: 60 * 60 * 1000,
        persistMs: 24 * 60 * 60 * 1000,
        swr: true,
      });
      const first = Array.isArray(result?.list) ? result.list[0] : null;
      const resolved = first || { ...item };
      setDetail(resolved);
      void addHistory({
        siteKey: homeSite.key,
        siteName: homeSite.name,
        vodId: resolved.vod_id || item.vod_id,
        vodName: resolved.vod_name || item.vod_name || "",
        vodPic: resolved.vod_pic || item.vod_pic || "",
        vodRemarks: resolved.vod_remarks || item.vod_remarks || "",
      }).catch(() => undefined);
    } catch (error) {
      setDetail(null);
      setDetailError(errorText(error));
    } finally {
      setDetailLoading(false);
    }
  }

  const pagecount = catalog.pagecount || 1;
  const canPrev = page > 1;
  const canNext = page < pagecount;

  const loadCatalogHandler = useClickHandler(
    (options?: { classItem?: ClassItem | null; page?: number }) => loadCatalog(options),
    { debounceMs: 100, priority: "high" }
  );

  const openDetailHandler = useClickHandler(
    (item: VodItem) => openDetail(item),
    { debounceMs: 100, priority: "high" }
  );

  const loadCatalogPageHandler = useClickHandler(
    (options: { page: number }) => loadCatalog(options),
    { debounceMs: 100, priority: "high" }
  );

  const selectHomeHandler = useClickHandler(
    (item: ClassItem | null) => {
      setSelectedClass(item);
      setPage(1);
      loadCatalog({ classItem: item, page: 1 });
    },
    { debounceMs: 100, priority: "high" }
  );

  if (homeSite?.api === "csp_Market") return <MarketView site={homeSite} />;

  return (
    <>
      <section className="catalog-section" aria-labelledby="catalog-heading">
        <div className="section-heading compact">
          <div>
            <h2 id="catalog-heading">{selectedClass ? selectedClass.type_name || "分类内容" : "首页内容"}</h2>
            <p>
              {homeSite
                ? `${homeSite.name} · ${catalogState === "loading" ? "加载中" : catalogState === "error" ? "加载失败" : `${catalog.list.length} 条`}${selectedClass ? ` · 第 ${page} 页` : ""}`
                : "选择下方内容源作为首页站点"}
            </p>
          </div>
          {homeSite && (
            <div className="catalog-actions">
              {selectedClass && (
                <button
                  className="secondary-button"
                  type="button"
                  disabled={catalogState === "loading"}
                  onClick={() => void selectHomeHandler(null)}
                >
                  返回首页
                </button>
              )}
              <button
                className="secondary-button"
                type="button"
                disabled={catalogState === "loading"}
                onClick={() => void loadCatalogHandler()}
              >
                <RefreshCw size={17} strokeWidth={1.8} />
                重新加载
              </button>
            </div>
          )}
        </div>

        {catalogState === "error" && (
          <div className="catalog-error" role="alert">{catalogError || "Spider 调用失败"}</div>
        )}
        {detailError && <div className="catalog-error" role="alert">{detailError}</div>}

        {classes.length > 0 && (
          <div className="class-row">
            <button
              className={!selectedClass ? "class-chip active" : "class-chip"}
              type="button"
              onClick={() => void selectHomeHandler(null)}
            >
              首页
            </button>
            {classes.map((item) => {
              const active = selectedClass?.type_id === item.type_id;
              return (
                <button
                  className={active ? "class-chip active" : "class-chip"}
                  key={`${item.type_id || ""}-${item.type_name || ""}`}
                  type="button"
                  onClick={() => void selectHomeHandler(item)}
                >
                  {item.type_name || item.type_id || "分类"}
                </button>
              );
            })}
          </div>
        )}

        {selectedClass && pagecount > 1 && (
          <div className="pager-row">
            <button
              className="secondary-button"
              type="button"
              disabled={!canPrev || catalogState === "loading"}
              onClick={() => void loadCatalogPageHandler({ page: page - 1 })}
            >
              上一页
            </button>
            <span>
              {page} / {pagecount}
              {typeof catalog.total === "number" ? ` · 共 ${catalog.total}` : ""}
            </span>
            <button
              className="secondary-button"
              type="button"
              disabled={!canNext || catalogState === "loading"}
              onClick={() => void loadCatalogPageHandler({ page: page + 1 })}
            >
              下一页
            </button>
          </div>
        )}

        <VodGrid
          emptyDetail={
            catalogState === "loading"
              ? "正在调用 Spider"
              : homeSite
                ? "当前站点未返回 list，或 runtime 尚不支持"
                : "导入配置并选择首页站点后显示"
          }
          emptyTitle={catalogState === "loading" ? "正在加载内容" : "暂无内容"}
          items={catalog.list}
          loading={catalogState === "loading"}
          onOpen={(item) => void openDetailHandler(item)}
        />
      </section>

      {(detail || detailLoading) && (
        <DetailPanel
          detail={detail}
          loading={detailLoading}
          siteKey={homeSite?.key || ""}
          siteName={homeSite?.name || ""}
          siteClick={homeSite?.click || ""}
          siteHeader={homeSite?.header}
          vipFlags={activeConfig?.document.flags || []}
          onPlay={onPlay}
          onClose={() => {
            setPrefetchPaused(false);
            setDetail(null);
            setDetailError("");
          }}
        />
      )}

    </>
  );
}

function SearchView({
  activeConfig,
  onOpenSettings,
  onPlay,
}: {
  activeConfig: ConfigDetail | null;
  onOpenSettings: () => void;
  onPlay: (session: PlaybackSession) => void;
}) {
  const homeSite = activeConfig?.homeSite || null;
  const [query, setQuery] = useState("");
  const [state, setState] = useState<CatalogState>("idle");
  const [error, setError] = useState("");
  const [items, setItems] = useState<VodItem[]>([]);
  const [detail, setDetail] = useState<DetailItem | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState("");

  usePrefetchDetails(items, homeSite?.key || null, homeSite?.type === 3 && state === "ready");

  async function runSearch(event?: React.FormEvent) {
    event?.preventDefault();
    if (!homeSite || homeSite.type !== 3) {
      setError("请先导入配置并选择 type=3 首页站点");
      setState("error");
      return;
    }
    const key = query.trim();
    if (!key) {
      setError("请输入搜索关键词");
      setState("error");
      return;
    }
    setState("loading");
    setError("");
    try {
      const result = await cachedInvoke<CatalogPage>({
        method: "searchContent",
        args: { key, quick: false },
        siteKey: homeSite.key,
        priority: "high",
        dedupe: true,
        cacheMs: 3 * 60 * 1000,
      });
      setItems(Array.isArray(result?.list) ? result.list : []);
      setState("ready");
    } catch (nextError) {
      setItems([]);
      setState("error");
      setError(errorText(nextError));
    }
  }

  async function openDetail(item: VodItem) {
    if (!homeSite || !item.vod_id) return;
    setPrefetchPaused(true);
    setDetailLoading(true);
    setDetailError("");
    setDetail({ ...item, vod_play_from: "", vod_play_url: "" });
    try {
      const result = await cachedInvoke<{ list?: DetailItem[] }>({
        method: "detailContent",
        args: { ids: [item.vod_id] },
        siteKey: homeSite.key,
        priority: "high",
        dedupe: true,
        cacheMs: 60 * 60 * 1000,
        persistMs: 24 * 60 * 60 * 1000,
        swr: true,
      });
      const first = Array.isArray(result?.list) ? result.list[0] : null;
      const resolved = first || { ...item };
      setDetail(resolved);
      void addHistory({
        siteKey: homeSite.key,
        siteName: homeSite.name,
        vodId: resolved.vod_id || item.vod_id,
        vodName: resolved.vod_name || item.vod_name || "",
        vodPic: resolved.vod_pic || item.vod_pic || "",
        vodRemarks: resolved.vod_remarks || item.vod_remarks || "",
      }).catch(() => undefined);
    } catch (nextError) {
      setDetail(null);
      setDetailError(errorText(nextError));
    } finally {
      setDetailLoading(false);
    }
  }

  const openDetailHandler = useClickHandler(
    (item: VodItem) => openDetail(item),
    { debounceMs: 100, priority: "high" }
  );

  return (
    <div className="search-view">
      <section className="catalog-section">
        <div className="section-heading compact">
          <div>
            <h2>站点搜索</h2>
            <p>{homeSite ? `在「${homeSite.name}」中搜索` : "需要活动配置与首页站点"}</p>
          </div>
          {!activeConfig && (
            <button className="secondary-button" type="button" onClick={onOpenSettings}>
              <Database size={17} strokeWidth={1.8} />导入配置
            </button>
          )}
        </div>

        <form className="search-form" onSubmit={(event) => void runSearch(event)}>
          <input
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder="输入片名、演员或关键词"
            disabled={!homeSite || state === "loading"}
          />
          <button className="command-button" disabled={!homeSite || state === "loading"} type="submit">
            <Search size={17} strokeWidth={1.8} />
            {state === "loading" ? "搜索中" : "搜索"}
          </button>
        </form>

        {error && <div className="catalog-error" role="alert">{error}</div>}
        {detailError && <div className="catalog-error" role="alert">{detailError}</div>}

        <VodGrid
          emptyDetail={state === "loading" ? "正在调用 Spider.searchContent" : "输入关键词后搜索"}
          emptyTitle={state === "loading" ? "正在搜索" : "暂无搜索结果"}
          items={items}
          loading={state === "loading"}
          onOpen={(item) => void openDetailHandler(item)}
        />
      </section>

      {(detail || detailLoading) && (
        <DetailPanel
          detail={detail}
          loading={detailLoading}
          siteKey={homeSite?.key || ""}
          siteName={homeSite?.name || ""}
          siteClick={homeSite?.click || ""}
          siteHeader={homeSite?.header}
          vipFlags={activeConfig?.document.flags || []}
          onPlay={onPlay}
          onClose={() => {
            setPrefetchPaused(false);
            setDetail(null);
            setDetailError("");
          }}
        />
      )}
    </div>
  );
}

function LiveView({
  activeConfig,
  onOpenSettings,
  onPlay,
}: {
  activeConfig: ConfigDetail | null;
  onOpenSettings: () => void;
  onPlay: (session: PlaybackSession) => void;
}) {
  const [sources, setSources] = useState<LiveSourceSummary[]>([]);
  const [selectedSource, setSelectedSource] = useState("");
  const [catalog, setCatalog] = useState<LiveCatalog | null>(null);
  const [selectedGroup, setSelectedGroup] = useState(0);
  const [selectedChannel, setSelectedChannel] = useState<LiveChannel | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [playError, setPlayError] = useState("");

  async function loadSource(name: string) {
    setSelectedSource(name);
    setSelectedGroup(0);
    setSelectedChannel(null);
    setLoading(true);
    setError("");
    try {
      setCatalog(await loadLiveSource(name));
    } catch (nextError) {
      setCatalog(null);
      setError(errorText(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    setSources([]);
    setCatalog(null);
    setSelectedSource("");
    setSelectedChannel(null);
    setError("");
    if (!activeConfig) return;
    setLoading(true);
    void listLiveSources()
      .then((items) => {
        if (cancelled) return;
        setSources(items);
        if (items[0]) {
          void loadSource(items[0].name);
        } else {
          setLoading(false);
        }
      })
      .catch((nextError) => {
        if (cancelled) return;
        setError(errorText(nextError));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeConfig?.summary.id]);

  const group = catalog?.groups[selectedGroup] || null;
  const channels = (group?.channels || []).filter((channel) =>
    channel.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
  );

  async function playLive(url: string) {
    setPlayError("");
    try {
      onPlay({ request: { url, title: selectedChannel?.name || "直播" } });
    } catch (nextError) {
      setPlayError(errorText(nextError));
    }
  }

  const loadSourceHandler = useClickHandler(
    (name: string) => loadSource(name),
    { debounceMs: 120, priority: "high" }
  );

  const playLiveHandler = useClickHandler(
    (url: string) => playLive(url),
    { debounceMs: 120, priority: "high" }
  );

  return (
    <div className="live-view">
      <section className="catalog-section">
        <div className="section-heading compact">
          <div>
            <h2>直播频道</h2>
            <p>
              {!activeConfig
                ? "需要先导入包含 lives 的配置"
                : loading
                  ? "正在加载直播源"
                  : catalog
                    ? `${catalog.source.name} · ${catalog.source.channelCount} 个频道`
                    : `${sources.length} 个直播源`}
            </p>
          </div>
          <div className="catalog-actions">
            {!activeConfig && (
              <button className="secondary-button" type="button" onClick={onOpenSettings}>
                <Database size={17} strokeWidth={1.8} />
                导入配置
              </button>
            )}
            {selectedSource && (
              <button
                className="secondary-button"
                type="button"
                disabled={loading}
                onClick={() => void loadSourceHandler(selectedSource)}
              >
                <RefreshCw size={17} strokeWidth={1.8} />
                重新加载
              </button>
            )}
          </div>
        </div>

        {error && <div className="catalog-error" role="alert">{error}</div>}
        {playError && <div className="catalog-error" role="alert">{playError}</div>}

        {sources.length > 0 && (
          <div className="live-source-row">
            {sources.map((source) => (
              <button
                className={selectedSource === source.name ? "live-source active" : "live-source"}
                key={source.name}
                type="button"
                disabled={loading && selectedSource === source.name}
                onClick={() => void loadSourceHandler(source.name)}
              >
                <Radio size={16} strokeWidth={1.8} />
                <span>
                  <strong>{source.name}</strong>
                  <small>{source.embedded ? `${source.channelCount} 个内嵌频道` : "远程源"}</small>
                </span>
              </button>
            ))}
          </div>
        )}

        {catalog?.groups.length ? (
          <>
            <div className="live-toolbar">
              <div className="class-row">
                {catalog.groups.map((item, index) => (
                  <button
                    className={selectedGroup === index ? "class-chip active" : "class-chip"}
                    key={`${item.name}-${index}`}
                    type="button"
                    onClick={() => {
                      setSelectedGroup(index);
                      setSelectedChannel(null);
                    }}
                  >
                    {item.name} ({item.channels.length})
                  </button>
                ))}
              </div>
              <input
                className="channel-filter"
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="筛选频道"
              />
            </div>

            <div className="channel-grid">
              {channels.map((channel, index) => (
                <button
                  className={selectedChannel === channel ? "channel-card active" : "channel-card"}
                  key={`${channel.number}-${channel.name}-${index}`}
                  type="button"
                  onClick={() => setSelectedChannel(channel)}
                >
                  <span className="channel-logo" style={posterStyle(channel.logo)}>
                    {!posterUrl(channel.logo) && <Radio size={18} strokeWidth={1.7} />}
                  </span>
                  <span className="channel-copy">
                    <strong>{channel.name}</strong>
                    <small>{channel.number || "-"} · {channel.urls.length} 条线路</small>
                  </span>
                </button>
              ))}
            </div>
          </>
        ) : (
          <div className="empty-library">
            <Radio size={26} strokeWidth={1.6} />
            <strong>{loading ? "正在加载直播源" : "当前配置没有直播频道"}</strong>
            <span>{loading ? "支持 M3U、TVBox TXT、JSON 与 Spider liveContent" : "配置根节点需要包含 lives"}</span>
          </div>
        )}
      </section>

      {selectedChannel && (
        <section className="channel-detail">
          <div className="section-heading compact">
            <div>
              <h2>{selectedChannel.name}</h2>
              <p>{group?.name || "直播"} · {selectedChannel.urls.length} 条候选线路</p>
            </div>
            <button className="secondary-button" type="button" onClick={() => setSelectedChannel(null)}>
              关闭
            </button>
          </div>
          <div className="stream-list">
            {selectedChannel.urls.map((url, index) => (
              <button className="stream-row" type="button" key={`${url}-${index}`} onClick={() => void playLiveHandler(url)}>
                <span>{index + 1}</span>
                <code title={url}>{url}</code>
              </button>
            ))}
          </div>
          <p className="detail-note">点击线路将在应用内播放器中播放。</p>
        </section>
      )}
    </div>
  );
}

function LibraryListView({
  activeConfig,
  emptyDetail,
  emptyTitle,
  kind,
  onOpenSettings,
  onPlay,
}: {
  activeConfig: ConfigDetail | null;
  emptyDetail: string;
  emptyTitle: string;
  kind: "keep" | "history";
  onOpenSettings: () => void;
  onPlay: (session: PlaybackSession) => void;
}) {
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<LibraryItem | null>(null);
  const [detail, setDetail] = useState<DetailItem | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState("");

  async function refresh() {
    setBusy(true);
    setError("");
    try {
      setItems(kind === "keep" ? await listKeeps() : await listHistory());
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, [kind]);

  async function openItem(item: LibraryItem) {
    setPrefetchPaused(true);
    setSelected(item);
    setDetailLoading(true);
    setDetailError("");
    try {
      const result = await cachedInvoke<{ list?: DetailItem[] }>({
        method: "detailContent",
        args: { ids: [item.vodId] },
        siteKey: item.siteKey,
        priority: "high",
        dedupe: true,
        cacheMs: 60 * 60 * 1000,
        persistMs: 24 * 60 * 60 * 1000,
        swr: true,
      });
      const first = Array.isArray(result?.list) ? result.list[0] : null;
      setDetail(
        first || {
          vod_id: item.vodId,
          vod_name: item.vodName,
          vod_pic: item.vodPic,
          vod_remarks: item.vodRemarks,
        },
      );
    } catch (nextError) {
      setDetail({
        vod_id: item.vodId,
        vod_name: item.vodName,
        vod_pic: item.vodPic,
        vod_remarks: item.vodRemarks,
      });
      setDetailError(errorText(nextError));
    } finally {
      setDetailLoading(false);
    }
  }

  async function removeItem(item: LibraryItem) {
    setBusy(true);
    try {
      if (kind === "keep") {
        await removeKeep(item.siteKey, item.vodId);
      } else {
        await removeHistory(item.siteKey, item.vodId);
      }
      await refresh();
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  const openItemHandler = useClickHandler(
    (item: LibraryItem) => openItem(item),
    { debounceMs: 100, priority: "high" }
  );

  return (
    <div className="library-page">
      <section className="catalog-section">
        <div className="section-heading compact">
          <div>
            <h2>{kind === "keep" ? "收藏" : "观看历史"}</h2>
            <p>{busy ? "加载中" : `${items.length} 条记录`}</p>
          </div>
          <div className="catalog-actions">
            <button className="secondary-button" type="button" disabled={busy} onClick={() => void refresh()}>
              <RefreshCw size={17} strokeWidth={1.8} />
              刷新
            </button>
            {kind === "history" && items.length > 0 && (
              <button
                className="secondary-button"
                type="button"
                disabled={busy}
                onClick={() => {
                  void clearHistory()
                    .then(refresh)
                    .catch((nextError) => setError(errorText(nextError)));
                }}
              >
                清空
              </button>
            )}
          </div>
        </div>
        {error && <div className="catalog-error" role="alert">{error}</div>}
        {detailError && <div className="catalog-error" role="alert">{detailError}</div>}
        {items.length ? (
          <div className="library-list">
            {items.map((item) => (
              <div className="library-row" key={`${item.siteKey}:${item.vodId}:${item.id}`}>
                <button className="library-main" type="button" onClick={() => void openItemHandler(item)}>
                  <span className="library-thumb" style={posterStyle(item.vodPic)}>
                    {!posterUrl(item.vodPic) && <Film size={18} strokeWidth={1.6} />}
                  </span>
                  <span className="library-copy">
                    <strong>{item.vodName || item.vodId}</strong>
                    <small>
                      {item.siteName || item.siteKey}
                      {item.vodRemarks ? ` · ${item.vodRemarks}` : ""}
                    </small>
                  </span>
                </button>
                <button
                  className="icon-button danger"
                  type="button"
                  title="移除"
                  aria-label={`移除 ${item.vodName || item.vodId}`}
                  disabled={busy}
                  onClick={() => void removeItem(item)}
                >
                  <Trash2 size={16} strokeWidth={1.8} />
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-library">
            <Heart size={26} strokeWidth={1.6} />
            <strong>{emptyTitle}</strong>
            <span>{emptyDetail}</span>
            <button className="secondary-button" type="button" onClick={onOpenSettings}>
              去导入配置
            </button>
          </div>
        )}
      </section>
      {(detail || detailLoading) && selected && (
        <DetailPanel
          detail={detail}
          loading={detailLoading}
          siteKey={selected.siteKey}
          siteName={selected.siteName}
          siteClick={activeConfig?.document.sites.find((site) => site.key === selected.siteKey)?.click || ""}
          siteHeader={activeConfig?.document.sites.find((site) => site.key === selected.siteKey)?.header}
          vipFlags={activeConfig?.document.flags || []}
          onPlay={onPlay}
          onClose={() => {
            setPrefetchPaused(false);
            setDetail(null);
            setSelected(null);
            setDetailError("");
            void refresh();
          }}
        />
      )}
    </div>
  );
}

function VodGrid({
  emptyDetail,
  emptyTitle,
  items,
  loading,
  onOpen,
}: {
  emptyDetail: string;
  emptyTitle: string;
  items: VodItem[];
  loading?: boolean;
  onOpen: (item: VodItem) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      setViewport({
        width: el.clientWidth,
        height: Math.max(320, window.innerHeight - rect.top - 24),
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  const GRID_COLS = 5;
  const ITEM_GAP = 12;
  const COPY_HEIGHT = 74;
  const containerWidth = viewport.width;
  const itemWidth =
    containerWidth > 0 ? (containerWidth - ITEM_GAP * (GRID_COLS - 1)) / GRID_COLS : 156;
  const itemHeight = Math.round(itemWidth * 1.5) + COPY_HEIGHT;
  // react-window 的 columnWidth/rowHeight 要包含 gap，cell 内容按 itemWidth/itemHeight 绘制
  const columnWidth = itemWidth + ITEM_GAP;
  const rowHeight = itemHeight + ITEM_GAP;

  const Cell = ({ columnIndex, rowIndex, style, ariaAttributes }: {
    columnIndex: number;
    rowIndex: number;
    style: React.CSSProperties;
    ariaAttributes: { "aria-colindex": number; role: "gridcell" };
  }) => {
    void ariaAttributes;
    const index = rowIndex * GRID_COLS + columnIndex;
    if (index >= items.length) return <div style={style} />;
    const item = items[index];
    return (
      <div
        className="vod-cell"
        style={{
          ...style,
          width: columnWidth,
          height: rowHeight,
        }}
      >
        <button
          className="vod-card"
          key={`${item.vod_id || item.vod_name || "vod"}-${index}`}
          type="button"
          onClick={() => onOpen(item)}
          style={{
            width: itemWidth,
            height: itemHeight,
          }}
        >
          <div className="vod-poster" style={posterStyle(item.vod_pic)}>
            {!posterUrl(item.vod_pic) && <Film size={22} strokeWidth={1.6} aria-hidden="true" />}
          </div>
          <div className="vod-copy">
            <strong title={item.vod_name || ""}>{item.vod_name || "未命名"}</strong>
            <small title={item.vod_remarks || ""}>{item.vod_remarks || item.vod_id || ""}</small>
          </div>
        </button>
      </div>
    );
  };

  const columnCount = GRID_COLS;
  const rowCount = Math.ceil(items.length / columnCount);

  return (
    <div className="vod-grid-wrap" ref={containerRef}>
      {loading ? (
        <SkeletonGrid containerWidth={containerWidth || undefined} rows={20} />
      ) : !items.length ? (
        <div className="empty-library">
          <ListVideo size={26} strokeWidth={1.6} aria-hidden="true" />
          <strong>{emptyTitle}</strong>
          <span>{emptyDetail}</span>
        </div>
      ) : (
        <Grid
          key={Math.round(itemWidth * 10)}
          className="vod-grid"
          columnCount={columnCount}
          columnWidth={columnWidth}
          rowCount={rowCount}
          rowHeight={rowHeight}
          style={{
            width: columnCount * columnWidth - ITEM_GAP,
            height: viewport.height,
          }}
          cellComponent={Cell}
          cellProps={{}}
          overscanCount={2}
        />
      )}
    </div>
  );
}

function DetailPanel({
  detail,
  loading,
  onClose,
  onPlay,
  siteClick,
  siteHeader,
  siteKey,
  siteName,
  vipFlags,
}: {
  detail: DetailItem | null;
  loading: boolean;
  onClose: () => void;
  onPlay: (session: PlaybackSession) => void;
  siteClick: string;
  siteHeader: unknown;
  siteKey: string;
  siteName: string;
  vipFlags: string[];
}) {
  const [kept, setKept] = useState(false);
  const [keepBusy, setKeepBusy] = useState(false);
  const [playBusy, setPlayBusy] = useState(false);
  const [playError, setPlayError] = useState("");
  const [selectedSource, setSelectedSource] = useState(0);
  // 剧集按钮分段渲染：默认只渲染前 180 集，避免上千集全量渲染卡顿
  const EPISODE_PAGE_SIZE = 180;
  const [episodeLimit, setEpisodeLimit] = useState(EPISODE_PAGE_SIZE);
  const [episodeLayout, setEpisodeLayout] = useState<"1" | "2">(() =>
    localStorage.getItem("episodeLayout") === "1" ? "1" : "2",
  );
  // detailContent 等待秒数：给用户明确的进度反馈，避免看似卡死
  const [waitSec, setWaitSec] = useState(0);
  const playSources = normalizePlaySources(detail?.vod_play_from, detail?.vod_play_url);

  useEffect(() => {
    setSelectedSource(0);
    setEpisodeLimit(EPISODE_PAGE_SIZE);
  }, [detail?.vod_id, detail?.vod_play_from, detail?.vod_play_url]);

  useEffect(() => {
    if (!loading) {
      setWaitSec(0);
      return;
    }
    setWaitSec(0);
    const timer = window.setInterval(() => setWaitSec((s) => s + 1), 1000);
    return () => window.clearInterval(timer);
  }, [loading, detail?.vod_id]);

  // 预取首集 playerContent（延迟执行），缩短首次点播延迟且不抢用户请求队列
  useEffect(() => {
    if (!detail?.vod_id || !siteKey || playSources.length === 0) return;
    const firstSource = playSources[0];
    const firstEpisode = firstSource?.episodes?.[0];
    if (!firstEpisode?.url) return;
    const timer = window.setTimeout(() => {
      void cachedInvoke<PlayerContentResponse>({
        method: "playerContent",
        args: { flag: firstSource.flag, id: firstEpisode.url, vipFlags },
        siteKey,
        priority: "low",
        dedupe: true,
        cacheMs: 2 * 60 * 1000,
      }).catch(() => {
        // 预取失败不阻塞主流程
      });
    }, 1500);
    return () => window.clearTimeout(timer);
  }, [detail?.vod_id, detail?.vod_play_from, detail?.vod_play_url, siteKey, vipFlags]);

  useEffect(() => {
    let cancelled = false;
    if (!detail?.vod_id || !siteKey) {
      setKept(false);
      return;
    }
    void isKept(siteKey, detail.vod_id)
      .then((value) => {
        if (!cancelled) setKept(value);
      })
      .catch(() => {
        if (!cancelled) setKept(false);
      });
    return () => {
      cancelled = true;
    };
  }, [detail?.vod_id, siteKey]);

  async function toggleKeep() {
    if (!detail?.vod_id || !siteKey || keepBusy) return;
    setKeepBusy(true);
    try {
      if (kept) {
        await removeKeep(siteKey, detail.vod_id);
        setKept(false);
      } else {
        await addKeep({
          siteKey,
          siteName,
          vodId: detail.vod_id,
          vodName: detail.vod_name || "",
          vodPic: detail.vod_pic || "",
          vodRemarks: detail.vod_remarks || "",
        });
        setKept(true);
      }
    } catch {
      // keep toggle is best-effort in the detail panel
    } finally {
      setKeepBusy(false);
    }
  }

  const toggleKeepHandler = useClickHandler(
    () => toggleKeep(),
    { debounceMs: 120, priority: "high" }
  );

  async function playEpisode(source: string, episode: { name: string; url: string }) {
    if (!siteKey || !episode.url || playBusy) return;
    setPlayBusy(true);
    setPlayError("");
    try {
      const result = await cachedInvoke<PlayerContentResponse>({
        method: "playerContent",
        args: { flag: source, id: episode.url, vipFlags },
        siteKey,
        priority: "high",
        dedupe: true,
        cacheMs: 2 * 60 * 1000,
      });
      let url = firstPlayableUrl(result?.url) || episode.url;
      const effectiveFlag = result?.flag || source;
      const playUrl = result?.playUrl?.trim() || "";
      const parserDirective = /^(parse|json):/i.test(playUrl);
      if (playUrl && !parserDirective) url = `${playUrl}${url}`;
      if (!isPlayerUrl(url)) {
        throw new Error(result?.msg || "Spider 未返回可播放的媒体地址");
      }
      let headers = mergePlayerHeaders(siteHeader, result?.header, result?.headers);
      if (result && (
        Number(result.parse || 0) !== 0
        || Number(result.jx || 0) !== 0
        || vipFlags.includes(effectiveFlag)
        || parserDirective
      )) {
        const resolved = await resolvePlayer({
          url,
          headers,
          parse: Number(result.parse || 0),
          jx: Number(result.jx || 0),
          playUrl: parserDirective ? playUrl : "",
          flag: effectiveFlag,
          siteKey,
          click: result.click || siteClick,
        });
        url = resolved.url;
        headers = { ...headers, ...resolved.headers };
      }
      let start: number | undefined;
      if (detail?.vod_id) {
        try {
          const progress = await getPlayerProgress({
            siteKey,
            vodId: detail.vod_id,
            episodeUrl: episode.url,
          });
          if (
            progress
            && Number.isFinite(progress.position)
            && Number.isFinite(progress.duration)
            && progress.position > 5
            && progress.position < progress.duration - 5
          ) {
            start = progress.position;
          }
        } catch {
          // A missing progress record should never block a first play.
        }
      }
      onPlay({
        request: {
          url,
          title: `${detail?.vod_name || "播放"} · ${episode.name}`,
          headers,
          start,
        },
        siteKey,
        vodId: detail?.vod_id,
        episodeUrl: episode.url,
      });
    } catch (nextError) {
      setPlayError(errorText(nextError));
    } finally {
      setPlayBusy(false);
    }
  }

  return (
    <section className="detail-screen" aria-label="影片详情">
      <div className="section-heading compact">
        <div>
          <h2>{detail?.vod_name || "加载详情"}</h2>
          <p>{loading ? `正在获取播放列表（已等待 ${waitSec}s）` : detail?.vod_remarks || detail?.type_name || "详情结果"}</p>
        </div>
        <div className="catalog-actions">
          {detail?.vod_id && siteKey && (
            <button className="secondary-button" type="button" disabled={keepBusy || loading} onClick={() => void toggleKeepHandler()}>
              <Heart size={16} strokeWidth={1.8} />
              {kept ? "已收藏" : "收藏"}
            </button>
          )}
          <button className="secondary-button" type="button" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
      {detail ? (
        <div className="detail-body">
          <div className="detail-poster" style={posterStyle(detail.vod_pic)}>
            {!posterUrl(detail.vod_pic) && <Film size={28} strokeWidth={1.6} />}
          </div>
          <div className="detail-meta">
            <p><strong>年份</strong>{detail.vod_year || "-"}</p>
            <p><strong>地区</strong>{detail.vod_area || "-"}</p>
            <p><strong>导演</strong>{detail.vod_director || "-"}</p>
            <p><strong>主演</strong>{detail.vod_actor || "-"}</p>
            <p className="detail-content">{detail.vod_content || "暂无简介"}</p>
            {playError && <div className="catalog-error" role="alert">{playError}</div>}
            {playSources.length > 0 && (
              <div className="play-groups">
                <div className="episode-toolbar">
                  <div className="source-tabs" role="tablist" aria-label="播放来源">
                    {playSources.map((source, index) => (
                      <button
                        className={selectedSource === index ? "source-tab active" : "source-tab"}
                        key={`${source.flag}-${index}`}
                        type="button"
                        role="tab"
                        aria-selected={selectedSource === index}
                        onClick={() => setSelectedSource(index)}
                      >
                        {source.flag}
                      </button>
                    ))}
                  </div>
                  <div className="episode-layout-toggle" role="group" aria-label="剧集排列方式">
                    <button
                      type="button"
                      className={episodeLayout === "1" ? "active" : ""}
                      onClick={() => {
                        setEpisodeLayout("1");
                        localStorage.setItem("episodeLayout", "1");
                      }}
                    >
                      单列
                    </button>
                    <button
                      type="button"
                      className={episodeLayout === "2" ? "active" : ""}
                      onClick={() => {
                        setEpisodeLayout("2");
                        localStorage.setItem("episodeLayout", "2");
                      }}
                    >
                      双列
                    </button>
                  </div>
                </div>
                <div className={`episode-grid cols-${episodeLayout}`}>
                  {(playSources[selectedSource]?.episodes || []).slice(0, episodeLimit).map((episode, episodeIndex) => (
                    <button
                      className="episode-chip"
                      disabled={playBusy || !episode.url}
                      key={`${episode.name}-${episodeIndex}`}
                      title={episode.url}
                      type="button"
                      onClick={() => void playEpisode(playSources[selectedSource].flag, episode)}
                    >
                      <MarqueeText text={episode.name} />
                    </button>
                  ))}
                </div>
                {(playSources[selectedSource]?.episodes?.length || 0) > episodeLimit && (
                  <button className="episode-more" type="button" onClick={() => setEpisodeLimit(Number.MAX_SAFE_INTEGER)}>
                    展开全部剧集（共 {playSources[selectedSource]?.episodes?.length} 集）
                  </button>
                )}
              </div>
            )}
            <p className="detail-note">{loading ? "正在获取播放列表…" : "选择来源和集数后将在应用内播放器中播放。"}</p>
          </div>
        </div>
      ) : (
        <SkeletonDetail />
      )}
    </section>
  );
}

let imageProxyPort: number | undefined;

function MarqueeText({ text }: { text: string }) {
  const outerRef = useRef<HTMLSpanElement>(null);
  const innerRef = useRef<HTMLSpanElement>(null);
  const [offset, setOffset] = useState(0);
  useEffect(() => {
    const outer = outerRef.current;
    const inner = innerRef.current;
    if (!outer || !inner) return;
    const update = () => setOffset(Math.max(0, inner.scrollWidth - outer.clientWidth));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(outer);
    return () => observer.disconnect();
  }, [text]);
  return (
    <span
      className={`episode-text${offset > 0 ? " marquee" : ""}`}
      ref={outerRef}
      style={{ "--marquee-offset": `-${offset}px` } as CSSProperties}
    >
      <span className="episode-text-inner" ref={innerRef}>{text}</span>
    </span>
  );
}

async function initImageProxy() {
  try {
    const port = await invoke<number | null>("image_proxy_port");
    imageProxyPort = port ?? undefined;
  } catch {
    imageProxyPort = undefined;
  }
}

function posterUrl(value?: string) {
  if (!value) return "";
  const base = value.split("@")[0]?.trim() || "";
  if (!base.startsWith("http")) return "";
  if (imageProxyPort) {
    return `http://127.0.0.1:${imageProxyPort}/img?u=${encodeURIComponent(base)}`;
  }
  return base;
}

function posterStyle(value?: string) {
  const url = posterUrl(value);
  return url
    ? {
        backgroundImage: `linear-gradient(180deg, rgba(17,19,16,0.05), rgba(17,19,16,0.72)), url("${url.replace(/"/g, '\\"')}")`,
      }
    : undefined;
}

function normalizePlaySources(flags = "", urls = "") {
  const sourceFlags = flags.split("$$$");
  const groups = urls.split("$$$");
  const count = Math.max(sourceFlags.length, groups.length);
  return Array.from({ length: count }, (_, index) => ({
    flag: sourceFlags[index]?.trim() || `线路 ${index + 1}`,
    episodes: splitEpisodes(groups[index] || "").map((value, episodeIndex) => {
      const separator = value.indexOf("$");
      if (separator < 0) {
        return { name: String(episodeIndex + 1).padStart(2, "0"), url: value.trim() };
      }
      return {
        name: value.slice(0, separator).trim() || String(episodeIndex + 1).padStart(2, "0"),
        url: value.slice(separator + 1).trim(),
      };
    }),
  })).filter((source) => source.flag || source.episodes.length);
}

function splitEpisodes(value: string) {
  if (!value) return [];
  const result: string[] = [];
  let start = 0;
  let depth = 0;
  const opens = "[(（【《";
  const closes = "])）】》";
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (opens.includes(character)) depth += 1;
    else if (closes.includes(character) && depth > 0) depth -= 1;
    else if (character === "#" && depth === 0) {
      const item = value.slice(start, index).trim();
      if (item) result.push(item);
      start = index + 1;
    }
  }
  const tail = value.slice(start).trim();
  if (tail) result.push(tail);
  return result;
}

function firstPlayableUrl(value: unknown) {
  if (typeof value === "string") return value.trim();
  if (Array.isArray(value)) {
    const strings = value.filter((item): item is string => typeof item === "string" && Boolean(item.trim()));
    for (let index = 1; index < strings.length; index += 2) {
      if (isPlayerUrl(strings[index])) return strings[index].trim();
    }
    return strings.find((item) => isPlayerUrl(item))?.trim() || "";
  }
  if (value && typeof value === "object") {
    const record = value as { values?: unknown; position?: unknown; url?: unknown; v?: unknown };
    const direct = [record.url, record.v].find((item): item is string => typeof item === "string" && isPlayerUrl(item));
    if (direct) return direct.trim();
    if (Array.isArray(record.values)) {
      const position = Number(record.position || 0);
      const entries = record.values;
      const selected = entries[position];
      if (typeof selected === "string" && isPlayerUrl(selected)) return selected.trim();
      if (selected && typeof selected === "object") {
        const candidate = (selected as { v?: unknown; url?: unknown }).v ?? (selected as { url?: unknown }).url;
        if (typeof candidate === "string" && isPlayerUrl(candidate)) return candidate.trim();
      }
      for (const entry of entries) {
        if (typeof entry === "string" && isPlayerUrl(entry)) return entry.trim();
        if (entry && typeof entry === "object") {
          const candidate = (entry as { v?: unknown; url?: unknown }).v ?? (entry as { url?: unknown }).url;
          if (typeof candidate === "string" && isPlayerUrl(candidate)) return candidate.trim();
        }
      }
    }
  }
  return "";
}

function isPlayerUrl(value: string) {
  return /^(https?:|file:|rtmp:|rtsp:|udp:|av:)/i.test(value.trim());
}

function mergePlayerHeaders(...values: unknown[]): Record<string, string> {
  return values.reduce<Record<string, string>>((headers, value) => {
    if (typeof value === "string") {
      try {
        return { ...headers, ...mergePlayerHeaders(JSON.parse(value)) };
      } catch {
        return headers;
      }
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) return headers;
    for (const [name, content] of Object.entries(value)) {
      if (typeof content === "string" && name.trim() && content.trim()) {
        headers[name] = content;
      }
    }
    return headers;
  }, {});
}

function ModalShell({
  children,
  onClose,
  subtitle,
  title,
}: {
  children: React.ReactNode;
  onClose: () => void;
  subtitle?: string;
  title: string;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section className="modal-shell" role="dialog" aria-modal="true" aria-label={title}>
        <header className="modal-header">
          <div>
            <h2>{title}</h2>
            {subtitle && <p>{subtitle}</p>}
          </div>
          <button className="icon-button" type="button" title="关闭" aria-label="关闭" onClick={onClose}>
            <X size={18} strokeWidth={1.8} />
          </button>
        </header>
        {children}
      </section>
    </div>
  );
}

function SourceDialog({
  activeKey,
  configCenterAvailable,
  onClose,
  onOpenConfigCenter,
  onSelect,
  sites,
}: {
  activeKey: string;
  configCenterAvailable: boolean;
  onClose: () => void;
  onOpenConfigCenter: () => void;
  onSelect: (site: SiteConfig) => Promise<void> | void;
  sites: SiteConfig[];
}) {
  const [query, setQuery] = useState("");
  const [busyKey, setBusyKey] = useState("");
  const contentSites = sites.filter((site) => site.api !== "csp_Config");
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filtered = contentSites.filter((site) =>
    !normalizedQuery
    || site.name.toLocaleLowerCase().includes(normalizedQuery)
    || site.key.toLocaleLowerCase().includes(normalizedQuery),
  );

  async function select(site: SiteConfig) {
    if (site.type !== 3 || busyKey) return;
    setBusyKey(site.key);
    try {
      await onSelect(site);
    } finally {
      setBusyKey("");
    }
  }

  return (
    <ModalShell
      title="内容源"
      subtitle={`${contentSites.length} 个可见站点${activeKey ? " · 点击后立即切换首页" : ""}`}
      onClose={onClose}
    >
      <div className="source-dialog-toolbar">
        <label className="dialog-search">
          <Search size={16} strokeWidth={1.8} />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder="搜索内容源"
          />
        </label>
        <button
          className="secondary-button"
          type="button"
          disabled={!configCenterAvailable}
          onClick={onOpenConfigCenter}
        >
          <Settings size={16} strokeWidth={1.8} />
          配置中心
        </button>
      </div>
      <div className="source-dialog-list">
        {filtered.map((site) => {
          const supported = site.type === 3;
          const active = site.key === activeKey;
          return (
            <button
              className={active ? "source-option active" : "source-option"}
              disabled={!supported || Boolean(busyKey)}
              key={site.key}
              type="button"
              onClick={() => void select(site)}
            >
              <span className="source-option-icon"><Globe2 size={18} strokeWidth={1.7} /></span>
              <span className="source-option-copy">
                <strong>{site.name}</strong>
                <small>{supported ? site.api || site.key : "当前桌面版本暂不支持此类型"}</small>
              </span>
              {busyKey === site.key ? <RefreshCw className="spin" size={17} /> : active ? <Check size={17} /> : null}
            </button>
          );
        })}
        {!filtered.length && <div className="dialog-empty">没有匹配的内容源</div>}
      </div>
    </ModalShell>
  );
}

function ConfigCenterDialog({
  activeConfig,
  onClose,
  onOpenSettings,
  site,
}: {
  activeConfig: ConfigDetail | null;
  onClose: () => void;
  onOpenSettings: () => void;
  site: SiteConfig | null;
}) {
  return (
    <ModalShell
      title="配置中心"
      subtitle={activeConfig ? activeConfig.summary.desc : "当前没有活动配置"}
      onClose={onClose}
    >
      <div className="config-center-summary">
        <div><span>配置地址</span><strong title={activeConfig?.summary.url}>{activeConfig?.summary.url || "-"}</strong></div>
        <div><span>当前内容源</span><strong>{activeConfig?.homeSite?.name || "未选择"}</strong></div>
        <div><span>站点数量</span><strong>{activeConfig?.summary.siteCount || 0}</strong></div>
      </div>
      {site ? (
        <ConfigCenterView site={site} />
      ) : (
        <div className="dialog-empty">当前配置不包含 `csp_Config` 配置中心。</div>
      )}
      <footer className="modal-footer">
        <button className="secondary-button" type="button" onClick={onOpenSettings}>
          <Database size={16} strokeWidth={1.8} />
          管理配置文件
        </button>
        <button className="command-button" type="button" onClick={onClose}>完成</button>
      </footer>
    </ModalShell>
  );
}

function PlayerScreen({
  onClose,
  session,
}: {
  onClose: () => void;
  session: PlaybackSession;
}) {
  const [status, setStatus] = useState<PlayerStatus | null>(session.opened ?? null);
  const [opening, setOpening] = useState(!session.opened);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    async function start() {
      setOpening(true);
      setError("");
      setStatus(null);
      try {
        const next = session.opened ?? (await openPlayer(session.request));
        if (cancelled) return;
        setStatus(next);
        timer = window.setInterval(async () => {
          try {
            const current = await playerStatus();
            if (!cancelled) setStatus(current);
          } catch {
            // The visible state remains the actionable fallback.
          }
        }, 750);
      } catch (nextError) {
        if (!cancelled) setError(errorText(nextError));
      } finally {
        if (!cancelled) setOpening(false);
      }
    }
    if (!session.opened) void start();
    else {
      setOpening(false);
      timer = window.setInterval(async () => {
        try {
          const current = await playerStatus();
          if (!cancelled) setStatus(current);
        } catch {
          // The visible state remains the actionable fallback.
        }
      }, 750);
    }
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      void closePlayer();
    };
  }, [session]);

  async function control(command: "togglePause" | "seek" | "volume" | "speed" | "stop" | "fullscreen", value?: number) {
    try {
      setStatus(await controlPlayer(command, value));
    } catch (nextError) {
      setError(errorText(nextError));
    }
  }

  async function close() {
    try {
      await closePlayer();
    } catch {
      // The player may already be closed.
    }
    onClose();
  }

  const stateLabel = error
    ? "播放失败"
    : opening
      ? "准备中"
      : status?.idle
        ? "等待媒体"
        : status?.paused
          ? "已暂停"
          : status?.ready
            ? "播放中"
            : "等待媒体";

  return (
    <section className="player-screen" aria-label="应用内播放器">
      <header className="player-screen-header">
        <div className="player-header-copy">
          <div className="player-title-line">
            <h2>{session.request.title || "正在播放"}</h2>
            <span className={error ? "player-state-pill error" : status?.paused ? "player-state-pill paused" : "player-state-pill"}>{stateLabel}</span>
          </div>
          <p>{opening ? "正在启动内置播放器" : status?.external ? "独立播放窗口 · mpv 自带控制条" : "独立播放窗口 · libmpv"}</p>
        </div>
        <button className="icon-button player-close" type="button" title="关闭播放器" aria-label="关闭播放器" onClick={() => void close()}>
          <X size={19} strokeWidth={1.8} />
        </button>
      </header>
      <div className="player-surface-wrap">
        <div className="player-surface-state">
          {opening ? (
            <>
              <RefreshCw className="spin" size={24} />
              <span>正在启动内置播放器</span>
            </>
          ) : error ? (
            <>
              <strong>播放失败</strong>
              <span>{error}</span>
              <button className="player-retry" type="button" onClick={() => void close()}>
                <RotateCcw size={16} strokeWidth={1.8} />
                关闭并重试
              </button>
            </>
          ) : (
            <>
              <strong>播放窗口已独立打开</strong>
              <span>{status?.external ? "鼠标移到窗口底部即显示控制条 · 单击暂停 · 双击全屏" : "单击暂停 · 双击全屏 · 底部进度条 · 方向键进退"}</span>
              <div className="player-external-actions">
                {!status?.external && (
                  <>
                    <button className="player-retry" type="button" onClick={() => void control("togglePause")}>
                      {status?.paused ? <Play size={16} strokeWidth={1.8} /> : <Pause size={16} strokeWidth={1.8} />}
                      {status?.paused ? "继续播放" : "暂停播放"}
                    </button>
                    <button className="player-retry" type="button" onClick={() => void control("fullscreen")}>
                      <Maximize2 size={16} strokeWidth={1.8} />
                      全屏
                    </button>
                  </>
                )}
                <button className="player-retry" type="button" onClick={() => void close()}>
                  <Square size={16} strokeWidth={1.8} />
                  关闭播放器
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

interface SettingsViewProps {
  activeConfig: ConfigDetail | null;
  configs: ConfigSummary[];
  initialError: string;
  onChanged: () => void;
  onRefresh: () => void;
}

function SettingsView({ activeConfig, configs, initialError, onChanged, onRefresh }: SettingsViewProps) {
  const [mode, setMode] = useState<"url" | "json" | "file">("url");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [json, setJson] = useState("");
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(initialError);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      if (mode === "url") {
        if (!url.trim()) throw new Error("请输入配置 URL");
        await loadConfigUrl(url.trim(), name.trim());
      } else if (mode === "json") {
        if (!json.trim()) throw new Error("请输入配置 JSON");
        await importConfigJson(json, name.trim());
      } else {
        if (!path.trim()) throw new Error("请选择配置文件");
        await importConfigFile(path.trim(), name.trim());
      }
      onChanged();
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  async function chooseConfigFile() {
    setError("");
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (selected) setPath(selected);
    } catch (nextError) {
      setError(errorText(nextError));
    }
  }

  async function activate(id: number) {
    setBusy(true);
    setError("");
    try {
      await activateConfig(id);
      onChanged();
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: number) {
    setBusy(true);
    setError("");
    try {
      await deleteConfig(id);
      onRefresh();
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-view">
      <section className="settings-section">
        <div className="section-heading compact">
          <div><h2>点播配置</h2><p>导入 WebHomeTV / TVBox JSON 配置</p></div>
          <div className="segmented-control" aria-label="导入方式">
            <button className={mode === "url" ? "active" : ""} type="button" onClick={() => setMode("url")}><Download size={15} />URL</button>
            <button className={mode === "json" ? "active" : ""} type="button" onClick={() => setMode("json")}><FileJson size={15} />JSON</button>
            <button className={mode === "file" ? "active" : ""} type="button" onClick={() => setMode("file")}><FolderOpen size={15} />文件</button>
          </div>
        </div>

        <form className="config-form" onSubmit={submit}>
          <label>
            <span>显示名称</span>
            <input value={name} onChange={(event) => setName(event.currentTarget.value)} placeholder="可选" />
          </label>
          {mode === "url" ? (
            <label>
              <span>配置 URL</span>
              <input type="url" value={url} onChange={(event) => setUrl(event.currentTarget.value)} placeholder="https://example.com/config.json" />
            </label>
          ) : mode === "json" ? (
            <label>
              <span>配置 JSON</span>
              <textarea value={json} onChange={(event) => setJson(event.currentTarget.value)} placeholder={'{"sites":[...]}' } spellCheck={false} />
            </label>
          ) : (
            <label>
              <span>配置文件</span>
              <span className="file-picker">
                <input value={path} onChange={(event) => setPath(event.currentTarget.value)} placeholder="D:\\path\\config.json" />
                <button className="icon-button" disabled={busy} type="button" title="选择配置文件" aria-label="选择配置文件" onClick={() => void chooseConfigFile()}>
                  <FolderOpen size={18} strokeWidth={1.8} />
                </button>
              </span>
            </label>
          )}
          {error && <div className="form-error" role="alert">{error}</div>}
          <div className="form-actions">
            <button className="command-button" disabled={busy} type="submit">
              <Download size={17} strokeWidth={1.8} />{busy ? "处理中" : "导入并启用"}
            </button>
          </div>
        </form>
      </section>

      <section className="settings-section config-list-section">
        <div className="section-heading compact"><div><h2>已保存配置</h2><p>{configs.length} 个配置</p></div></div>
        {configs.length ? (
          <div className="config-list">
            {configs.map((item) => (
              <div className={item.active ? "config-row active" : "config-row"} key={item.id}>
                <span className="config-row-icon"><Database size={18} strokeWidth={1.7} /></span>
                <span className="config-row-copy"><strong>{item.desc}</strong><small title={item.url}>{item.siteCount} 个站点 · {item.url}</small></span>
                {item.active ? <span className="active-label"><Check size={14} />当前</span> : (
                  <button className="row-button" disabled={busy} type="button" onClick={() => void activate(item.id)}>启用</button>
                )}
                <button className="icon-button danger" disabled={busy} type="button" title="删除配置" aria-label={`删除 ${item.desc}`} onClick={() => void remove(item.id)}>
                  <Trash2 size={16} strokeWidth={1.8} />
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="empty-configs"><Database size={24} /><span>暂无已保存配置</span></div>
        )}
        {activeConfig?.summary.notice && <div className="config-notice">{activeConfig.summary.notice}</div>}
      </section>
    </div>
  );
}

function MarketView({ site }: { site: SiteConfig }) {
  const [categories, setCategories] = useState<MarketCategory[]>([]);
  const [selected, setSelected] = useState<MarketCategory | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<MarketInstallResult | null>(null);
  const ext = typeof site.ext === "string" ? site.ext : "";

  async function load() {
    setState("loading");
    setError("");
    try {
      const next = await marketCatalog(ext);
      setCategories(next);
      setSelected(next[0] || null);
      setState("ready");
    } catch (nextError) {
      setError(errorText(nextError));
      setState("error");
    }
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setState("loading");
      setError("");
      try {
        const next = await marketCatalog(ext);
        if (cancelled) return;
        setCategories(next);
        setSelected(next[0] || null);
        setState("ready");
      } catch (nextError) {
        if (cancelled) return;
        setError(errorText(nextError));
        setState("error");
      }
    })();
    return () => { cancelled = true; };
  }, [site.key]);

  async function install(item: MarketItem) {
    if (!item.url || busy) return;
    const versionLabel = item.version ? `（版本 ${item.version}）` : "";
    if (!window.confirm(`确认下载并更新到「${item.name}」${versionLabel}？更新完成后需要重启程序生效。`)) return;
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const next = await marketInstall(item.url);
      setResult(next);
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setBusy(false);
    }
  }

  async function restart() {
    try {
      await appRestart();
    } catch {
      // The app is expected to exit while restarting.
    }
  }

  const items = selected?.list || [];
  const status = state === "loading" ? "加载中" : state === "error" ? "加载失败" : `${items.length} 项`;

  return (
    <section className="config-center-content">
      <div className="section-heading compact">
        <div>
          <h2>版本信息</h2>
          <p>{site.name} · {status}</p>
        </div>
        <button className="icon-button" disabled={busy} type="button" title="刷新版本信息" aria-label="刷新版本信息" onClick={() => void load()}>
          <RefreshCw className={state === "loading" ? "spin" : ""} size={17} strokeWidth={1.8} />
        </button>
      </div>

      {categories.length > 0 && (
        <div className="config-center-tabs" role="tablist" aria-label="版本分类">
          {categories.map((category) => {
            const active = category === selected;
            return (
              <button
                aria-selected={active}
                className={active ? "active" : ""}
                disabled={busy}
                key={category.name}
                role="tab"
                type="button"
                onClick={() => setSelected(category)}
              >
                {category.name || "分类"}
              </button>
            );
          })}
        </div>
      )}

      {error && <div className="form-error" role="alert">{error}</div>}

      {state === "loading" && (
        <div className="config-center-state"><RefreshCw className="spin" size={20} />正在加载版本信息</div>
      )}

      {result && (
        <div className="config-notice" role="status">
          <strong>更新完成</strong>
          <span>
            {result.mode === "replace"
              ? `已更新「${result.configName}」${result.version ? `（版本 ${result.version}）` : ""}：新文件已替换到原配置路径，原有配置保持不变。`
              : `已导入配置「${result.configName}」${result.version ? `（版本 ${result.version}）` : ""}，重启后生效。`}
          </span>
          <button className="command-button" type="button" onClick={() => void restart()}>
            <RefreshCw size={15} strokeWidth={1.8} />立即重启
          </button>
        </div>
      )}

      {state === "ready" && items.length > 0 && (
        <div className="config-center-grid">
          {items.map((item, index) => {
            const installable = Boolean(item.url);
            return (
              <article
                className={installable ? "config-entry market-entry" : "config-entry market-entry current"}
                key={`${item.name}-${index}`}
              >
                <div className="config-entry-poster" title={item.name}>
                  <Settings aria-hidden="true" size={21} strokeWidth={1.6} />
                </div>
                <div className="config-entry-body">
                  <div className="config-entry-copy">
                    <strong title={item.name}>{item.name}</strong>
                    {item.version && <small>版本 {item.version}</small>}
                  </div>
                  <div className="config-entry-control">
                    {installable ? (
                      <button
                        className="config-auth-button"
                        disabled={busy}
                        type="button"
                        onClick={() => void install(item)}
                      >
                        <Download size={16} strokeWidth={1.8} />更新到最新版
                      </button>
                    ) : (
                      <button className="config-unsupported" disabled type="button">当前已安装版本</button>
                    )}
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      )}

      {state === "ready" && !items.length && (
        <div className="config-center-state">版本信息未返回条目</div>
      )}
    </section>
  );
}

function ConfigCenterView({ site }: { site: SiteConfig }) {
  if (site.api === "csp_Market") return <MarketView site={site} />;
  const [state, setState] = useState<CatalogState>("idle");
  const [classes, setClasses] = useState<ClassItem[]>([]);
  const [selectedClass, setSelectedClass] = useState<ClassItem | null>(null);
  const [items, setItems] = useState<VodItem[]>([]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [operationBusy, setOperationBusy] = useState(false);
  const [authBusy, setAuthBusy] = useState(false);
  const [auth, setAuth] = useState<CloudAuthSession | null>(null);
  const [error, setError] = useState("");
  const mountedRef = useRef(false);
  const requestRef = useRef(0);
  const siteGenerationRef = useRef(0);
  const siteKeyRef = useRef(site.key);
  const authSessionRef = useRef("");
  siteKeyRef.current = site.key;

  function canCommit(request: number, siteKey: string, cancelled = false) {
    return !cancelled
      && mountedRef.current
      && requestRef.current === request
      && siteKeyRef.current === siteKey;
  }

  async function readConfigValues(nextItems: VodItem[], siteKey: string) {
    const keys = new Set<string>();
    for (const item of nextItems) {
      const contract = classifyConfigAction(item.action);
      if (contract.kind === "select") keys.add(contract.key);
      else if (contract.kind === "pan-block") keys.add("panBlock");
      else if (contract.kind === "pan-order") keys.add(contract.key);
    }

    const results = await Promise.allSettled([...keys].map(async (key) => {
      const value = await invokeSpider<unknown>("configGet", { key }, siteKey);
      return [key, typeof value === "string" ? value : value == null ? "" : String(value)] as const;
    }));
    const nextValues: Record<string, string> = {};
    let failures = 0;
    for (const result of results) {
      if (result.status === "fulfilled") nextValues[result.value[0]] = result.value[1];
      else failures += 1;
    }
    return { failures, values: nextValues };
  }

  async function loadCategory(classItem: ClassItem) {
    const siteKey = site.key;
    const request = ++requestRef.current;
    const tid = String(classItem.type_id ?? "").trim();
    setSelectedClass(classItem);
    setItems([]);
    setValues({});
    setError("");
    if (!tid) {
      setState("error");
      setError("当前分类缺少 type_id，无法读取配置条目。");
      return;
    }

    setState("loading");
    try {
      const result = await invokeSpider<CatalogPage>(
        "categoryContent",
        { tid, page: "1", filter: true, extend: {} },
        siteKey,
      );
      if (!canCommit(request, siteKey)) return;
      const nextItems = Array.isArray(result?.list) ? result.list : [];
      const config = await readConfigValues(nextItems, siteKey);
      if (!canCommit(request, siteKey)) return;
      setItems(nextItems);
      setValues(config.values);
      setState("ready");
      if (config.failures) setError(`${config.failures} 项配置值读取失败，相关控件已停用。`);
    } catch (nextError) {
      if (!canCommit(request, siteKey)) return;
      setItems([]);
      setValues({});
      setState("error");
      setError(errorText(nextError));
    }
  }

  async function loadInitial(isCancelled: () => boolean = () => false) {
    const siteKey = site.key;
    const request = ++requestRef.current;
    setClasses([]);
    setSelectedClass(null);
    setItems([]);
    setValues({});
    setError("");
    setState("loading");
    try {
      const home = await invokeSpider<CatalogPage>("homeContent", { filter: true }, siteKey);
      if (!canCommit(request, siteKey, isCancelled())) return;
      const nextClasses = Array.isArray(home?.class) ? home.class : [];
      const firstClass = nextClasses[0] || null;
      setClasses(nextClasses);
      setSelectedClass(firstClass);
      if (!firstClass) {
        setState("ready");
        return;
      }

      const tid = String(firstClass.type_id ?? "").trim();
      if (!tid) {
        setState("error");
        setError("首个分类缺少 type_id，无法读取配置条目。");
        return;
      }
      const category = await invokeSpider<CatalogPage>(
        "categoryContent",
        { tid, page: "1", filter: true, extend: {} },
        siteKey,
      );
      if (!canCommit(request, siteKey, isCancelled())) return;
      const nextItems = Array.isArray(category?.list) ? category.list : [];
      const config = await readConfigValues(nextItems, siteKey);
      if (!canCommit(request, siteKey, isCancelled())) return;
      setItems(nextItems);
      setValues(config.values);
      setState("ready");
      if (config.failures) setError(`${config.failures} 项配置值读取失败，相关控件已停用。`);
    } catch (nextError) {
      if (!canCommit(request, siteKey, isCancelled())) return;
      setItems([]);
      setValues({});
      setState("error");
      setError(errorText(nextError));
    }
  }

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      requestRef.current += 1;
      const sessionId = authSessionRef.current;
      authSessionRef.current = "";
      if (sessionId) void invokeSpider("authCancel", { sessionId }, siteKeyRef.current);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    siteGenerationRef.current += 1;
    setOperationBusy(false);
    setAuthBusy(false);
    setAuth(null);
    void loadInitial(() => cancelled);
    return () => {
      cancelled = true;
      requestRef.current += 1;
    };
  }, [site.key]);

  useEffect(() => {
    const sessionId = auth?.sessionId;
    if (!sessionId || auth.state !== "pending") return;
    let cancelled = false;
    let timer = 0;

    const poll = async () => {
      try {
        const result = await invokeSpider<CloudAuthResponse>("authPoll", { sessionId }, site.key);
        if (cancelled || !mountedRef.current || authSessionRef.current !== sessionId) return;
        const nextState = result.state === "success"
          ? "success"
          : result.state === "expired"
            ? "expired"
            : result.state === "error"
              ? "error"
              : "pending";
        setAuth((current) => current?.sessionId === sessionId ? {
          ...current,
          message: result.account || result.message || current.message,
          state: nextState,
        } : current);
        if (nextState === "success") {
          authSessionRef.current = "";
          if (selectedClass) await loadCategory(selectedClass);
          else await loadInitial();
          return;
        }
        if (nextState === "expired" || nextState === "error") {
          authSessionRef.current = "";
          return;
        }
        timer = window.setTimeout(() => void poll(), 3000);
      } catch (nextError) {
        if (!cancelled && mountedRef.current && authSessionRef.current === sessionId) {
          setAuth((current) => current?.sessionId === sessionId ? {
            ...current,
            message: errorText(nextError),
            state: "error",
          } : current);
          authSessionRef.current = "";
        }
      }
    };

    timer = window.setTimeout(() => void poll(), 3000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [auth?.sessionId, auth?.state, site.key]);

  async function saveValue(key: string, value: string) {
    if (operationBusy) return;
    const siteKey = site.key;
    const siteGeneration = siteGenerationRef.current;
    const classItem = selectedClass;
    setOperationBusy(true);
    setError("");
    try {
      await invokeSpider("configSet", { key, value }, siteKey);
      if (!mountedRef.current
        || siteKeyRef.current !== siteKey
        || siteGenerationRef.current !== siteGeneration) return;
      if (classItem) await loadCategory(classItem);
      else await loadInitial();
    } catch (nextError) {
      if (mountedRef.current
        && siteKeyRef.current === siteKey
        && siteGenerationRef.current === siteGeneration) {
        setError(errorText(nextError));
      }
    } finally {
      if (mountedRef.current
        && siteKeyRef.current === siteKey
        && siteGenerationRef.current === siteGeneration) {
        setOperationBusy(false);
      }
    }
  }

  async function startAuth(provider: CloudAuthProvider) {
    if (authBusy || operationBusy) return;
    setAuthBusy(true);
    setError("");
    const previous = authSessionRef.current;
    authSessionRef.current = "";
    if (previous) await invokeSpider("authCancel", { sessionId: previous }, site.key).catch(() => undefined);
    try {
      const result = await invokeSpider<CloudAuthResponse>("authStart", { provider }, site.key);
      const sessionId = result.sessionId?.trim() || "";
      if (!sessionId) throw new Error("登录服务未返回会话编号");
      let qrImage = result.qrImage || "";
      if (!qrImage && result.qrText) {
        qrImage = await QRCode.toDataURL(result.qrText, {
          errorCorrectionLevel: "M",
          margin: 1,
          width: 256,
        });
      }
      if (!qrImage) throw new Error("登录服务未返回二维码");
      authSessionRef.current = sessionId;
      setAuth({
        message: result.message || "等待扫码",
        provider,
        qrImage,
        sessionId,
        state: "pending",
      });
    } catch (nextError) {
      setError(errorText(nextError));
      setAuth(null);
    } finally {
      setAuthBusy(false);
    }
  }

  async function cancelAuth() {
    const sessionId = authSessionRef.current || auth?.sessionId || "";
    authSessionRef.current = "";
    setAuth(null);
    if (sessionId) await invokeSpider("authCancel", { sessionId }, site.key).catch(() => undefined);
  }

  async function clearAuth(provider: CloudAuthProvider) {
    if (authBusy || operationBusy) return;
    setAuthBusy(true);
    setError("");
    try {
      await invokeSpider("authClear", { provider }, site.key);
      if (selectedClass) await loadCategory(selectedClass);
      else await loadInitial();
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setAuthBusy(false);
    }
  }

  function refreshCurrent() {
    if (selectedClass) return loadCategory(selectedClass);
    return loadInitial();
  }

  const busy = state === "loading" || operationBusy || authBusy || auth?.state === "pending";
  const status = state === "loading"
    ? "加载中"
    : state === "error"
      ? "加载失败"
      : `${items.length} 项`;

  return (
    <section className="config-center-content">
      <div className="section-heading compact">
        <div>
          <h2>{selectedClass?.type_name || "配置项目"}</h2>
          <p>{site.name} · {status}</p>
        </div>
        <button className="icon-button" disabled={busy} type="button" title="刷新当前分类" aria-label="刷新当前分类" onClick={() => void refreshCurrent()}>
          <RefreshCw className={state === "loading" ? "spin" : ""} size={17} strokeWidth={1.8} />
        </button>
      </div>

      {classes.length > 0 && (
        <div className="config-center-tabs" role="tablist" aria-label="配置分类">
          {classes.map((item, index) => {
            const active = item === selectedClass
              || (item.type_id != null && item.type_id === selectedClass?.type_id);
            return (
              <button
                aria-selected={active}
                className={active ? "active" : ""}
                disabled={busy}
                key={`${item.type_id || "class"}-${item.type_name || index}`}
                role="tab"
                type="button"
                onClick={() => void loadCategory(item)}
              >
                {item.type_name || item.type_id || "分类"}
              </button>
            );
          })}
        </div>
      )}

      {error && <div className="form-error" role="alert">{error}</div>}

      {auth && (
        <div className={`cloud-auth-panel ${auth.state}`} role="status">
          <div className="cloud-auth-qr">
            <img alt={`${cloudAuthLabels[auth.provider]}登录二维码`} src={auth.qrImage} />
          </div>
          <div className="cloud-auth-copy">
            <span>{cloudAuthLabels[auth.provider]}网盘</span>
            <strong>{auth.state === "success" ? "登录成功" : auth.state === "pending" ? "等待扫码" : "登录未完成"}</strong>
            <p>{auth.message}</p>
            <div className="cloud-auth-actions">
              {auth.state === "pending" ? (
                <button className="secondary-button" type="button" onClick={() => void cancelAuth()}>取消</button>
              ) : (
                <>
                  <button className="secondary-button" type="button" onClick={() => setAuth(null)}>关闭</button>
                  {auth.state !== "success" && (
                    <button className="command-button" type="button" onClick={() => void startAuth(auth.provider)}>
                      <RefreshCw size={16} />重新生成
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {state === "loading" && (
        <div className="config-center-state"><RefreshCw className="spin" size={20} />正在加载配置</div>
      )}

      {state === "ready" && items.length > 0 && (
        <div className="config-center-grid">
          {items.map((item, index) => {
            const action = typeof item.action === "string" ? item.action : "";
            const contract = classifyConfigAction(action);
            const picture = posterUrl(item.vod_pic);
            let control: React.ReactNode;

            if (contract.kind === "select") {
              const loaded = Object.prototype.hasOwnProperty.call(values, contract.key);
              const current = loaded ? values[contract.key] : "";
              const known = contract.options.some((option) => option === current);
              control = (
                <select
                  aria-label={item.vod_name || contract.key}
                  disabled={busy || !loaded}
                  value={current}
                  onChange={(event) => void saveValue(contract.key, event.currentTarget.value)}
                >
                  {!loaded && <option value="">读取失败</option>}
                  {loaded && !known && <option value={current}>{current || "未设置"}</option>}
                  {contract.options.map((option) => <option key={option} value={option}>{option}</option>)}
                </select>
              );
            } else if (contract.kind === "pan-block") {
              const loaded = Object.prototype.hasOwnProperty.call(values, "panBlock");
              const enabled = loaded && !parsePanBlock(values.panBlock).includes(contract.provider);
              control = (
                <label className="config-checkbox">
                  <input
                    aria-label={`启用${contract.provider}`}
                    checked={enabled}
                    disabled={busy || !loaded}
                    type="checkbox"
                    onChange={(event) => void saveValue(
                      "panBlock",
                      togglePanBlock(values.panBlock, contract.provider, event.currentTarget.checked),
                    )}
                  />
                  <span>{loaded ? enabled ? "已启用" : "已屏蔽" : "读取失败"}</span>
                </label>
              );
            } else if (contract.kind === "pan-order") {
              const loaded = Object.prototype.hasOwnProperty.call(values, contract.key);
              const order = loaded ? normalizePanOrder(values[contract.key]) : [];
              control = loaded ? (
                <div className="pan-order-list">
                  {order.map((provider, providerIndex) => (
                    <div className="pan-order-row" key={`${provider}-${providerIndex}`}>
                      <span title={provider}>{provider}</span>
                      <button
                        className="icon-button"
                        disabled={busy || providerIndex === 0}
                        title={`上移 ${provider}`}
                        aria-label={`上移 ${provider}`}
                        type="button"
                        onClick={() => void saveValue(
                          contract.key,
                          movePanOrder(order, providerIndex, -1).join(","),
                        )}
                      >
                        <ArrowUp size={15} strokeWidth={1.8} />
                      </button>
                      <button
                        className="icon-button"
                        disabled={busy || providerIndex === order.length - 1}
                        title={`下移 ${provider}`}
                        aria-label={`下移 ${provider}`}
                        type="button"
                        onClick={() => void saveValue(
                          contract.key,
                          movePanOrder(order, providerIndex, 1).join(","),
                        )}
                      >
                        <ArrowDown size={15} strokeWidth={1.8} />
                      </button>
                    </div>
                  ))}
                </div>
              ) : <button className="config-unsupported" disabled type="button">配置值读取失败</button>;
            } else if (contract.kind === "auth-login") {
              const loggedIn = Boolean(item.vod_remarks && !item.vod_remarks.includes("未登录") && !item.vod_remarks.includes("点击"));
              control = (
                <button
                  className="config-auth-button"
                  disabled={busy || Boolean(auth)}
                  type="button"
                  onClick={() => void startAuth(contract.provider)}
                >
                  <QrCode size={16} strokeWidth={1.8} />{loggedIn ? "重新登录" : "扫码登录"}
                </button>
              );
            } else if (contract.kind === "auth-clear") {
              control = (
                <button
                  className="config-clear-button"
                  disabled={busy}
                  type="button"
                  onClick={() => void clearAuth(contract.provider)}
                >
                  <Trash2 size={15} strokeWidth={1.8} />清除
                </button>
              );
            } else {
              control = <button className="config-unsupported" disabled type="button">桌面端暂不支持此操作</button>;
            }

            return (
              <article
                className={contract.kind === "pan-order" ? "config-entry wide" : "config-entry"}
                key={`${action || item.vod_id || item.vod_name || "config"}-${index}`}
              >
                <div className="config-entry-poster" title={item.vod_pic || undefined}>
                  {picture
                    ? <img alt="" loading="lazy" src={picture} />
                    : <Settings aria-hidden="true" size={21} strokeWidth={1.6} />}
                </div>
                <div className="config-entry-body">
                  <div className="config-entry-copy">
                    <strong title={item.vod_name || ""}>{item.vod_name || "未命名配置"}</strong>
                    <small title={item.vod_remarks || ""}>{item.vod_remarks || ""}</small>
                  </div>
                  <code title={action}>{action || "未提供 action"}</code>
                  <div className="config-entry-control">{control}</div>
                </div>
              </article>
            );
          })}
        </div>
      )}

      {state === "ready" && !items.length && (
        <div className="config-center-state">
          {classes.length ? "当前分类暂无配置条目" : "配置中心未返回分类"}
        </div>
      )}
    </section>
  );
}

export default App;
