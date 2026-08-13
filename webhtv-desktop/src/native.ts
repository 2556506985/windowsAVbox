import { invoke } from "@tauri-apps/api/core";

export interface BootstrapState {
  appName: string;
  appVersion: string;
  bridgeVersion: number;
  mode: "desktop";
  platform: string;
  localServerBase: string | null;
  uptimeMs: number;
}

export function getBootstrapState() {
  return invoke<BootstrapState>("bootstrap");
}
