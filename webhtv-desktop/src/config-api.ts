import { invoke } from "@tauri-apps/api/core";

export interface SiteConfig {
  key: string;
  name: string;
  type: number;
  api: string;
  ext: unknown;
  jar: string;
  click: string;
  playUrl: string;
  homePage: string;
  chromeMode: string;
  webHomeChrome: unknown;
  extensions: unknown;
  hide: number;
  indexs: number;
  timeout: number;
  searchable: number;
  changeable: number;
  quickSearch: number;
  categories: string[];
  header: unknown;
  style?: { type: string; ratio: number };
}

export interface VodConfigDocument {
  spider: string;
  sites: SiteConfig[];
  parses: unknown[];
  flags: string[];
  lives: unknown;
  wallpaper: string;
  logo: string;
  notice: string;
  danmaku: string;
  home: string;
  parse: string;
}

export interface ConfigSummary {
  id: number;
  url: string;
  name: string;
  desc: string;
  siteCount: number;
  active: boolean;
  homeKey: string;
  parseName: string;
  logo: string;
  notice: string;
  updatedAt: number;
}

export interface ConfigDetail {
  summary: ConfigSummary;
  document: VodConfigDocument;
  homeSite: SiteConfig | null;
}

export function listConfigs() {
  return invoke<ConfigSummary[]>("config_list");
}

export function getActiveConfig() {
  return invoke<ConfigDetail | null>("config_active");
}

export function loadConfigUrl(url: string, name?: string) {
  return invoke<ConfigDetail>("config_load_url", { url, name: name || null });
}

export function importConfigJson(json: string, name?: string) {
  return invoke<ConfigDetail>("config_import_json", { json, name: name || null });
}

export function importConfigFile(path: string, name?: string) {
  return invoke<ConfigDetail>("config_import_file", { path, name: name || null });
}

export function activateConfig(id: number) {
  return invoke<ConfigDetail>("config_activate", { id });
}

export function selectHomeSite(configId: number, siteKey: string) {
  return invoke<ConfigDetail>("config_select_home", { configId, siteKey });
}

export function deleteConfig(id: number) {
  return invoke<void>("config_delete", { id });
}
