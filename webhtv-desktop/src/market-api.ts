import { invoke } from "@tauri-apps/api/core";

export interface MarketItem {
  name: string;
  url: string;
  version: string;
  icon: string;
}

export interface MarketCategory {
  name: string;
  list: MarketItem[];
}

export interface MarketInstallResult {
  configName: string;
  version: string;
  mode: "replace" | "import";
}

export function marketCatalog(url: string) {
  return invoke<MarketCategory[]>("market_catalog", { url });
}

export function marketInstall(url: string) {
  return invoke<MarketInstallResult>("market_install", { url });
}

export function appRestart() {
  return invoke<void>("app_restart");
}
