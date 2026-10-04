import { useCallback, useEffect, useRef, useState } from "react";

type AnyFn = (...args: any[]) => unknown;

interface DebounceOptions {
  delay?: number;
  leading?: boolean;
  trailing?: boolean;
}

export function useDebouncedCallback<T extends AnyFn>(
  callback: T,
  options: DebounceOptions = {}
): T {
  const { delay = 300, leading = false, trailing = true } = options;
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastArgsRef = useRef<Parameters<T> | null>(null);
  const lastCallTimeRef = useRef<number>(0);
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  const debouncedFn = useCallback(
    ((...args: any[]) => {
      lastArgsRef.current = args as Parameters<T>;
      const now = Date.now();
      const elapsed = now - lastCallTimeRef.current;

      const invoke = () => {
        lastCallTimeRef.current = Date.now();
        if (trailing && lastArgsRef.current) {
          (callbackRef.current as AnyFn)(...lastArgsRef.current);
        }
      };

      if (leading && elapsed >= delay) {
        invoke();
        return;
      }

      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      timeoutRef.current = setTimeout(invoke, delay);
    }) as T,
    [delay, leading, trailing]
  );

  useEffect(() => {
    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

  return debouncedFn;
}

interface ClickHandlerOptions {
  debounceMs?: number;
  priority?: "high" | "normal" | "low";
  onPending?: () => void;
  onCancel?: () => void;
}

export interface ClickHandler<T extends AnyFn> {
  (...args: Parameters<T>): void;
  cancel: () => void;
  isPending: boolean;
}

export function useClickHandler<T extends AnyFn>(
  handler: T,
  options: ClickHandlerOptions = {}
): ClickHandler<T> {
  const { debounceMs = 150, onPending, onCancel } = options;
  const abortRef = useRef<AbortController | null>(null);
  const pendingRef = useRef(false);
  const [isPending, setIsPending] = useState(false);

  const wrapped = useCallback(
    async (...args: any[]) => {
      if (pendingRef.current && debounceMs > 0) return;
      pendingRef.current = true;
      setIsPending(true);
      onPending?.();

      if (abortRef.current) abortRef.current.abort();
      abortRef.current = new AbortController();

      try {
        await (handler as AnyFn)(...args);
      } catch (error) {
        if (error instanceof DOMException && error.name === "AbortError") {
          onCancel?.();
          return;
        }
        throw error;
      } finally {
        pendingRef.current = false;
        setIsPending(false);
      }
    },
    [handler, debounceMs, onPending, onCancel]
  );

  const debounced = useDebouncedCallback(wrapped, { delay: debounceMs, trailing: true });

  const result = Object.assign(debounced, {
    cancel: () => {
      abortRef.current?.abort();
      pendingRef.current = false;
      setIsPending(false);
    },
    isPending,
  });

  return result as ClickHandler<T>;
}