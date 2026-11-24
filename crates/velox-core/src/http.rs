//! Segmented HTTP downloader with dynamic (work-stealing) segmentation.
//!
//! Every connection owns one [`SegState`]. A worker *reserves* bytes before
//! buffering them and *commits* them after they hit the disk. When a worker
//! finishes, the manager halves the segment with the most bytes left and
//! starts a new connection on the upper half – the old worker simply stops
//! once it reaches the new (lower) end. Reservation under a mutex guarantees
//! no byte is ever downloaded twice.

use bytes::Bytes;
use futures_util::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::{Client, StatusCode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::ratelimit::RateLimiter;
use crate::types::{Kind, Segment};
use crate::util;

/// Below this many remaining bytes a segment is not split any further.
const MIN_SPLIT: u64 = 1024 * 1024;
/// Initial segments are never smaller than this.
const MIN_SEGMENT: u64 = 1024 * 1024;
/// Bytes buffered per connection before a positional write.
const WRITE_BUF: usize = 1024 * 1024;
const MAX_RETRIES: u32 = 12;
const RAMP_START: u16 = 2;
const RAMP_STEP: u16 = 2;
const RAMP_EVERY: Duration = Duration::from_millis(150);
pub const UNKNOWN: u64 = u64::MAX;

#[derive(Debug, thiserror::Error, Clone)]
pub enum DlError {
    #[error("cancelled")]
    Cancelled,
    #[error("HTTP {0}")]
    Http(u16),
    #[error("network: {0}")]
    Network(String),
    #[error("disk: {0}")]
    Io(String),
    /// Server ignored our Range header (or the file changed) – restart from 0.
    #[error("server does not honour ranges")]
    RangeIgnored,
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for DlError {
    fn from(e: std::io::Error) -> Self {
        DlError::Io(e.to_string())
    }
}

impl From<reqwest::Error> for DlError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(s) = e.status() {
            return DlError::Http(s.as_u16());
        }
        let mut msg = e.to_string();
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            msg = format!("{msg}: {s}");
            src = s.source();
        }
        DlError::Network(msg)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SegState {
    pub start: u64,
    pub end: u64,
    pub reserved: u64,
    pub written: u64,
    pub active: bool,
}

/// Live state shared between the engine (UI ticker) and the download task.
pub(crate) struct Live {
    pub downloaded: AtomicU64,
    pub total: AtomicU64,
    pub active: AtomicU16,
    pub segments: Mutex<Vec<SegState>>,
    pub limiter: RateLimiter,
    /// 0 = none, 1 = pause, 2 = remove
    pub stop_reason: AtomicU8,
    pub hls_done: AtomicU64,
    pub hls_count: AtomicU64,
    pub hls_bytes: AtomicU64,
}

impl Live {
    pub fn new(downloaded: u64, total: Option<u64>, segs: &[Segment], limit: u64) -> Self {
        Self {
            downloaded: AtomicU64::new(downloaded),
            total: AtomicU64::new(total.unwrap_or(UNKNOWN)),
            active: AtomicU16::new(0),
            segments: Mutex::new(
                segs.iter()
                    .map(|s| SegState { start: s.start, end: s.end, reserved: s.written, written: s.written, active: false })
                    .collect(),
            ),
            limiter: RateLimiter::new(limit),
            stop_reason: AtomicU8::new(0),
            hls_done: AtomicU64::new(0),
            hls_count: AtomicU64::new(0),
            hls_bytes: AtomicU64::new(0),
        }
    }

    pub fn total(&self) -> Option<u64> {
        match self.total.load(Ordering::Relaxed) {
            UNKNOWN => None,
            t => Some(t),
        }
    }

    pub fn snapshot(&self) -> Vec<Segment> {
        self.segments.lock().iter().map(|s| Segment { start: s.start, end: s.end, written: s.written }).collect()
    }
}

pub(crate) struct Ctx {
    pub client: Client,
    pub url: String,
    pub headers: HeaderMap,
    pub live: Arc<Live>,
    pub global: Arc<RateLimiter>,
    pub cancel: CancellationToken,
    pub max_conns: u16,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub(crate) struct Probed {
    pub final_url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub kind: Kind,
}

fn header_str(h: &HeaderMap, k: impl reqwest::header::AsHeaderName) -> Option<String> {
    h.get(k).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

/// `bytes 0-99/1234` -> 1234
fn total_from_content_range(v: &str) -> Option<u64> {
    v.rsplit('/').next()?.trim().parse().ok()
}

pub(crate) async fn probe(client: &Client, url: &str, headers: &HeaderMap) -> Result<Probed, DlError> {
    let resp = client
        .get(url)
        .headers(headers.clone())
        .header(RANGE, "bytes=0-")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(DlError::Http(status.as_u16()));
    }
    let h = resp.headers();
    let final_url = resp.url().to_string();
    let content_type = header_str(h, CONTENT_TYPE);
    let (size, resumable) = if status == StatusCode::PARTIAL_CONTENT {
        (header_str(h, CONTENT_RANGE).as_deref().and_then(total_from_content_range), true)
    } else {
        (header_str(h, CONTENT_LENGTH).and_then(|v| v.parse().ok()), false)
    };
    let kind = Kind::detect(&final_url, content_type.as_deref());
    let mut name = header_str(h, CONTENT_DISPOSITION)
        .as_deref()
        .and_then(util::filename_from_disposition)
        .or_else(|| util::filename_from_url(&final_url))
        .or_else(|| util::filename_from_url(url))
        .unwrap_or_else(|| "download".into());
    if !name.contains('.') {
        if let Some(ext) = content_type.as_deref().and_then(util::ext_for_mime) {
            name = format!("{name}.{ext}");
        }
    }
    let etag = header_str(h, ETAG).filter(|e| !e.starts_with("W/"));
    let last_modified = header_str(h, LAST_MODIFIED);
    drop(resp); // closes the connection, we only needed the headers
    Ok(Probed {
        final_url,
        file_name: util::sanitize(&name),
        size,
        resumable: resumable && size.is_some(),
        content_type,
        etag,
        last_modified,
        kind,
    })
}

/// Split `total` bytes into `n` equal segments.
pub(crate) fn plan(total: u64, conns: u16) -> Vec<Segment> {
    let n = (total / MIN_SEGMENT).clamp(1, conns.max(1) as u64);
    let size = total / n;
    (0..n)
        .map(|i| {
            let start = i * size;
            let end = if i == n - 1 { total } else { start + size };
            Segment { start, end, written: start }
        })
        .collect()
}

#[cfg(windows)]
fn make_sparse(f: &std::fs::File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let mut ret = 0u32;
    // SAFETY: valid handle, no in/out buffers.
    unsafe {
        DeviceIoControl(
            f.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut ret,
            std::ptr::null_mut(),
        );
    }
}
#[cfg(not(windows))]
fn make_sparse(_f: &std::fs::File) {}

fn open_part(path: &Path, total: Option<u64>, fresh: bool) -> Result<std::fs::File, DlError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(fresh).open(path)?;
    if let Some(t) = total {
        if fresh || f.metadata()?.len() != t {
            // Sparse on NTFS: avoids the synchronous zero-fill that would stall
            // the first write of a far-away segment on big files.
            make_sparse(&f);
            f.set_len(t)?;
        }
    }
    Ok(f)
}

/// Download into `part`. Returns the final size.
pub(crate) async fn run(ctx: Arc<Ctx>, part: PathBuf) -> Result<u64, DlError> {
    let total = ctx.live.total();
    let segmented = ctx.resumable && total.is_some();
    let fresh = {
        let mut segs = ctx.live.segments.lock();
        let fresh = segs.is_empty() || !segmented;
        if segs.is_empty() || !segmented {
            *segs = match total {
                Some(t) if segmented => plan(t, ctx.max_conns)
                    .into_iter()
                    .map(|s| SegState { start: s.start, end: s.end, reserved: s.start, written: s.start, active: false })
                    .collect(),
                Some(t) => vec![SegState { start: 0, end: t, reserved: 0, written: 0, active: false }],
                None => vec![SegState { start: 0, end: UNKNOWN, reserved: 0, written: 0, active: false }],
            };
            ctx.live.downloaded.store(0, Ordering::Relaxed);
        }
        fresh
    };
    let fresh = fresh || !part.exists();
    if fresh {
        // If the part file vanished, start over.
        let mut segs = ctx.live.segments.lock();
        for s in segs.iter_mut() {
            s.written = s.start;
            s.reserved = s.start;
        }
        ctx.live.downloaded.store(0, Ordering::Relaxed);
    }
    let file = Arc::new(open_part(&part, total, fresh)?);

    let child = ctx.cancel.child_token();
    let mut set: JoinSet<Result<WorkerEnd, DlError>> = JoinSet::new();
    let spawn = |set: &mut JoinSet<Result<WorkerEnd, DlError>>, idx: usize| {
        let (ctx, file, tok) = (ctx.clone(), file.clone(), child.clone());
        ctx.live.segments.lock()[idx].active = true;
        ctx.live.active.fetch_add(1, Ordering::Relaxed);
        set.spawn(async move {
            let r = worker(&ctx, idx, &file, &tok).await;
            ctx.live.segments.lock()[idx].active = false;
            ctx.live.active.fetch_sub(1, Ordering::Relaxed);
            r
        });
    };
    // Keep `limit` connections busy: unfinished idle segments first, then
    // steal half of the biggest remaining range.
    let refill = |set: &mut JoinSet<Result<WorkerEnd, DlError>>, limit: u16| loop {
        if ctx.live.active.load(Ordering::Relaxed) >= limit {
            break;
        }
        let idle = {
            let segs = ctx.live.segments.lock();
            segs.iter().position(|s| !s.active && s.written < s.end)
        };
        match idle.or_else(|| if segmented { steal(&ctx.live) } else { None }) {
            Some(i) => spawn(set, i),
            None => break,
        }
    };

    // Adaptive connection limit: servers that cap connections per IP make
    // extra workers yield; we settle at what the server tolerates.
    let mut limit = ctx.max_conns.max(1);
    // Ramp up gently (2 connections, +2 every 150 ms). Opening 16+ sockets in
    // the same instant trips connection-rate limits on many servers.
    let mut ramp = RAMP_START.min(limit);
    refill(&mut set, ramp);
    let mut tick = tokio::time::interval(RAMP_EVERY);
    tick.tick().await;

    let mut first_err: Option<DlError> = None;
    loop {
        tokio::select! {
            joined = set.join_next() => {
                let Some(joined) = joined else { break };
                let res = joined.unwrap_or_else(|e| Err(DlError::Other(e.to_string())));
                match res {
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                            child.cancel();
                        }
                        continue;
                    }
                    Ok(WorkerEnd::Yield) => {
                        let active = ctx.live.active.load(Ordering::Relaxed).max(1);
                        limit = active.min(limit);
                        ramp = ramp.min(limit);
                        tracing::debug!("connection yielded, limit now {limit}");
                    }
                    Ok(WorkerEnd::Done) => {}
                }
                if first_err.is_none() && !child.is_cancelled() {
                    refill(&mut set, ramp);
                }
            }
            _ = tick.tick(), if ramp < limit && first_err.is_none() => {
                ramp = (ramp + RAMP_STEP).min(limit);
                if !child.is_cancelled() {
                    refill(&mut set, ramp);
                }
            }
        }
    }

    // Wait for any in-flight positional writes then decide.
    if let Some(e) = first_err {
        return Err(e);
    }
    if ctx.cancel.is_cancelled() {
        return Err(DlError::Cancelled);
    }
    let segs = ctx.live.segments.lock().clone();
    let size = match total {
        Some(t) => {
            if segs.iter().any(|s| s.written < s.end) {
                return Err(DlError::Network("connection closed before all data arrived".into()));
            }
            t
        }
        None => {
            let t = segs.iter().map(|s| s.written).max().unwrap_or(0);
            ctx.live.total.store(t, Ordering::Relaxed);
            t
        }
    };
    let f = file.clone();
    tokio::task::spawn_blocking(move || f.sync_data()).await.ok();
    Ok(size)
}

/// Halve the segment with most remaining bytes. Returns the new segment index.
fn steal(live: &Live) -> Option<usize> {
    let mut segs = live.segments.lock();
    let (idx, rem) = segs
        .iter()
        .enumerate()
        .filter(|(_, s)| s.active && s.end != UNKNOWN)
        .map(|(i, s)| (i, s.end.saturating_sub(s.reserved)))
        .max_by_key(|(_, r)| *r)?;
    if rem < MIN_SPLIT * 2 {
        return None;
    }
    let s = segs[idx];
    // Align to 64 KiB for nicer disk writes.
    let mid = ((s.reserved + rem / 2) / 65_536 * 65_536).max(s.reserved + 1);
    if mid >= s.end {
        return None;
    }
    segs[idx].end = mid;
    segs.push(SegState { start: mid, end: s.end, reserved: mid, written: mid, active: false });
    Some(segs.len() - 1)
}

async fn flush(file: &Arc<std::fs::File>, buf: &mut Vec<u8>, off: u64) -> Result<(), DlError> {
    if buf.is_empty() {
        return Ok(());
    }
    let data = std::mem::take(buf);
    let f = file.clone();
    let back = tokio::task::spawn_blocking(move || util::write_at(&f, &data, off).map(|_| data))
        .await
        .map_err(|e| DlError::Io(e.to_string()))??;
    *buf = back;
    buf.clear();
    Ok(())
}

pub(crate) enum WorkerEnd {
    /// The segment is complete.
    Done,
    /// The server refused this extra connection; others will take over.
    Yield,
}

async fn worker(ctx: &Ctx, idx: usize, file: &Arc<std::fs::File>, cancel: &CancellationToken) -> Result<WorkerEnd, DlError> {
    let mut attempt = 0u32;
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUF + 64 * 1024);
    loop {
        // (Re)start from the committed position.
        let (from, end) = {
            let mut segs = ctx.live.segments.lock();
            let s = &mut segs[idx];
            s.reserved = s.written;
            (s.written, s.end)
        };
        if from >= end {
            return Ok(WorkerEnd::Done);
        }
        let mut req = ctx.client.get(&ctx.url).headers(ctx.headers.clone());
        let ranged = ctx.resumable;
        if ranged {
            let range = if end == UNKNOWN { format!("bytes={from}-") } else { format!("bytes={from}-{}", end - 1) };
            req = req.header(RANGE, range);
            if let Some(v) = ctx.etag.as_ref().or(ctx.last_modified.as_ref()) {
                if let Ok(hv) = HeaderValue::from_str(v) {
                    req = req.header(IF_RANGE, hv);
                }
            }
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(DlError::Cancelled),
            r = req.send() => r,
        };
        let outcome: Result<bool, DlError> = async {
            let resp = resp?;
            let st = resp.status();
            if ranged && st == StatusCode::OK {
                return Err(DlError::RangeIgnored);
            }
            if st == StatusCode::RANGE_NOT_SATISFIABLE {
                return Err(DlError::RangeIgnored);
            }
            if !st.is_success() {
                return Err(DlError::Http(st.as_u16()));
            }
            let mut stream = resp.bytes_stream();
            let mut buf_off = from;
            loop {
                let next: Option<Result<Bytes, reqwest::Error>> = tokio::select! {
                    _ = cancel.cancelled() => {
                        flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                        return Err(DlError::Cancelled);
                    }
                    n = stream.next() => n,
                };
                let chunk = match next {
                    Some(Ok(c)) => c,
                    Some(Err(e)) => {
                        flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                        return Err(e.into());
                    }
                    None => {
                        flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                        let s = ctx.live.segments.lock()[idx];
                        if s.end == UNKNOWN {
                            ctx.live.segments.lock()[idx].end = s.written;
                            return Ok(true);
                        }
                        return Ok(s.written >= s.end);
                    }
                };
                ctx.global.acquire(chunk.len()).await;
                ctx.live.limiter.acquire(chunk.len()).await;
                let (allowed, reached_end) = {
                    let mut segs = ctx.live.segments.lock();
                    let s = &mut segs[idx];
                    let allowed = (chunk.len() as u64).min(s.end.saturating_sub(s.reserved)) as usize;
                    s.reserved += allowed as u64;
                    (allowed, s.reserved >= s.end)
                };
                buf.extend_from_slice(&chunk[..allowed]);
                ctx.live.downloaded.fetch_add(allowed as u64, Ordering::Relaxed);
                if buf.len() >= WRITE_BUF || reached_end {
                    let n = buf.len() as u64;
                    flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                    buf_off += n;
                    attempt = 0;
                }
                if reached_end {
                    return Ok(true);
                }
            }
        }
        .await;

        // Un-count bytes that were reserved but never committed.
        {
            let mut segs = ctx.live.segments.lock();
            let s = &mut segs[idx];
            let lost = s.reserved - s.written;
            s.reserved = s.written;
            ctx.live.downloaded.fetch_sub(lost, Ordering::Relaxed);
        }
        // Other connections are alive, so this failure most likely means the
        // server caps connections per client: step aside instead of failing.
        let others = ctx.live.active.load(Ordering::Relaxed) > 1;
        match outcome {
            Ok(true) => return Ok(WorkerEnd::Done),
            Ok(false) => {} // premature EOF -> retry
            Err(DlError::Cancelled) => return Err(DlError::Cancelled),
            Err(e @ (DlError::RangeIgnored | DlError::Io(_))) => return Err(e),
            Err(DlError::Http(code)) if others && matches!(code, 403 | 429 | 503) => return Ok(WorkerEnd::Yield),
            Err(DlError::Http(code)) if (400..500).contains(&code) && code != 408 && code != 429 => {
                return Err(DlError::Http(code))
            }
            Err(e) => {
                tracing::debug!("segment {idx} attempt {attempt}: {e}");
                if others && attempt >= 1 {
                    return Ok(WorkerEnd::Yield);
                }
                if attempt >= MAX_RETRIES {
                    return Err(e);
                }
            }
        }
        attempt += 1;
        let backoff = Duration::from_millis((400u64 << attempt.min(5)).min(10_000));
        tokio::select! {
            _ = cancel.cancelled() => return Err(DlError::Cancelled),
            _ = tokio::time::sleep(backoff) => {}
        }
    }
}

/// Write the buffer at its offset and advance the committed frontier.
async fn flush_commit(ctx: &Ctx, idx: usize, file: &Arc<std::fs::File>, buf: &mut Vec<u8>, off: u64) -> Result<(), DlError> {
    let n = buf.len() as u64;
    if n == 0 {
        return Ok(());
    }
    // `off` is where this buffer begins; it always equals the committed frontier.
    let committed = ctx.live.segments.lock()[idx].written;
    let off = off.max(committed);
    flush(file, buf, off).await?;
    ctx.live.segments.lock()[idx].written = off + n;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_segments() {
        let p = plan(100 * 1024 * 1024, 16);
        assert_eq!(p.len(), 16);
        assert_eq!(p.last().unwrap().end, 100 * 1024 * 1024);
        assert!(p.windows(2).all(|w| w[0].end == w[1].start));
        assert_eq!(plan(500_000, 16).len(), 1);
    }

    #[test]
    fn steals_half() {
        let live = Live::new(0, Some(64 << 20), &[Segment { start: 0, end: 64 << 20, written: 0 }], 0);
        live.segments.lock()[0].active = true;
        let i = steal(&live).unwrap();
        let s = live.segments.lock();
        assert_eq!(s[0].end, s[i].start);
        assert_eq!(s[i].end, 64 << 20);
    }

    #[test]
    fn content_range() {
        assert_eq!(total_from_content_range("bytes 0-0/12345"), Some(12345));
        assert_eq!(total_from_content_range("bytes 0-0/*"), None);
    }
}
