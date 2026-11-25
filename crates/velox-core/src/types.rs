use serde::{Deserialize, Serialize};

use crate::category::Category;

pub type DownloadId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Connecting,
    Downloading,
    Merging,
    Paused,
    Completed,
    Failed,
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Status::Connecting | Status::Downloading | Status::Merging)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Http,
    Hls,
    Dash,
    /// A video-site page (YouTube, Vimeo, …) resolved through yt-dlp.
    Site,
}

impl Kind {
    pub fn detect(url: &str, content_type: Option<&str>) -> Kind {
        let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
        let ct = content_type.unwrap_or_default().to_ascii_lowercase();
        if crate::site::is_site_url(url) {
            Kind::Site
        } else if path.ends_with(".m3u8") || ct.contains("mpegurl") {
            Kind::Hls
        } else if path.ends_with(".mpd") || ct.contains("dash+xml") {
            Kind::Dash
        } else {
            Kind::Http
        }
    }
}

/// A request to add a new download (from UI, clipboard or browser extension).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AddRequest {
    pub url: String,
    pub file_name: Option<String>,
    /// Target directory. `None` = settings default (+ category folder).
    pub dir: Option<String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub headers: Vec<(String, String)>,
    pub kind: Option<Kind>,
    pub connections: Option<u16>,
    /// Bytes per second, 0 = unlimited.
    pub speed_limit: Option<u64>,
    /// Add to the list without starting it.
    pub start_paused: bool,
    /// Size already known from a probe – avoids an extra request.
    pub size_hint: Option<u64>,
    /// Video sites: "1080", "720", … or "audio". `None` = best.
    pub quality: Option<String>,
}

/// A byte range of a segmented download. Offsets are absolute.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    /// Exclusive.
    pub end: u64,
    /// Bytes `start..written` are on disk.
    pub written: u64,
    /// A connection is currently working on this range (live view only).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
}

impl Segment {
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.written)
    }
    pub fn done(&self) -> bool {
        self.written >= self.end
    }
}

/// Persisted record for a download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: DownloadId,
    pub url: String,
    pub kind: Kind,
    pub file_name: String,
    pub dir: String,
    pub category: Category,
    pub total: Option<u64>,
    pub downloaded: u64,
    pub status: Status,
    pub resumable: bool,
    pub segments: Vec<Segment>,
    /// HLS: number of media segments already appended.
    #[serde(default)]
    pub hls_done: u64,
    #[serde(default)]
    pub hls_count: u64,
    /// HLS: bytes actually written to the part file (resume point).
    #[serde(default)]
    pub hls_bytes: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub connections: u16,
    pub speed_limit: u64,
    #[serde(default)]
    pub quality: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

impl Record {
    pub fn path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.dir).join(&self.file_name)
    }
}

/// Live snapshot sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    #[serde(flatten)]
    pub record: Record,
    /// Bytes per second (smoothed).
    pub speed: f64,
    /// Seconds remaining.
    pub eta: Option<f64>,
    pub active_connections: u16,
}

/// Lightweight high-frequency progress update.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: DownloadId,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub eta: Option<f64>,
    pub active_connections: u16,
    pub segments: Vec<Segment>,
    pub status: Status,
}

/// Result of probing a URL before the user confirms a download.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub url: String,
    pub final_url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub kind: Kind,
    pub category: Category,
    pub dir: String,
    /// Video sites: available qualities, best first.
    pub qualities: Vec<crate::site::QualityOption>,
}

/// What a captured stream is, so the browser can label it ("1080p · 39:47")
/// and hide duplicates instead of showing raw playlist file names.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    /// The URL answered with a playable playlist.
    pub ok: bool,
    /// HLS master playlist (several qualities / renditions).
    pub master: bool,
    /// Tallest video height offered.
    pub height: Option<u32>,
    /// Seconds, when the playlist is complete (VOD).
    pub duration: Option<f64>,
    pub qualities: Vec<crate::site::QualityOption>,
    /// Variant and rendition playlists a master points to (absolute URLs).
    pub children: Vec<String>,
    /// Same value for playlists with the same content (other tokens / mirrors).
    pub fingerprint: String,
}
