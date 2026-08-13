import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { cancelPending, cachedInvoke, clearCache, getCacheStats } from "./request-cache";

describe("request cache", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(() => Promise.resolve({ ok: true }));
    clearCache();
  });

  afterEach(() => {
    cancelPending();
  });

  it("dedupes concurrent identical calls into one spider invoke", async () => {
    const options = {
      method: "homeContent" as const,
      args: { filter: true },
      siteKey: "demo",
      priority: "high" as const,
      dedupe: true,
    };

    await Promise.all([cachedInvoke(options), cachedInvoke(options), cachedInvoke(options)]);

    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("reuses a successful cached result within the TTL window", async () => {
    const options = {
      method: "detailContent" as const,
      args: { ids: ["1"] },
      siteKey: "demo",
      priority: "high" as const,
      cacheMs: 60_000,
    };

    const first = await cachedInvoke(options);
    invokeMock.mockReset();
    const second = await cachedInvoke(options);

    expect(first).toEqual({ ok: true });
    expect(second).toEqual({ ok: true });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("removes failed entries so the next call retries", async () => {
    const options = {
      method: "categoryContent" as const,
      args: { tid: "1", page: "1" },
      siteKey: "demo",
      priority: "high" as const,
      cacheMs: 60_000,
    };

    invokeMock.mockRejectedValueOnce(new Error("boom"));
    await expect(cachedInvoke(options)).rejects.toThrow("boom");

    invokeMock.mockResolvedValueOnce({ list: ["ok"] });
    await expect(cachedInvoke(options)).resolves.toEqual({ list: ["ok"] });
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("expires entries after the TTL", async () => {
    vi.useFakeTimers();
    try {
      const options = {
        method: "homeContent" as const,
        args: { filter: true },
        siteKey: "demo",
        priority: "high" as const,
        cacheMs: 1_000,
      };

      await cachedInvoke(options);
      vi.advanceTimersByTime(1_100);
      await cachedInvoke(options);

      expect(invokeMock).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps per-key stats for debugging", async () => {
    const options = {
      method: "searchContent" as const,
      args: { key: "电影" },
      siteKey: "demo",
      priority: "high" as const,
      dedupe: true,
    };

    await cachedInvoke(options);
    const stats = getCacheStats();

    expect(stats.size).toBe(1);
    expect(stats.keys[0]).toContain("demo");
    expect(stats.keys[0]).toContain("searchContent");
  });

  it("persists successful results and serves them after a restart", async () => {
    const options = {
      method: "detailContent" as const,
      args: { ids: ["42"] },
      siteKey: "demo",
      priority: "high" as const,
      cacheMs: 60_000,
      persistMs: 24 * 60 * 60 * 1000,
    };

    await cachedInvoke(options);
    expect(invokeMock).toHaveBeenCalledTimes(1);

    const stored = JSON.parse(window.localStorage.getItem("webhtv.persistent-cache.v1") || "{}");
    const entries = Object.values(stored) as Array<{ value: unknown; savedAt: number }>;
    expect(entries).toHaveLength(1);
    expect(entries[0].value).toEqual({ ok: true });

    cancelPending();
    invokeMock.mockReset();
    invokeMock.mockRejectedValue(new Error("spider invoke must not be called after restart"));

    const afterRestart = await cachedInvoke(options);
    expect(afterRestart).toEqual({ ok: true });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("clearCache removes persisted entries too", async () => {
    const options = {
      method: "detailContent" as const,
      args: { ids: ["7"] },
      siteKey: "demo",
      priority: "high" as const,
      persistMs: 24 * 60 * 60 * 1000,
    };

    await cachedInvoke(options);
    expect(window.localStorage.getItem("webhtv.persistent-cache.v1")).not.toBeNull();

    clearCache();
    const remaining = Object.keys(
      JSON.parse(window.localStorage.getItem("webhtv.persistent-cache.v1") || "{}")
    );
    expect(remaining).toHaveLength(0);
  });
});

