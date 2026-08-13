import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  callBridge,
  installFongmiBridge,
  setBridgeRuntimeConfig,
} from "./fongmi-bridge";

describe("fongmi bridge shim", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    setBridgeRuntimeConfig({ localServerBase: null });
    installFongmiBridge();
  });

  it("preserves the Android callback contract over Tauri promises", async () => {
    invokeMock.mockResolvedValue({ platform: "windows" });

    await expect(callBridge("device.info", {})).resolves.toEqual({ platform: "windows" });
    expect(invokeMock).toHaveBeenCalledWith("bridge_invoke", expect.objectContaining({
      method: "device.info",
      payload: "{}",
    }));
  });

  it("rejects through fongmiNative", async () => {
    invokeMock.mockRejectedValue(new Error("not implemented"));

    await expect(callBridge("missing.method", {})).rejects.toThrow("not implemented");
  });

  it("keeps resourceUrl synchronous", () => {
    setBridgeRuntimeConfig({ localServerBase: "http://127.0.0.1:9978/" });

    const result = window.fongmiBridge.resourceUrl(
      "https://example.com/poster image.jpg",
      JSON.stringify({ credentials: "include", headers: { Referer: "https://example.com" } }),
    );

    expect(result).toContain("http://127.0.0.1:9978/webResource?");
    expect(result).toContain("credentials=include");
    expect(result).toContain("url=https%3A%2F%2Fexample.com%2Fposter+image.jpg");
  });
});
