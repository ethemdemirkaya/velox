//! External helpers: `yt-dlp` (resolves video-site pages to direct media
//! URLs) and `ffmpeg` (muxes separate audio/video streams). Velox keeps its
//! own copies in `<data dir>/tools` and falls back to the system `PATH`.

use futures_util::StreamExt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static TOOLS_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_dir(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = TOOLS_DIR.set(dir);
}

pub fn dir() -> Option<&'static Path> {
    TOOLS_DIR.get().map(PathBuf::as_path)
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Find a tool: our private copy first, then `PATH`.
pub fn find(name: &str) -> Option<PathBuf> {
    if let Some(d) = dir() {
        let p = d.join(exe(name));
        if p.is_file() {
            return Some(p);
        }
    }
    which::which(name).ok()
}

pub fn ytdlp() -> Option<PathBuf> {
    find("yt-dlp")
}

pub fn ffmpeg() -> Option<PathBuf> {
    find("ffmpeg")
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    pub ytdlp: bool,
    pub ffmpeg: bool,
    /// ffmpeg can be installed automatically on this platform.
    pub ffmpeg_installable: bool,
}

pub fn status() -> ToolsStatus {
    ToolsStatus { ytdlp: ytdlp().is_some(), ffmpeg: ffmpeg().is_some(), ffmpeg_installable: cfg!(windows) }
}

fn ytdlp_url() -> &'static str {
    if cfg!(windows) {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    }
}

pub(crate) const FFMPEG_ZIP: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip";

/// Stream `url` into `dest` (via a temp file, atomically renamed).
pub(crate) async fn fetch(client: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let tmp = dest.with_extension("download");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut s = resp.bytes_stream();
    while let Some(chunk) = s.next().await {
        f.write_all(&chunk.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    drop(f);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

pub(crate) async fn install_ytdlp(client: &reqwest::Client) -> Result<PathBuf, String> {
    let d = dir().ok_or("no tools dir")?;
    let dest = d.join(exe("yt-dlp"));
    fetch(client, ytdlp_url(), &dest).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(dest)
}

/// Pull `ffmpeg.exe` / `ffprobe.exe` out of the downloaded build archive.
pub(crate) fn extract_ffmpeg(zip_path: &Path) -> Result<(), String> {
    let d = dir().ok_or("no tools dir")?;
    let f = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut found = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let file = name.rsplit('/').next().unwrap_or("").to_string();
        if name.contains("/bin/") && (file == exe("ffmpeg") || file == exe("ffprobe")) {
            let mut out = std::fs::File::create(d.join(&file)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            found |= file == exe("ffmpeg");
        }
    }
    if found {
        Ok(())
    } else {
        Err("ffmpeg not found in archive".into())
    }
}

pub(crate) fn command(program: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    c.kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}
