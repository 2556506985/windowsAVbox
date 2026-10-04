import { Component, ReactNode } from "react";
import { RefreshCw, WifiOff } from "lucide-react";

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
  errorInfo: React.ErrorInfo | null;
}

export class ErrorBoundary extends Component<
  { children: ReactNode; fallback?: ReactNode; onReset?: () => void },
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { hasError: false, error: null, errorInfo: null };

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    this.setState({ error, errorInfo });
    console.error("ErrorBoundary caught:", error, errorInfo);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null, errorInfo: null });
    this.props.onReset?.();
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback;
      return (
        <div className="error-boundary" role="alert">
          <div className="error-boundary-content">
            <WifiOff size={32} strokeWidth={1.5} />
            <h3>出错了</h3>
            <p>{this.state.error?.message || "未知错误"}</p>
            <details className="error-details">
              <summary>详细信息</summary>
              <pre>{this.state.error?.stack}</pre>
            </details>
            <button className="command-button" onClick={this.handleReset} type="button">
              <RefreshCw size={16} strokeWidth={1.8} /> 重试
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

export function withErrorBoundary<P extends object>(
  WrappedComponent: React.ComponentType<P>,
  fallback?: ReactNode,
  onReset?: () => void,
) {
  return function WithErrorBoundary(props: P) {
    return (
      <ErrorBoundary fallback={fallback} onReset={onReset}>
        <WrappedComponent {...props} />
      </ErrorBoundary>
    );
  };
}

interface RetryOptions {
  maxRetries?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
  shouldRetry?: (error: unknown) => boolean;
}

export function withRetry<T extends (...args: any[]) => Promise<any>>(
  fn: T,
  options: RetryOptions = {},
): T {
  const { maxRetries = 3, baseDelayMs = 500, maxDelayMs = 8000, shouldRetry = () => true } = options;

  const retriedFn = async (...args: Parameters<T>): Promise<Awaited<ReturnType<T>>> => {
    let lastError: unknown;
    for (let attempt = 0; attempt <= maxRetries; attempt++) {
      try {
        return await fn(...args);
      } catch (error) {
        lastError = error;
        if (attempt === maxRetries || !shouldRetry(error)) break;
        const delay = Math.min(baseDelayMs * 2 ** attempt + Math.random() * 100, maxDelayMs);
        await new Promise((r) => setTimeout(r, delay));
      }
    }
    throw lastError;
  };

  return retriedFn as T;
}

export function isNetworkError(error: unknown): boolean {
  if (error instanceof TypeError && error.message.includes("Network")) return true;
  if (error instanceof Response && !error.ok) return true;
  if (error && typeof error === "object" && "code" in error) {
    const code = (error as { code?: string }).code;
    if (code === "ECONNREFUSED" || code === "ETIMEDOUT" || code === "ENOTFOUND") return true;
  }
  // Tauri commands reject with plain string hints from Rust — match transient language there too.
  if (typeof error === "string") {
    const msg = error.toLowerCase();
    return (
      msg.includes("timed out") ||
      msg.includes("timeout") ||
      msg.includes("network") ||
      msg.includes("failed to fetch") ||
      msg.includes("econnrefused") ||
      msg.includes("disconnected") ||
      msg.includes("no longer available") ||
      msg.includes("stopped before replying")
    );
  }
  return false;
}