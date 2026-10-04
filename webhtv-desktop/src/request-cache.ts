import { invokeSpider, type SpiderMethod } from "./spider-api";

interface CacheEntry<T> {
  promise: Promise<T>;
  timestamp: number;
  abortController: AbortController;
}

interface RequestOptions {
  method: SpiderMethod;
  args: Record<string, unknown>;
  siteKey?: string;
  priority?: "high" | "normal" | "low";
  dedupe?: boolean;
  cacheMs?: number;
  persistMs?: number;
  swr?: boolean;
}

interface PersistentEntry {
  value: unknown;
  savedAt: number;
}

const CACHE = new Map<string, CacheEntry<unknown>>();
const DEFAULT_CACHE_MS = 5 * 60 * 1000;
const MAX_CONCURRENT_HIGH = 3;
const MAX_CONCURRENT_NORMAL = 2;
const PERSIST_KEY = "webhtv.persistent-cache.v1";
const PERSIST_MAX_ENTRIES = 200;

function loadPersistent(): Map<string, PersistentEntry> {
  const entries = new Map<string, PersistentEntry>();
  try {
    const raw = window.localStorage.getItem(PERSIST_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Record<string, PersistentEntry>;
      for (const [key, entry] of Object.entries(parsed)) entries.set(key, entry);
    }
  } catch {
    // storage unavailable or corrupted: fall back to memory-only caching
  }
  return entries;
}

function savePersistent(entries: Map<string, PersistentEntry>): void {
  try {
    if (entries.size > PERSIST_MAX_ENTRIES) {
      const oldest = [...entries.entries()]
        .sort((a, b) => a[1].savedAt - b[1].savedAt)
        .slice(0, entries.size - PERSIST_MAX_ENTRIES);
      for (const [key] of oldest) entries.delete(key);
    }
    const record: Record<string, PersistentEntry> = {};
    for (const [key, entry] of entries) record[key] = entry;
    window.localStorage.setItem(PERSIST_KEY, JSON.stringify(record));
  } catch {
    // quota exceeded or storage unavailable: drop persistence silently
  }
}

let runningHigh = 0;
let runningNormal = 0;
const queueHigh: Array<() => void> = [];
const queueNormal: Array<() => void> = [];
const refreshing = new Set<string>();
// Throttle: don't trigger another background refresh within N ms of the last one.
const lastRefreshAt = new Map<string, number>();
const REFRESH_THROTTLE_MS = 60 * 1000;

function cacheKey(options: RequestOptions): string {
  return `${options.siteKey || "default"}:${options.method}:${JSON.stringify(options.args)}`;
}

function refreshStale(key: string, options: RequestOptions): void {
  if (refreshing.has(key)) return;
  const last = lastRefreshAt.get(key) ?? 0;
  if (Date.now() - last < REFRESH_THROTTLE_MS) return;
  refreshing.add(key);
  lastRefreshAt.set(key, Date.now());
  const execute = async (): Promise<void> => {
    try {
      let attempt = 0;
      const maxAttempts = 3;
      while (attempt < maxAttempts) {
        try {
          const value = await invokeSpider(options.method, options.args, options.siteKey);
          if ((options.persistMs ?? 0) > 0) {
            const entries = loadPersistent();
            entries.set(key, { value, savedAt: Date.now() });
            savePersistent(entries);
          }
          CACHE.set(key, {
            promise: Promise.resolve(value),
            timestamp: Date.now(),
            abortController: new AbortController(),
          });
          console.debug(`[SWR] refresh success key=${key} attempt=${attempt + 1}`);
          return;
        } catch (error) {
          attempt++;
          if (attempt < maxAttempts) {
            const delay = Math.pow(2, attempt) * 1000;
            console.warn(`[SWR] refresh failed key=${key} attempt=${attempt}/${maxAttempts}, retry in ${delay}ms:`, error);
            await new Promise(r => setTimeout(r, delay));
          } else {
            console.error(`[SWR] refresh failed permanently key=${key} after ${maxAttempts} attempts:`, error);
          }
        }
      }
    } finally {
      refreshing.delete(key);
    }
  };
  if (options.priority === "high") {
    void runHigh(execute);
  } else {
    void runNormal(execute);
  }
}

function runHigh<T>(fn: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const run = async () => {
      runningHigh++;
      try {
        const result = await fn();
        resolve(result);
      } catch (e) {
        reject(e);
      } finally {
        runningHigh--;
        if (queueHigh.length) queueHigh.shift()!();
      }
    };
    if (runningHigh < MAX_CONCURRENT_HIGH) {
      run();
    } else {
      queueHigh.push(run);
    }
  });
}

function runNormal<T>(fn: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const run = async () => {
      runningNormal++;
      try {
        const result = await fn();
        resolve(result);
      } catch (e) {
        reject(e);
      } finally {
        runningNormal--;
        if (queueNormal.length) queueNormal.shift()!();
      }
    };
    if (runningNormal < MAX_CONCURRENT_NORMAL) {
      run();
    } else {
      queueNormal.push(run);
    }
  });
}

export async function cachedInvoke<T = unknown>(
  options: RequestOptions
): Promise<T> {
  const key = cacheKey(options);
  const cacheMs = options.cacheMs ?? DEFAULT_CACHE_MS;
  const persistMs = options.persistMs ?? 0;
  const now = Date.now();

  if (options.dedupe !== false) {
    const existing = CACHE.get(key);
    if (existing && now - existing.timestamp < cacheMs) {
      if (options.swr) refreshStale(key, options);
      return existing.promise as Promise<T>;
    }
    if (persistMs > 0) {
      const persisted = loadPersistent().get(key);
      if (persisted && now - persisted.savedAt < persistMs) {
        if (options.swr) refreshStale(key, options);
        const promise = Promise.resolve(persisted.value as T);
        CACHE.set(key, { promise, timestamp: now, abortController: new AbortController() });
        return promise;
      }
    }
  }

  const abortController = new AbortController();
  const execute = async (): Promise<T> => {
    try {
      return await invokeSpider<T>(options.method, options.args, options.siteKey);
    } catch (error) {
      abortController.abort();
      throw error;
    }
  };

  const promise = (options.priority === "high" ? runHigh : runNormal)(execute);

  if (persistMs > 0) {
    promise
      .then((value) => {
        const entries = loadPersistent();
        entries.set(key, { value, savedAt: Date.now() });
        savePersistent(entries);
      })
      .catch(() => {
        // failed requests are never persisted; stale entries stay untouched
      });
  }

  const entry: CacheEntry<T> = { promise, timestamp: now, abortController };
  CACHE.set(key, entry);

  try {
    return await promise;
  } catch (error) {
    CACHE.delete(key);
    throw error;
  }
}

export function cancelPending(key?: string): void {
  if (key) {
    const entry = CACHE.get(key);
    if (entry) {
      entry.abortController.abort();
      CACHE.delete(key);
    }
  } else {
    for (const entry of CACHE.values()) {
      entry.abortController.abort();
    }
    CACHE.clear();
  }
}

export function clearCache(pattern?: string): void {
  const persisted = loadPersistent();
  if (!pattern) {
    CACHE.clear();
    savePersistent(new Map());
    return;
  }
  for (const key of CACHE.keys()) {
    if (key.includes(pattern)) CACHE.delete(key);
  }
  let changed = false;
  for (const key of persisted.keys()) {
    if (key.includes(pattern)) {
      persisted.delete(key);
      changed = true;
    }
  }
  if (changed) savePersistent(persisted);
}

export function getCacheStats(): { size: number; keys: string[] } {
  return { size: CACHE.size, keys: Array.from(CACHE.keys()) };
}

export function isCached(
  method: SpiderMethod,
  args: Record<string, unknown>,
  siteKey?: string,
  withinMs?: number
): boolean {
  const key = cacheKey({ method, args, siteKey });
  const now = Date.now();
  const entry = CACHE.get(key);
  if (entry && now - entry.timestamp < (withinMs ?? DEFAULT_CACHE_MS)) return true;
  if (withinMs === undefined) return false;
  const persisted = loadPersistent().get(key);
  return persisted !== undefined && now - persisted.savedAt < withinMs;
}

export function createAbortSignal(): AbortSignal {
  return new AbortController().signal;
}