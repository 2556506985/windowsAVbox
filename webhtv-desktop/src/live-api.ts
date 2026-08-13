import { invoke } from "@tauri-apps/api/core";

export interface LiveSourceSummary {
  name: string;
  logo: string;
  epg: string;
  channelCount: number;
  embedded: boolean;
}

export interface LiveChannel {
  name: string;
  number: string;
  logo: string;
  tvgId: string;
  tvgName: string;
  urls: string[];
}

export interface LiveGroup {
  name: string;
  channels: LiveChannel[];
}

export interface LiveCatalog {
  source: LiveSourceSummary;
  groups: LiveGroup[];
}

export function listLiveSources() {
  return invoke<LiveSourceSummary[]>("live_sources");
}

export function loadLiveSource(sourceName: string) {
  return invoke<LiveCatalog>("live_load", { sourceName });
}
