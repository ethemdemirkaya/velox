//! HLS (m3u8) downloader with parallel segment fetching and AES-128 support,
//! plus a DASH fallback that delegates to ffmpeg when it is installed.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use futures_util::{stream, StreamExt, TryStreamExt};
use m3u8_rs::{KeyMethod, Playlist};
use reqwest::header::{HeaderMap, RANGE};
use reqwest::Client;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::http::{DlError, Live};
use crate::ratelimit::RateLimiter;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

pub(crate) struct HlsCtx {
    pub client: Client,
    pub url: String,
    pub headers: HeaderMap,
    pub live: Arc<Live>,
    pub global: Arc<RateLimiter>,
    pub cancel: CancellationToken,
    pub conns: u16,
}

#[derive(Clone)]
struct Job {
    uri: String,
    range: Option<(u64, u64)>,
    key: Option<(String, Option<[u8; 16]>)>,
    seq: u64,
}

pub(crate) struct HlsDone {
    /// The output container produced (`ts` or `mp4`).
    pub ext: &'static str,
}

async fn get_bytes(ctx: &HlsCtx, url: &str, range: Option<(u64, u64)>) -> Result<Vec<u8>, DlError> {
    let mut attempt = 0u32;
    loop {
        let mut req = ctx.client.get(url).headers(ctx.headers.clone());
        if let Some((off, len)) = range {
            req = req.header(RANGE, format!("bytes={off}-{}", off + len - 1));
        }
        let res: Result<Vec<u8>, DlError> = async {
            let resp = req.send().await?;
            let st = resp.status();
            if !st.is_success() {
                return Err(DlError::Http(st.as_u16()));
            }
            let mut out = Vec::with_capacity(resp.content_length().unwrap_or(0) as usize);
            let mut s = resp.bytes_stream();
            while let Some(chunk) = tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(DlError::Cancelled),
                c = s.next() => c,
            } {
                let chunk = chunk?;
                ctx.global.acquire(chunk.len()).await;
                ctx.live.limiter.acquire(chunk.len()).await;
                ctx.live.downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                out.extend_from_slice(&chunk);
            }
            Ok(out)
        }
        .await;
        match res {
            Ok(v) => return Ok(v),
            Err(DlError::Cancelled) => return Err(DlError::Cancelled),
            Err(DlError::Http(c)) if (400..500).contains(&c) && c != 408 && c != 429 => return Err(DlError::Http(c)),
            Err(e) if attempt >= 8 => return Err(e),
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(300 << attempt.min(5))).await;
            }
        }
    }
}

fn resolve(base: &str, uri: &str) -> String {
    url::Url::parse(base).and_then(|b| b.join(uri)).map(|u| u.to_string()).unwrap_or_else(|_| uri.to_string())
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let hex = s.trim_start_matches("0x").trim_start_matches("0X");
    if hex.len() != 32 {
        return None;
    }
    let mut iv = [0u8; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(iv)
}

/// Resolve master -> best variant -> media playlist and flatten it into jobs.
async fn load(ctx: &HlsCtx) -> Result<(Vec<Job>, Option<Job>), DlError> {
    let mut url = ctx.url.clone();
    for _ in 0..3 {
        let body = get_bytes(ctx, &url, None).await?;
        ctx.live.downloaded.fetch_sub(body.len() as u64, Ordering::Relaxed);
        let pl = m3u8_rs::parse_playlist_res(&body).map_err(|_| DlError::Other("invalid m3u8 playlist".into()))?;
        match pl {
            Playlist::MasterPlaylist(m) => {
                let best = m
                    .variants
                    .iter()
                    .filter(|v| !v.is_i_frame)
                    .max_by_key(|v| v.bandwidth)
                    .ok_or_else(|| DlError::Other("playlist has no variants".into()))?;
                url = resolve(&url, &best.uri);
            }
            Playlist::MediaPlaylist(m) => {
                let mut jobs = Vec::with_capacity(m.segments.len());
                let mut key: Option<(String, Option<[u8; 16]>)> = None;
                let mut init = None;
                let mut next_off = 0u64;
                for (i, s) in m.segments.iter().enumerate() {
                    if let Some(k) = &s.key {
                        key = match &k.method {
                            KeyMethod::AES128 => {
                                let uri = k.uri.as_ref().ok_or_else(|| DlError::Other("key without URI".into()))?;
                                Some((resolve(&url, uri), k.iv.as_deref().and_then(parse_iv)))
                            }
                            KeyMethod::None => None,
                            _ => return Err(DlError::Other("unsupported HLS encryption (SAMPLE-AES / DRM)".into())),
                        };
                    }
                    if init.is_none() {
                        if let Some(map) = &s.map {
                            init = Some(Job {
                                uri: resolve(&url, &map.uri),
                                range: map.byte_range.as_ref().map(|b| (b.offset.unwrap_or(0), b.length)),
                                key: None,
                                seq: 0,
                            });
                        }
                    }
                    let range = s.byte_range.as_ref().map(|b| {
                        let off = b.offset.unwrap_or(next_off);
                        next_off = off + b.length;
                        (off, b.length)
                    });
                    jobs.push(Job { uri: resolve(&url, &s.uri), range, key: key.clone(), seq: m.media_sequence + i as u64 });
                }
                return Ok((jobs, init));
            }
        }
    }
    Err(DlError::Other("playlist nesting too deep".into()))
}

pub(crate) async fn run(ctx: Arc<HlsCtx>, part: PathBuf, resume_from: u64, resume_bytes: u64) -> Result<HlsDone, DlError> {
    let (jobs, init) = load(&ctx).await?;
    if jobs.is_empty() {
        return Err(DlError::Other("empty playlist".into()));
    }
    ctx.live.hls_count.store(jobs.len() as u64, Ordering::Relaxed);
    let fmp4 = init.is_some();

    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let start = if resume_from > 0 && part.exists() { resume_from.min(jobs.len() as u64) } else { 0 };
    let file = std::fs::OpenOptions::new().create(true).write(true).truncate(start == 0).open(&part)?;
    if start > 0 {
        file.set_len(resume_bytes)?;
    }
    let mut file = std::io::BufWriter::with_capacity(4 << 20, file);
    use std::io::Seek;
    file.seek(std::io::SeekFrom::End(0))?;
    ctx.live.downloaded.store(if start > 0 { resume_bytes } else { 0 }, Ordering::Relaxed);
    ctx.live.hls_done.store(start, Ordering::Relaxed);
    ctx.live.hls_bytes.store(if start > 0 { resume_bytes } else { 0 }, Ordering::Relaxed);

    if start == 0 {
        if let Some(init) = &init {
            let data = get_bytes(&ctx, &init.uri, init.range).await?;
            file.write_all(&data)?;
        }
    }

    let keys: Arc<tokio::sync::Mutex<HashMap<String, [u8; 16]>>> = Default::default();
    let total = jobs.len() as u64;
    if start == 0 {
        file.flush()?;
        ctx.live.hls_bytes.store(file.get_ref().metadata()?.len(), Ordering::Relaxed);
    }
    let mut results = stream::iter(jobs.into_iter().skip(start as usize))
        .map(|job| {
            let ctx = ctx.clone();
            let keys = keys.clone();
            async move {
                let mut data = get_bytes(&ctx, &job.uri, job.range).await?;
                if let Some((key_url, iv)) = &job.key {
                    let key = {
                        let mut k = keys.lock().await;
                        match k.get(key_url) {
                            Some(v) => *v,
                            None => {
                                let raw = get_bytes(&ctx, key_url, None).await?;
                                let arr: [u8; 16] =
                                    raw.get(..16).and_then(|s| s.try_into().ok()).ok_or_else(|| DlError::Other("bad AES key".into()))?;
                                k.insert(key_url.clone(), arr);
                                arr
                            }
                        }
                    };
                    let iv = iv.unwrap_or_else(|| {
                        let mut b = [0u8; 16];
                        b[8..].copy_from_slice(&job.seq.to_be_bytes());
                        b
                    });
                    let n = Aes128CbcDec::new(&key.into(), &iv.into())
                        .decrypt_padded_mut::<Pkcs7>(&mut data)
                        .map_err(|_| DlError::Other("AES decrypt failed".into()))?
                        .len();
                    data.truncate(n);
                }
                Ok::<_, DlError>(data)
            }
        })
        .buffered(ctx.conns.max(1) as usize);

    let mut done = start;
    let mut bytes_written = ctx.live.hls_bytes.load(Ordering::Relaxed);
    while let Some(seg) = results.try_next().await? {
        file.write_all(&seg)?;
        bytes_written += seg.len() as u64;
        done += 1;
        ctx.live.hls_done.store(done, Ordering::Relaxed);
        // Estimated size so the UI can show a meaningful progress bar.
        let est = bytes_written / done.max(1) * total;
        ctx.live.total.store(est.max(bytes_written), Ordering::Relaxed);
        // Flush every segment so the persisted resume point (`hls_done`,
        // `hls_bytes`) always matches what is on disk.
        file.flush()?;
        ctx.live.hls_bytes.store(bytes_written, Ordering::Relaxed);
    }
    file.flush()?;
    drop(file);
    ctx.live.total.store(bytes_written, Ordering::Relaxed);
    ctx.live.downloaded.store(bytes_written, Ordering::Relaxed);
    Ok(HlsDone { ext: if fmp4 { "mp4" } else { "ts" } })
}

pub fn ffmpeg() -> Option<PathBuf> {
    which::which("ffmpeg").ok()
}

/// Remux a finished `.ts` into `.mp4` without re-encoding.
pub(crate) async fn remux(input: &Path, output: &Path) -> bool {
    let Some(ff) = ffmpeg() else { return false };
    let mut cmd = tokio::process::Command::new(ff);
    cmd.args(["-y", "-loglevel", "error", "-i"]).arg(input).args(["-c", "copy", "-bsf:a", "aac_adtstoasc"]).arg(output);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    matches!(cmd.status().await, Ok(s) if s.success())
}

/// DASH: let ffmpeg fetch and mux the best audio+video representation.
pub(crate) async fn run_dash(ctx: Arc<HlsCtx>, output: PathBuf) -> Result<(), DlError> {
    let ff = ffmpeg().ok_or_else(|| DlError::Other("DASH needs ffmpeg in PATH (https://ffmpeg.org)".into()))?;
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let headers: String = ctx
        .headers
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| format!("{}: {}\r\n", k.as_str(), v)))
        .collect();
    let mut cmd = tokio::process::Command::new(ff);
    cmd.args(["-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"]);
    if !headers.is_empty() {
        cmd.arg("-headers").arg(headers);
    }
    cmd.arg("-i").arg(&ctx.url).args(["-c", "copy", "-f", "mp4"]).arg(&output);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| DlError::Other(format!("ffmpeg: {e}")))?;
    let stdout = child.stdout.take().unwrap();
    let live = ctx.live.clone();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if let Some(v) = l.strip_prefix("total_size=") {
                if let Ok(n) = v.trim().parse::<u64>() {
                    live.downloaded.store(n, Ordering::Relaxed);
                }
            }
        }
    });
    let status = tokio::select! {
        _ = ctx.cancel.cancelled() => {
            let _ = child.kill().await;
            reader.abort();
            let _ = std::fs::remove_file(&output);
            return Err(DlError::Cancelled);
        }
        s = child.wait() => s?,
    };
    let _ = reader.await;
    if !status.success() {
        return Err(DlError::Other("ffmpeg failed to download the DASH stream".into()));
    }
    let len = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    ctx.live.total.store(len, Ordering::Relaxed);
    ctx.live.downloaded.store(len, Ordering::Relaxed);
    Ok(())
}
