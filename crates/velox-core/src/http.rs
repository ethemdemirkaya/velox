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
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::ratelimit::RateLimiter;
use crate::types::{Kind, Segment};
use crate::util;

/// Below this many remaining bytes a segment is not split any further.
const MIN_SPLIT: u64 = 512 * 1024;
/// Initial segments are never smaller than this.
const MIN_SEGMENT: u64 = 1024 * 1024;
/// Bytes buffered per connection before a positional write (fewer syscalls
/// on gigabit lines).
const WRITE_BUF: usize = 2 * 1024 * 1024;
const MAX_RETRIES: u32 = 12;
/// Connection ramp: start wide, grow fast. Servers that dislike it make the
/// extra workers yield (adaptive limit below).
const RAMP_START: u16 = 8;
const RAMP_STEP: u16 = 4;
const RAMP_EVERY: Duration = Duration::from_millis(100);
/// A connection slower than median/4 for this many 1-second ticks gets
/// replaced by a fresh one (often landing on a faster CDN node).
const SLOW_TICKS: u8 = 3;
/// Only replace slow connections that still have this much left to fetch.
const SLOW_MIN_REMAINING: u64 = 128 * 1024;
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

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SegState {
    pub start: u64,
    pub end: u64,
    pub reserved: u64,
    pub written: u64,
    pub active: bool,
    /// Set by the manager: drop this connection and reconnect.
    pub restart: bool,
    /// `reserved` at the previous speed tick.
    pub last: u64,
    /// Consecutive ticks this connection was a straggler.
    pub slow: u8,
    /// Speed ticks since this connection started (young ones are still in
    /// TCP slow start and must not be judged or used as reference).
    pub age: u8,
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
    /// Typical bytes/second of one healthy connection (median while several
    /// run) – lets a lone straggler at the end still be detected.
    pub typical: AtomicU64,
}

impl Live {
    pub fn new(downloaded: u64, total: Option<u64>, segs: &[Segment], limit: u64) -> Self {
        Self {
            downloaded: AtomicU64::new(downloaded),
            total: AtomicU64::new(total.unwrap_or(UNKNOWN)),
            active: AtomicU16::new(0),
            segments: Mutex::new(
                segs.iter()
                    .map(|s| SegState { start: s.start, end: s.end, reserved: s.written, written: s.written, ..Default::default() })
                    .collect(),
            ),
            limiter: RateLimiter::new(limit),
            stop_reason: AtomicU8::new(0),
            hls_done: AtomicU64::new(0),
            hls_count: AtomicU64::new(0),
            hls_bytes: AtomicU64::new(0),
            typical: AtomicU64::new(0),
        }
    }

    pub fn total(&self) -> Option<u64> {
        match self.total.load(Ordering::Relaxed) {
            UNKNOWN => None,
            t => Some(t),
        }
    }

    pub fn snapshot(&self) -> Vec<Segment> {
        self.segments.lock().iter().map(|s| Segment { start: s.start, end: s.end, written: s.written, active: s.active }).collect()
    }
}

pub(crate) struct Ctx {
    /// One client per server IP (CDNs return several); connections are
    /// spread across them so one node's per-host cap doesn't bound us.
    pub clients: Vec<Client>,
    /// The probe's still-open response (`Range: bytes=0-`), reused as the
    /// first connection – saves a full TCP+TLS round trip.
    pub first: Mutex<Option<reqwest::Response>>,
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
    /// The unread probe response, kept so the download can continue on it.
    pub response: Option<reqwest::Response>,
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
    Ok(Probed {
        response: Some(resp),
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
            Segment { start, end, written: start, active: false }
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
                    .map(|s| SegState { start: s.start, end: s.end, reserved: s.start, written: s.start, ..Default::default() })
                    .collect(),
                Some(t) => vec![SegState { start: 0, end: t, ..Default::default() }],
                None => vec![SegState { start: 0, end: UNKNOWN, ..Default::default() }],
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
        {
            let mut segs = ctx.live.segments.lock();
            let s = &mut segs[idx];
            s.active = true;
            s.last = s.reserved;
            s.age = 0;
            s.slow = 0;
        }
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
            let mut segs = ctx.live.segments.lock();
            let free = (limit as usize).saturating_sub(ctx.live.active.load(Ordering::Relaxed) as usize).max(1);
            claim_idle(&mut segs, free)
        };
        match idle.or_else(|| if segmented { steal(&ctx.live) } else { None }) {
            Some(i) => spawn(set, i),
            None => break,
        }
    };

    // Connection count follows AIMD (like TCP congestion control): when the
    // server refuses connections the limit drops to what is actually working
    // (at most once per second, never below 2); after 3 quiet seconds it grows
    // by one again. The download
    // settles at whatever the server tolerates instead of collapsing to 1.
    let max = ctx.max_conns.max(1);
    let mut limit = max;
    let mut last_cut = Instant::now() - Duration::from_secs(10);
    let mut last_yield = Instant::now() - Duration::from_secs(10);
    let mut ramp = RAMP_START.min(limit);
    refill(&mut set, ramp);
    let mut tick = tokio::time::interval(RAMP_EVERY);
    tick.tick().await;
    let mut speed_tick = tokio::time::interval(Duration::from_secs(1));
    speed_tick.tick().await;

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
                        last_yield = Instant::now();
                        if last_cut.elapsed() >= Duration::from_secs(1) {
                            last_cut = Instant::now();
                            // Converge in one step to what is actually working
                            // (+1 to keep probing), never below 2.
                            let working = ctx.live.active.load(Ordering::Relaxed);
                            limit = (limit - limit / 4).min((working + 1).max(2)).max(2).min(max);
                            ramp = ramp.min(limit);
                            tracing::debug!("server refused a connection, limit now {limit}");
                        }
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
            _ = speed_tick.tick(), if segmented && first_err.is_none() => {
                flag_stragglers(&ctx.live);
                // Additive increase once the server has been quiet for a while.
                if limit < max && last_yield.elapsed() >= Duration::from_secs(2) && last_cut.elapsed() >= Duration::from_secs(2) {
                    limit += 1;
                    ramp = limit;
                    last_cut = Instant::now();
                    if !child.is_cancelled() {
                        refill(&mut set, ramp);
                    }
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

/// Pick the next idle segment for a new connection. When there are more idle
/// segments than free connection slots, the untouched neighbours are merged
/// into it so one request covers them: servers that throttle every new
/// request (common on CDN mirrors) then see a few long transfers instead of
/// dozens of short ones. Work stealing splits them again if slots open up.
fn claim_idle(segs: &mut [SegState], free: usize) -> Option<usize> {
    let i = segs.iter().position(|s| !s.active && s.written < s.end)?;
    let idle = segs.iter().filter(|s| !s.active && s.written < s.end).count();
    let mut take = idle.div_ceil(free).saturating_sub(1);
    let mut end = segs[i].end;
    while take > 0 {
        let Some(j) = segs.iter().position(|s| s.start == end && !s.active && s.written == s.start && s.end > s.start) else {
            break;
        };
        end = segs[j].end;
        segs[j].end = segs[j].start; // absorbed: zero-length, counts as done
        take -= 1;
    }
    segs[i].end = end;
    Some(i)
}

/// Mark connections that lag far behind the median for replacement.
fn flag_stragglers(live: &Live) {
    const MATURE: u8 = 2;
    let mut segs = live.segments.lock();
    let mut deltas: Vec<u64> =
        segs.iter().filter(|s| s.active && s.age >= MATURE).map(|s| s.reserved.saturating_sub(s.last)).collect();
    // Reference speed of a healthy connection: median of the mature ones,
    // remembered (decaying slowly) so a lone straggler at the end is caught.
    let prev = live.typical.load(Ordering::Relaxed);
    // No mature connections yet (fast servers finish segments quickly):
    // fall back to the average speed of everything currently running.
    let active: Vec<u64> = segs.iter().filter(|s| s.active).map(|s| s.reserved.saturating_sub(s.last)).collect();
    let candidate = if deltas.len() >= 3 {
        deltas.sort_unstable();
        Some(deltas[deltas.len() / 2])
    } else if active.len() >= 3 {
        Some(active.iter().sum::<u64>() / active.len() as u64)
    } else {
        None
    };
    let reference = match candidate {
        Some(c) => {
            let r = c.max(prev - prev / 10);
            live.typical.store(r, Ordering::Relaxed);
            r
        }
        None => prev,
    };
    for s in segs.iter_mut() {
        let delta = s.reserved.saturating_sub(s.last);
        s.last = s.reserved;
        if !s.active {
            continue;
        }
        s.age = s.age.saturating_add(1);
        if s.age <= MATURE || s.end == UNKNOWN || s.end.saturating_sub(s.reserved) < SLOW_MIN_REMAINING || reference < 256 * 1024 {
            s.slow = 0;
            continue;
        }
        if delta * 4 < reference {
            s.slow += 1;
            if s.slow >= SLOW_TICKS {
                s.slow = 0;
                s.restart = true;
                s.age = 0;
            }
        } else {
            s.slow = 0;
        }
    }
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
    segs.push(SegState { start: mid, end: s.end, reserved: mid, written: mid, last: mid, ..Default::default() });
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
    let mut rotation = 0usize;
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
        let client = &ctx.clients[(idx + rotation) % ctx.clients.len()];
        let mut req = client.get(&ctx.url).headers(ctx.headers.clone());
        let ranged = ctx.resumable;
        // The probe connection is already streaming byte 0 onwards.
        let reuse = if from == 0 { ctx.first.lock().take() } else { None };
        if ranged {
            let range = if end == UNKNOWN { format!("bytes={from}-") } else { format!("bytes={from}-{}", end - 1) };
            req = req.header(RANGE, range);
            if let Some(v) = ctx.etag.as_ref().or(ctx.last_modified.as_ref()) {
                if let Ok(hv) = HeaderValue::from_str(v) {
                    req = req.header(IF_RANGE, hv);
                }
            }
        }
        let resp = match reuse {
            Some(r) => Ok(r),
            None => tokio::select! {
                _ = cancel.cancelled() => return Err(DlError::Cancelled),
                r = req.send() => r,
            },
        };
        let mut reconnect = false;
        let mut retry_after: Option<u64> = None;
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
                if matches!(st.as_u16(), 429 | 503) {
                    retry_after = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.trim().parse::<u64>().ok());
                }
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
                    // Wake up now and then so a stalled socket can be replaced.
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if std::mem::take(&mut ctx.live.segments.lock()[idx].restart) {
                            flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                            reconnect = true;
                            return Ok(false);
                        }
                        continue;
                    }
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
                let (allowed, reached_end, restart) = {
                    let mut segs = ctx.live.segments.lock();
                    let s = &mut segs[idx];
                    let allowed = (chunk.len() as u64).min(s.end.saturating_sub(s.reserved)) as usize;
                    s.reserved += allowed as u64;
                    (allowed, s.reserved >= s.end, if s.reserved >= s.end { false } else { std::mem::take(&mut s.restart) })
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
                if restart {
                    flush_commit(ctx, idx, file, &mut buf, buf_off).await?;
                    reconnect = true;
                    return Ok(false);
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
        if reconnect {
            // Straggler: reconnect right away, on the next server IP.
            rotation += 1;
            tracing::debug!("segment {idx}: replacing slow connection");
            continue;
        }
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
        rotation += 1;
        let backoff = match retry_after {
            Some(secs) => Duration::from_secs(secs.min(30)),
            None => Duration::from_millis((400u64 << attempt.min(5)).min(10_000)),
        };
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
        let live = Live::new(0, Some(64 << 20), &[Segment { start: 0, end: 64 << 20, written: 0, active: false }], 0);
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
