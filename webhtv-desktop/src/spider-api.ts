import { invoke } from "@tauri-apps/api/core";
import { withRetry, isNetworkError } from "./components/ErrorBoundary";

export type SpiderMethod =
  | "homeContent"
  | "homeVideoContent"
  | "categoryContent"
  | "detailContent"
  | "searchContent"
  | "playerContent"
  | "liveContent"
  | "manualVideoCheck"
  | "isVideoFormat"
  | "configSet"
  | "configGet"
  | "authStart"
  | "authPoll"
  | "authCancel"
  | "authClear"
  | "authStatus"
  | "action";

function invokeSpiderBase<T = unknown>(
  method: SpiderMethod,
  args: Record<string, unknown> = {},
  siteKey?: string,
): Promise<T> {
  return invoke<T>("spider_invoke", { siteKey: siteKey || null, method, args });
}

interface InvokeSpiderFn {
  <T = unknown>(method: SpiderMethod, args: Record<string, unknown>, siteKey?: string): Promise<T>;
}

const invokeWithRetry: InvokeSpiderFn = withRetry(invokeSpiderBase, {
  maxRetries: 3,
  baseDelayMs: 400,
  maxDelayMs: 5000,
  shouldRetry: isNetworkError,
});

export function invokeSpider<T = unknown>(
  method: SpiderMethod,
  args: Record<string, unknown> = {},
  siteKey?: string,
): Promise<T> {
  return invokeWithRetry<T>(method, args, siteKey);
}
