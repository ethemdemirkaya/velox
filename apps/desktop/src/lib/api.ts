import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Status = "queued" | "connecting" | "downloading" | "merging" | "paused" | "completed" | "failed";
export type Kind = "http" | "hls" | "dash" | "site";
export type Category = "video" | "music" | "documents" | "programs" | "compressed" | "images" | "other";

export interface Segment {
  start: number;
  end: number;
  written: number;
  active?: boolean;
}

export interface Download {
  id: string;
  url: string;
  kind: Kind;
  fileName: string;
  dir: string;
  category: Category;
  total: number | null;
  downloaded: number;
  status: Status;
  resumable: boolean;
  segments: Segment[];
  hlsDone: number;
  hlsCount: number;
  connections: number;
  speedLimit: number;
  error: string | null;
  createdAt: number;
  finishedAt: number | null;
  referrer: string | null;
  speed: number;
  eta: number | null;
  activeConnections: number;
}

export interface Progress {
  id: string;
  downloaded: number;
  total: number | null;
  speed: number;
  eta: number | null;
  activeConnections: number;
  segments: Segment[];
  status: Status;
}

export interface AddRequest {
  url: string;
  fileName?: string | null;
  dir?: string | null;
  referrer?: string | null;
  cookies?: string | null;
  userAgent?: string | null;
  headers?: [string, string][];
  kind?: Kind | null;
  connections?: number | null;
  speedLimit?: number | null;
  startPaused?: boolean;
  sizeHint?: number | null;
}

export interface ProbeInfo {
  url: string;
  finalUrl: string;
  fileName: string;
  size: number | null;
  resumable: boolean;
  contentType: string | null;
  kind: Kind;
  category: Category;
  dir: string;
}

export interface Settings {
  downloadDir: string;
  useCategoryFolders: boolean;
  maxConcurrent: number;
  connections: number;
  speedLimit: number;
  language: string;
  confirmNew: boolean;
  clipboardWatch: boolean;
  autostart: boolean;
  closeToTray: boolean;
  notifications: boolean;
  progressWindow: boolean;
  progressDetails: boolean;
  captureEnabled: boolean;
  captureExtensions: string[];
  captureMinSizeKb: number;
  userAgent: string;
  proxy: string;
  schedule: { enabled: boolean; start: string; stop: string };
  afterQueue: "nothing" | "shutdown" | "sleep";
}

export interface ToolsStatus {
  ytdlp: boolean;
  ffmpeg: boolean;
  ffmpegInstallable: boolean;
}

export interface AppInfo {
  version: string;
  ffmpeg: boolean;
  hostRegistered: boolean;
  extensionId: string;
  platform: string;
}

export type EngineEvent =
  | { type: "added"; data: Download }
  | { type: "updated"; data: Download }
  | { type: "removed"; data: string }
  | { type: "progress"; data: Progress[] }
  | { type: "settings"; data: Settings }
  | { type: "tools"; data: ToolsStatus }
  | { type: "queue_finished" };

export const api = {
  list: () => invoke<Download[]>("list_downloads"),
  add: (req: AddRequest) => invoke<Download>("add_download", { req }),
  probe: (req: AddRequest) => invoke<ProbeInfo>("probe_url", { req }),
  pause: (id: string) => invoke<void>("pause", { id }),
  resume: (id: string) => invoke<void>("resume", { id }),
  remove: (id: string, deleteFile: boolean) => invoke<void>("remove", { id, deleteFile }),
  pauseAll: () => invoke<void>("pause_all"),
  resumeAll: () => invoke<void>("resume_all"),
  clearCompleted: () => invoke<void>("clear_completed"),
  setLimit: (id: string, limit: number) => invoke<void>("set_download_limit", { id, limit }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  openFile: (id: string) => invoke<void>("open_file", { id }),
  showInFolder: (id: string) => invoke<void>("show_in_folder", { id }),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  getPending: (token: string) => invoke<AddRequest>("get_pending", { token }),
  resolvePending: (token: string, req: AddRequest | null) => invoke<Download | null>("resolve_pending", { token, req }),
  appInfo: () => invoke<AppInfo>("app_info"),
  checkUpdate: () => invoke<string | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  quit: () => invoke<void>("quit"),
  toolsStatus: () => invoke<ToolsStatus>("tools_status"),
  installTools: () => invoke<ToolsStatus>("install_tools"),
  showProgress: (id: string) => invoke<void>("show_progress", { id }),
  readClipboard: () => invoke<string>("read_clipboard"),
  writeClipboard: (text: string) => invoke<void>("write_clipboard", { text }),
};

export function onEngine(cb: (e: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>("engine", (e) => cb(e.payload));
}

export function onClipboard(cb: (url: string) => void): Promise<UnlistenFn> {
  return listen<string>("clipboard-url", (e) => cb(e.payload));
}
