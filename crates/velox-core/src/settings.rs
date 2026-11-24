use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM" local time to start the queue.
    pub start: String,
    /// "HH:MM" local time to pause everything. Empty = never.
    pub stop: String,
}

impl Default for Schedule {
    fn default() -> Self {
        Self { enabled: false, start: "02:00".into(), stop: "08:00".into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AfterQueue {
    #[default]
    Nothing,
    Shutdown,
    Sleep,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub download_dir: String,
    pub use_category_folders: bool,
    pub max_concurrent: u16,
    pub connections: u16,
    /// Global limit in bytes/s, 0 = unlimited.
    pub speed_limit: u64,
    pub language: String,
    pub confirm_new: bool,
    pub clipboard_watch: bool,
    pub autostart: bool,
    pub close_to_tray: bool,
    pub notifications: bool,
    /// Show the small per-download progress window (IDM style).
    pub progress_window: bool,
    pub capture_enabled: bool,
    pub capture_extensions: Vec<String>,
    /// Minimum size in KiB the extension captures automatically.
    pub capture_min_size_kb: u64,
    pub user_agent: String,
    /// `http://host:port` or `socks5://host:port`; empty = system/none.
    pub proxy: String,
    pub schedule: Schedule,
    pub after_queue: AfterQueue,
}

pub const MAX_CONNECTIONS: u16 = 32;

impl Default for Settings {
    fn default() -> Self {
        let download_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .map(|p| p.join("Velox").to_string_lossy().into_owned())
            .unwrap_or_else(|| "Velox".into());
        Self {
            download_dir,
            use_category_folders: true,
            max_concurrent: 3,
            connections: 16,
            speed_limit: 0,
            language: "auto".into(),
            confirm_new: true,
            clipboard_watch: true,
            autostart: false,
            close_to_tray: true,
            notifications: true,
            progress_window: true,
            capture_enabled: true,
            capture_extensions: "zip rar 7z gz xz tar iso exe msi dmg apk deb rpm mp4 mkv avi mov webm mp3 flac wav m4a pdf epub bin img"
                .split(' ')
                .map(String::from)
                .collect(),
            capture_min_size_kb: 512,
            user_agent: String::new(),
            proxy: String::new(),
            schedule: Schedule::default(),
            after_queue: AfterQueue::Nothing,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        self.max_concurrent = self.max_concurrent.clamp(1, 20);
        self
    }
}
