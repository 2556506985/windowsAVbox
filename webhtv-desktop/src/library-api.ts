import { invoke } from "@tauri-apps/api/core";

export interface LibraryItem {
  id: number;
  siteKey: string;
  siteName: string;
  vodId: string;
  vodName: string;
  vodPic: string;
  vodRemarks: string;
  updatedAt: number;
}

export interface LibraryPayload {
  siteKey: string;
  siteName?: string;
  vodId: string;
  vodName?: string;
  vodPic?: string;
  vodRemarks?: string;
}

export function listKeeps() {
  return invoke<LibraryItem[]>("library_list_keeps");
}

export function listHistory() {
  return invoke<LibraryItem[]>("library_list_history");
}

export function isKept(siteKey: string, vodId: string) {
  return invoke<boolean>("library_is_kept", { siteKey, vodId });
}

export function addKeep(item: LibraryPayload) {
  return invoke<LibraryItem>("library_add_keep", { item });
}

export function removeKeep(siteKey: string, vodId: string) {
  return invoke<void>("library_remove_keep", { siteKey, vodId });
}

export function addHistory(item: LibraryPayload) {
  return invoke<LibraryItem>("library_add_history", { item });
}

export function removeHistory(siteKey: string, vodId: string) {
  return invoke<void>("library_remove_history", { siteKey, vodId });
}

export function clearHistory() {
  return invoke<void>("library_clear_history");
}
