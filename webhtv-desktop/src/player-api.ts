import { invoke } from "@tauri-apps/api/core";

export interface PlayerOpenRequest {
  url: string;
  title?: string;
  headers?: Record<string, string>;
  start?: number;
  parse?: number;
  jx?: number;
  playUrl?: string;
  flag?: string;
  siteKey?: string;
  click?: string;
}

export interface PlayerStatus {
  ready: boolean;
  idle: boolean;
  paused: boolean;
  external: boolean;
  title: string;
  url: string;
  time: number;
  duration: number;
  volume: number;
  speed: number;
  apiVersion: string;
  dllPath: string;
}

export interface PlayerResolveResponse {
  url: string;
  headers: Record<string, string>;
  kind: string;
}

export interface PlayerProgressKey {
  siteKey: string;
  vodId: string;
  episodeUrl: string;
}

export interface PlayerProgress extends PlayerProgressKey {
  position: number;
  duration: number;
}

export interface PlayerSurfaceBounds {
  x: number;
  y: number;
  width: number;
  height: number;
  visible?: boolean;
}

export type PlayerControl = "togglePause" | "pause" | "resume" | "stop" | "fullscreen" | "seek" | "volume" | "speed";

export function openPlayer(request: PlayerOpenRequest) {
  return invoke<PlayerStatus>("player_open", { request });
}

export function attachPlayerSurface() {
  return invoke<void>("player_surface_attach");
}

export function updatePlayerSurface(bounds: PlayerSurfaceBounds) {
  return invoke<void>("player_surface_update", { bounds });
}

export function detachPlayerSurface() {
  return invoke<void>("player_surface_detach");
}

export function resolvePlayer(request: PlayerOpenRequest) {
  return invoke<PlayerResolveResponse>("player_resolve", { request });
}

export function getPlayerProgress(request: PlayerProgressKey) {
  return invoke<{ position: number; duration: number } | null>("player_progress_get", { request });
}

export function setPlayerProgress(request: PlayerProgressKey & Pick<PlayerProgress, "position" | "duration">) {
  return invoke<void>("player_progress_set", { request });
}

export function controlPlayer(command: PlayerControl, value?: number) {
  return invoke<PlayerStatus>("player_control", { request: { command, value } });
}

export function playerStatus() {
  return invoke<PlayerStatus>("player_status");
}

export function closePlayer() {
  return invoke<void>("player_close");
}
