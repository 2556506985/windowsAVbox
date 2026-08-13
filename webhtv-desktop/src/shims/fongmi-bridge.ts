import { invoke as tauriInvoke } from "@tauri-apps/api/core";

type Callback = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
};

interface ResourceOptions {
  credentials?: string;
  headers?: unknown;
}

declare global {
  interface Window {
    fongmiBridge: {
      invoke(requestId: string, method: string, payload: string): void;
      console(level: string, message: string): void;
      network(type: string, method: string, url: string, status: number, durationMs: number, detail: string): void;
      resourceUrl(url: string, options: string): string;
      resultLength(id: string): number;
      resultChunk(id: string, start: number): string;
      clearResult(id: string): void;
      inlineResult(id: string, payload: string): void;
    };
    fongmiNative: {
      resolve(id: string, data: unknown): void;
      reject(id: string, error: string): void;
    };
  }
}

const callbacks = new Map<string, Callback>();
const largeResults = new Map<string, string>();
let localServerBase = "";
let sequence = 0;

export function setBridgeRuntimeConfig(config: { localServerBase?: string | null }) {
  localServerBase = (config.localServerBase || "").replace(/\/$/, "");
}

export function installFongmiBridge() {
  window.fongmiNative = {
    resolve(id, data) {
      const callback = callbacks.get(id);
      if (!callback) return;
      callbacks.delete(id);
      callback.resolve(data);
    },
    reject(id, error) {
      const callback = callbacks.get(id);
      if (!callback) return;
      callbacks.delete(id);
      callback.reject(new Error(error || "Bridge request failed"));
    },
  };

  window.fongmiBridge = {
    invoke(requestId, method, payload) {
      void tauriInvoke("bridge_invoke", { requestId, method, payload })
        .then((data) => window.fongmiNative.resolve(requestId, data))
        .catch((error: unknown) => {
          const message = error instanceof Error ? error.message : String(error);
          window.fongmiNative.reject(requestId, message);
        });
    },
    console(level, message) {
      void tauriInvoke("bridge_console", { level, message });
    },
    network(type, method, url, status, durationMs, detail) {
      void tauriInvoke("bridge_network", { type, method, url, status, durationMs, detail });
    },
    resourceUrl(url, options) {
      if (!localServerBase) return url;
      const query = new URLSearchParams({ url });
      try {
        const parsed = JSON.parse(options || "{}") as ResourceOptions;
        if (parsed.headers !== undefined) query.set("headers", JSON.stringify(parsed.headers));
        if (parsed.credentials === "include") query.set("credentials", "include");
      } catch {
        // A malformed options object must not make the original resource unusable.
      }
      return `${localServerBase}/webResource?${query.toString()}`;
    },
    resultLength(id) {
      return largeResults.get(id)?.length || 0;
    },
    resultChunk(id, start) {
      return (largeResults.get(id) || "").slice(start, start + 60000);
    },
    clearResult(id) {
      largeResults.delete(id);
    },
    inlineResult(id, payload) {
      void tauriInvoke("bridge_inline_result", { id, payload });
    },
  };
}

export function callBridge<T = unknown>(method: string, payload: Record<string, unknown>): Promise<T> {
  const id = `fm_desktop_${Date.now()}_${++sequence}`;
  return new Promise<T>((resolve, reject) => {
    callbacks.set(id, {
      resolve: (value) => resolve(value as T),
      reject,
    });
    window.fongmiBridge.invoke(id, method, JSON.stringify(payload));
  });
}
