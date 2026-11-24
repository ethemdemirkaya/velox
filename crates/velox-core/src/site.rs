//! Video-site downloads (YouTube, Vimeo, X, …).
//!
//! `yt-dlp` only *resolves* the page into direct media URLs (best video +
//! best audio). The bytes are then fetched by Velox's own segmented engine –
//! many connections instead of one – and muxed losslessly with ffmpeg.

use parking_lot::Mutex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::http::{self, DlError, Live};
use crate::tools;

/// Hosts handled through yt-dlp (page URLs, not media URLs).
pub fn is_site_url(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").trim_start_matches("m.");
    let path = u.path();
    match host {
        "youtube.com" | "music.youtube.com" => path.starts_with("/watch") || path.starts_with("/shorts/") || path.starts_with("/live/"),
        "youtu.be" => path.len() > 1,
        // Only individual posts – timelines/home pages are not videos.
        "x.com" | "twitter.com" | "mobile.twitter.com" => path.contains("/status/"),
        "instagram.com" => path.starts_with("/p/") || path.starts_with("/reel/") || path.starts_with("/reels/") || path.starts_with("/tv/"),
        "tiktok.com" => path.contains("/video/"),
        "facebook.com" => path.contains("/videos/") || path.starts_with("/watch") || path.starts_with("/reel/"),
        "reddit.com" | "old.reddit.com" => path.contains("/comments/"),
        "twitch.tv" => path.contains("/videos/") || path.contains("/clip/"),
        "clips.twitch.tv" | "vimeo.com" | "dailymotion.com" | "soundcloud.com" | "bilibili.com" | "streamable.com" => path.len() > 1,
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Format {
    pub url: String,
    pub headers: HeaderMap,
    pub size: Option<u64>,
    /// yt-dlp protocol: `https`, `m3u8_native`, `http_dash_segments`, ...
    pub protocol: String,
}

impl Format {
    pub fn is_http(&self) -> bool {
        matches!(self.protocol.as_str(), "http" | "https" | "")
    }
    pub fn is_hls(&self) -> bool {
        self.protocol.starts_with("m3u8")
    }
}

/// A quality the user can pick (video height, or audio only).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityOption {
    /// "1080", "720", … or "audio".
    pub id: String,
    pub height: Option<u32>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub title: String,
    pub ext: String,
    pub formats: Vec<Format>,
    pub approx_size: Option<u64>,
    pub qualities: Vec<QualityOption>,
}

impl Resolved {
    /// Some protocol we cannot fetch ourselves (DASH segments, RTMP, ...):
    /// hand the whole job to yt-dlp.
    pub fn needs_ytdlp(&self) -> bool {
        self.formats.iter().any(|f| !f.is_http() && !f.is_hls())
    }
    pub fn needs_remux(&self) -> bool {
        self.formats.len() > 1 || self.formats.iter().any(Format::is_hls)
    }
}

static CACHE: Mutex<Option<HashMap<String, (Instant, Resolved)>>> = Mutex::new(None);

/// yt-dlp format selector for a quality choice.
///
/// Prefers plain HTTPS files (segmented download = fastest) and H.264/AAC in
/// MP4 (plays everywhere), then falls back to whatever the site offers (HLS…).
/// `!=?` lets formats with unknown codecs through (X reports none for its
/// progressive MP4s).
pub(crate) fn selector(quality: Option<&str>, have_ffmpeg: bool) -> String {
    const HTTP: &str = "[protocol~='^https?$']";
    if quality == Some("audio") {
        return format!("ba[ext=m4a]{HTTP}/ba[ext=m4a]/ba/b");
    }
    let h = quality.and_then(|q| q.parse::<u32>().ok()).map(|h| format!("[height<=?{h}]")).unwrap_or_default();
    if have_ffmpeg {
        format!(
            "bv*{h}[ext=mp4]{HTTP}+ba[ext=m4a]{HTTP}/b{h}[vcodec!=?none][acodec!=?none]{HTTP}/bv*{h}[ext=mp4]+ba[ext=m4a]/bv*{h}+ba/b{h}/b"
        )
    } else {
        format!("b{h}[vcodec!=?none][acodec!=?none]{HTTP}/b{h}[vcodec!=?none][acodec!=?none]/b{h}/b")
    }
}

/// Distinct video heights (best first) plus "audio only", with size estimates.
fn qualities_of(v: &Value) -> Vec<QualityOption> {
    let Some(list) = v.get("formats").and_then(Value::as_array) else { return vec![] };
    let is_none = |f: &Value, k: &str| f.get(k).and_then(Value::as_str) == Some("none");
    let best_audio = list
        .iter()
        .filter(|f| is_none(f, "vcodec") && !is_none(f, "acodec"))
        .filter_map(size_of)
        .max();
    let mut by_height: std::collections::BTreeMap<u32, (Option<u64>, bool)> = Default::default();
    for f in list.iter().filter(|f| !is_none(f, "vcodec")) {
        let Some(h) = f.get("height").and_then(Value::as_u64).map(|h| h as u32) else { continue };
        if h < 144 {
            continue;
        }
        let has_audio = !is_none(f, "acodec") && f.get("acodec").is_some();
        let size = size_of(f).map(|s| if has_audio { s } else { s + best_audio.unwrap_or(0) });
        let e = by_height.entry(h).or_insert((None, false));
        if size > e.0 {
            *e = (size, true);
        }
    }
    let mut out: Vec<QualityOption> =
        by_height.into_iter().rev().map(|(h, (size, _))| QualityOption { id: h.to_string(), height: Some(h), size }).collect();
    if best_audio.is_some() {
        out.push(QualityOption { id: "audio".into(), height: None, size: best_audio });
    }
    out
}

fn format_of(f: &Value) -> Option<Format> {
    Some(Format {
        url: f.get("url")?.as_str()?.to_string(),
        headers: headers_of(f),
        size: size_of(f),
        protocol: f.get("protocol").and_then(Value::as_str).unwrap_or("https").to_string(),
    })
}

fn headers_of(v: &Value) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Some(obj) = v.get("http_headers").and_then(Value::as_object) {
        for (k, val) in obj {
            if let (Ok(name), Some(Ok(value))) = (HeaderName::from_bytes(k.as_bytes()), val.as_str().map(HeaderValue::from_str)) {
                // We must never ask for compressed bytes.
                if name != reqwest::header::ACCEPT_ENCODING {
                    h.insert(name, value);
                }
            }
        }
    }
    h
}

fn size_of(v: &Value) -> Option<u64> {
    v.get("filesize").and_then(Value::as_u64).or_else(|| v.get("filesize_approx").and_then(Value::as_f64).map(|f| f as u64))
}

pub(crate) async fn resolve(url: &str, quality: Option<&str>) -> Result<Resolved, String> {
    let key = format!("{url}#{}", quality.unwrap_or("best"));
    if let Some((t, r)) = CACHE.lock().get_or_insert_with(HashMap::new).get(&key) {
        if t.elapsed() < Duration::from_secs(600) {
            return Ok(r.clone());
        }
    }
    let ytdlp = tools::ytdlp().ok_or("yt-dlp is not installed")?;
    let ffmpeg = tools::ffmpeg();
    let mut cmd = tools::command(&ytdlp);
    cmd.args(["-J", "--no-playlist", "--no-warnings", "-f", &selector(quality, ffmpeg.is_some())]);
    cmd.arg(url);
    let out = tokio::time::timeout(Duration::from_secs(90), cmd.output())
        .await
        .map_err(|_| "yt-dlp timed out".to_string())?
        .map_err(|e| format!("yt-dlp: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(err.trim());
        return Err(line.trim_start_matches("ERROR: ").chars().take(300).collect());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("yt-dlp output: {e}"))?;
    let title = v.get("title").and_then(Value::as_str).unwrap_or("video").to_string();
    let formats: Vec<Format> = match v.get("requested_formats").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(format_of)
            .collect(),
        _ => vec![format_of(&v).ok_or("no downloadable format")?],
    };
    if formats.is_empty() {
        return Err("no downloadable format".into());
    }
    let exts: Vec<&str> = v
        .get("requested_formats")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|f| f.get("ext").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    let ext = if formats.len() > 1 {
        if exts.iter().all(|e| matches!(*e, "mp4" | "m4a")) {
            "mp4"
        } else {
            "mkv"
        }
    } else {
        v.get("ext").and_then(Value::as_str).unwrap_or("mp4")
    }
    .to_string();
    let approx_size = formats.iter().map(|f| f.size).sum::<Option<u64>>();
    let qualities = qualities_of(&v);
    let r = Resolved { title, ext, formats, approx_size, qualities };
    CACHE.lock().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), r.clone()));
    Ok(r)
}

pub(crate) fn forget(url: &str) {
    if let Some(m) = CACHE.lock().as_mut() {
        let prefix = format!("{url}#");
        m.retain(|k, _| !k.starts_with(&prefix));
    }
}

pub(crate) struct SiteCtx {
    pub client: reqwest::Client,
    pub live: Arc<Live>,
    pub global: Arc<crate::ratelimit::RateLimiter>,
    pub cancel: tokio_util::sync::CancellationToken,
    pub conns: u16,
}

/// Download every stream into `<base>.f<k>.velox` and return the part paths.
/// `stage` = number of streams already complete (resume).
pub(crate) async fn download(ctx: &SiteCtx, r: &Resolved, base: &std::path::Path, stage: usize, resume: &[crate::Segment]) -> Result<Vec<std::path::PathBuf>, DlError> {
    // Sizes first so the UI can show one combined progress bar.
    let mut sizes = Vec::with_capacity(r.formats.len());
    let mut ranged = Vec::with_capacity(r.formats.len());
    for f in &r.formats {
        if f.is_hls() {
            sizes.push(None);
            ranged.push(false);
            continue;
        }
        let p = http::probe(&ctx.client, &f.url, &f.headers).await?;
        sizes.push(p.size.or(f.size));
        ranged.push(p.resumable);
    }
    let grand: Option<u64> = sizes.iter().copied().sum();
    ctx.live.total.store(grand.unwrap_or(http::UNKNOWN), Ordering::Relaxed);

    let mut parts = Vec::new();
    let mut offset = 0u64;
    for (k, f) in r.formats.iter().enumerate() {
        let part = std::path::PathBuf::from(format!("{}.f{k}.velox", base.display()));
        let size = sizes[k];
        if k < stage && part.exists() {
            offset += size.unwrap_or(0);
            parts.push(part);
            continue;
        }
        // Resume this stream's segments (stored with the combined offset).
        let own: Vec<crate::Segment> = if k == stage && size.is_some() {
            resume
                .iter()
                .filter(|s| s.start >= offset && s.end <= offset + size.unwrap())
                .map(|s| crate::Segment { start: s.start - offset, end: s.end - offset, written: s.written - offset, active: false })
                .collect()
        } else {
            vec![]
        };
        let done: u64 = own.iter().map(|s| s.written - s.start).sum();
        let sub = Arc::new(Live::new(done, size, &own, 0));
        let hctx = Arc::new(http::Ctx {
            client: ctx.client.clone(),
            url: f.url.clone(),
            headers: f.headers.clone(),
            live: sub.clone(),
            global: ctx.global.clone(),
            cancel: ctx.cancel.clone(),
            max_conns: ctx.conns,
            resumable: ranged[k],
            etag: None,
            last_modified: None,
        });
        // Mirror the sub-download into the job's live state (offset map).
        let (main, mirror_sub, off) = (ctx.live.clone(), sub.clone(), offset);
        let mirror = tokio::spawn(async move {
            loop {
                mirror_main(&main, &mirror_sub, off);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let res = if f.is_hls() {
            let hctx = Arc::new(crate::hls::HlsCtx {
                client: ctx.client.clone(),
                url: f.url.clone(),
                headers: f.headers.clone(),
                live: sub.clone(),
                global: ctx.global.clone(),
                cancel: ctx.cancel.clone(),
                conns: ctx.conns.min(16),
            });
            crate::hls::run(hctx, part.clone(), 0, 0).await.map(|_| 0)
        } else {
            http::run(hctx, part.clone()).await
        };
        mirror.abort();
        mirror_main(&ctx.live, &sub, offset);
        res?;
        ctx.live.hls_done.store(k as u64 + 1, Ordering::Relaxed);
        offset += size.unwrap_or_else(|| sub.downloaded.load(Ordering::Relaxed));
        parts.push(part);
    }
    Ok(parts)
}

fn mirror_main(main: &Live, sub: &Live, off: u64) {
    main.downloaded.store(off + sub.downloaded.load(Ordering::Relaxed), Ordering::Relaxed);
    if let Some(t) = sub.total() {
        // Streams of unknown size (HLS): grow the combined estimate.
        if main.total().is_none_or(|c| c < off + t) {
            main.total.store(off + t, Ordering::Relaxed);
        }
    }
    main.active.store(sub.active.load(Ordering::Relaxed), Ordering::Relaxed);
    let mut segs: Vec<http::SegState> = main.segments.lock().iter().filter(|s| s.end <= off).copied().collect();
    if off > 0 && segs.is_empty() {
        segs.push(http::SegState { start: 0, end: off, reserved: off, written: off, active: false });
    }
    segs.extend(sub.segments.lock().iter().map(|s| http::SegState {
        start: s.start + off,
        end: s.end.saturating_add(off),
        reserved: s.reserved + off,
        written: s.written + off,
        active: s.active,
    }));
    *main.segments.lock() = segs;
}

/// Mux video + audio without re-encoding.
pub(crate) async fn merge(parts: &[std::path::PathBuf], out: &std::path::Path) -> Result<(), DlError> {
    let ff = tools::ffmpeg().ok_or_else(|| DlError::Other("ffmpeg is required to merge audio and video".into()))?;
    let mut cmd = tools::command(&ff);
    cmd.args(["-y", "-loglevel", "error"]);
    for p in parts {
        cmd.arg("-i").arg(p);
    }
    for i in 0..parts.len() {
        cmd.args(["-map", &i.to_string()]);
    }
    cmd.args(["-c", "copy"]);
    if out.extension().and_then(|e| e.to_str()) == Some("mp4") {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(out);
    let o = cmd.output().await.map_err(|e| DlError::Other(format!("ffmpeg: {e}")))?;
    if !o.status.success() {
        return Err(DlError::Other(format!("ffmpeg merge failed: {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or(""))));
    }
    Ok(())
}

/// Last resort for protocols Velox can't fetch itself: let yt-dlp download
/// (and merge) the whole thing while we relay its progress.
pub(crate) async fn ytdlp_download(ctx: &SiteCtx, page: &str, quality: Option<&str>, base: &std::path::Path) -> Result<std::path::PathBuf, DlError> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let ytdlp = tools::ytdlp().ok_or_else(|| DlError::Other("yt-dlp is not installed".into()))?;
    let mut cmd = tools::command(&ytdlp);
    let template = format!("{}.%(ext)s", base.with_extension("").display());
    cmd.args(["--no-playlist", "--no-warnings", "--newline", "--progress", "-N", "8", "--merge-output-format", "mp4"]);
    cmd.args(["-f", &selector(quality, tools::ffmpeg().is_some())]);
    if let Some(ff) = tools::ffmpeg() {
        cmd.arg("--ffmpeg-location").arg(ff);
    }
    cmd.args(["--progress-template", "download:VELOX %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"]);
    cmd.args(["--print", "after_move:VELOXFILE %(filepath)s", "-o"]).arg(&template).arg(page);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| DlError::Other(format!("yt-dlp: {e}")))?;
    let mut stderr = child.stderr.take().unwrap();
    let err_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    });
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let (mut done_prev, mut last, mut file) = (0u64, 0u64, None);
    loop {
        let line = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(DlError::Cancelled);
            }
            l = lines.next_line() => l,
        };
        let Ok(Some(line)) = line else { break };
        if let Some(p) = line.strip_prefix("VELOXFILE ") {
            file = Some(std::path::PathBuf::from(p.trim()));
        } else if let Some(rest) = line.strip_prefix("VELOX ") {
            let n: Vec<Option<u64>> = rest.split_whitespace().map(|x| x.parse::<f64>().ok().map(|f| f as u64)).collect();
            let cur = n.first().copied().flatten().unwrap_or(0);
            if cur < last {
                done_prev += last; // the next stream started
            }
            last = cur;
            ctx.live.downloaded.store(done_prev + cur, Ordering::Relaxed);
            if let Some(t) = n.get(1).copied().flatten().or(n.get(2).copied().flatten()) {
                ctx.live.total.store(done_prev + t, Ordering::Relaxed);
            }
        }
    }
    let status = child.wait().await?;
    if !status.success() {
        let err = err_task.await.unwrap_or_default();
        let msg = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("yt-dlp failed").trim_start_matches("ERROR: ");
        return Err(DlError::Other(msg.chars().take(300).collect()));
    }
    file.ok_or_else(|| DlError::Other("yt-dlp did not report the output file".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_sites() {
        assert!(is_site_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(is_site_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(is_site_url("https://www.youtube.com/shorts/abc"));
        assert!(!is_site_url("https://www.youtube.com/"));
        assert!(!is_site_url("https://example.com/video.mp4"));
        assert!(is_site_url("https://x.com/NASA/status/1234567890"));
        assert!(!is_site_url("https://x.com/home"));
    }
}
