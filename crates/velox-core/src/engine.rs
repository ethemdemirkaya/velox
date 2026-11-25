//! The engine: queue, lifecycle, speed accounting, persistence and scheduling.

use parking_lot::{Mutex, RwLock};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, COOKIE, REFERER, USER_AGENT};
use reqwest::{redirect::Policy, Client, Proxy};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::category::Category;
use crate::hls::{self, HlsCtx};
use crate::http::{self, DlError, Live};
use crate::ratelimit::RateLimiter;
use crate::settings::{AfterQueue, Settings, MAX_CONNECTIONS};
use crate::site;
use crate::store::Store;
use crate::tools::{self, ToolsStatus};
use crate::types::*;
use crate::util;

/// Honest default identity. Pretending to be Chrome without a browser's TLS
/// fingerprint gets requests flagged as bots by many CDNs; downloads captured
/// by the extension still carry the real browser UA and cookies.
const DEFAULT_UA: &str = concat!("Velox/", env!("CARGO_PKG_VERSION"), " (+https://github.com/ethemdemirkaya/velox)");

const STOP_PAUSE: u8 = 1;
const STOP_REMOVE: u8 = 2;
const STOP_REMOVE_DELETE: u8 = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EngineEvent {
    Added(DownloadInfo),
    Updated(DownloadInfo),
    Removed(DownloadId),
    Progress(Vec<Progress>),
    Settings(Settings),
    Tools(ToolsStatus),
    QueueFinished,
}

struct Job {
    record: Record,
    live: Arc<Live>,
    cancel: Option<CancellationToken>,
    speed: f64,
    last_bytes: u64,
}

impl Job {
    fn info(&self) -> DownloadInfo {
        let eta = match (self.record.total, self.speed > 1.0) {
            (Some(t), true) => Some(t.saturating_sub(self.record.downloaded) as f64 / self.speed),
            _ => None,
        };
        DownloadInfo {
            record: self.record.clone(),
            speed: self.speed,
            eta,
            active_connections: self.live.active.load(Ordering::Relaxed),
        }
    }
    fn running(&self) -> bool {
        self.cancel.is_some()
    }
}

struct Inner {
    store: Store,
    settings: RwLock<Settings>,
    jobs: Mutex<HashMap<DownloadId, Job>>,
    events: broadcast::Sender<EngineEvent>,
    global: Arc<RateLimiter>,
    client: RwLock<Client>,
    had_work: AtomicBool,
    /// Unread probe responses by URL, reused as the first connection.
    warm: Mutex<HashMap<String, (Instant, reqwest::Response)>>,
    /// Per-server-IP clients by host (multi-IP spreading).
    ip_clients: Mutex<HashMap<String, (Instant, Vec<Client>)>>,
}

/// Cheap to clone handle to the download engine.
#[derive(Clone)]
pub struct Engine(Arc<Inner>);

struct Resolved {
    url: String,
    kind: Kind,
    file_name: String,
    total: Option<u64>,
    resumable: bool,
    content_type: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    final_url: String,
    qualities: Vec<site::QualityOption>,
}

fn build_client(s: &Settings) -> Client {
    build_client_pinned(s, None)
}

/// Like [`build_client`], optionally pinning `host` to one server address.
fn build_client_pinned(s: &Settings, pin: Option<(&str, std::net::SocketAddr)>) -> Client {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, HeaderValue::from_static("*/*"));
    // Never let a server compress: ranges must map 1:1 to file bytes.
    h.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    let ua = if s.user_agent.trim().is_empty() { DEFAULT_UA } else { s.user_agent.trim() };
    let mut b = Client::builder()
        // Separate TCP connections are the whole point of segmentation;
        // HTTP/2 would multiplex them onto one socket.
        .http1_only()
        // OS TLS stack (Schannel on Windows): same fingerprint as browsers &
        // curl, honours system / corporate root certificates.
        .use_native_tls()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(MAX_CONNECTIONS as usize * 2)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(40))
        .redirect(Policy::limited(15))
        .user_agent(ua)
        .default_headers(h);
    if !s.proxy.trim().is_empty() {
        match Proxy::all(s.proxy.trim()) {
            Ok(p) => b = b.proxy(p),
            Err(e) => tracing::warn!("invalid proxy {}: {e}", s.proxy),
        }
    }
    if let Some((host, addr)) = pin {
        b = b.resolve(host, addr);
    }
    b.build().expect("http client")
}

fn header_map(referrer: &Option<String>, cookies: &Option<String>, ua: &Option<String>, extra: &[(String, String)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    let mut put = |k: HeaderName, v: &str| {
        if let Ok(v) = HeaderValue::from_str(v) {
            h.insert(k, v);
        }
    };
    if let Some(r) = referrer.as_deref().filter(|s| !s.is_empty()) {
        put(REFERER, r);
    }
    if let Some(c) = cookies.as_deref().filter(|s| !s.is_empty()) {
        put(COOKIE, c);
    }
    if let Some(u) = ua.as_deref().filter(|s| !s.is_empty()) {
        put(USER_AGENT, u);
    }
    for (k, v) in extra {
        if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
            put(name, v);
        }
    }
    h
}

/// Pick the number of connections from the file size. Users never need to
/// tune this; servers that dislike many connections are handled adaptively.
fn auto_connections(total: Option<u64>) -> u16 {
    const MB: u64 = 1024 * 1024;
    match total {
        None => 8,
        Some(t) if t < 4 * MB => 4,
        Some(t) if t < 64 * MB => 8,
        Some(t) if t < 1024 * MB => 16,
        Some(_) => MAX_CONNECTIONS,
    }
}

fn stream_name(url: &str, given: Option<&str>) -> String {
    let base = given
        .map(str::to_string)
        .or_else(|| util::filename_from_url(url))
        .unwrap_or_else(|| "video".into());
    let (stem, ext) = util::split_ext(&base);
    let stem = if matches!(ext, ".m3u8" | ".mpd" | ".ts" | ".mp4") || ext.is_empty() { stem } else { &base };
    let stem = if matches!(stem, "index" | "master" | "playlist" | "manifest" | "chunklist") {
        format!("video-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))
    } else {
        stem.to_string()
    };
    util::sanitize(&format!("{stem}.mp4"))
}

impl Engine {
    /// Open (or create) the engine database in `data_dir`. Must be called
    /// from within a Tokio runtime.
    pub fn open(data_dir: &Path) -> Result<Engine, String> {
        tools::set_dir(data_dir.join("tools"));
        let store = Store::open(&data_dir.join("velox.db")).map_err(|e| e.to_string())?;
        Ok(Self::with_store(store))
    }

    pub fn in_memory() -> Engine {
        tools::set_dir(std::env::temp_dir().join("velox-tools"));
        Self::with_store(Store::memory())
    }

    fn with_store(store: Store) -> Engine {
        let settings = store.settings();
        let mut jobs = HashMap::new();
        for mut r in store.load_all() {
            if r.status.is_active() {
                // We crashed or were closed mid-download.
                r.status = Status::Paused;
            }
            let live = Arc::new(Live::new(r.downloaded, r.total, &r.segments, r.speed_limit));
            jobs.insert(r.id.clone(), Job { record: r, live, cancel: None, speed: 0.0, last_bytes: 0 });
        }
        let (events, _) = broadcast::channel(512);
        let inner = Arc::new(Inner {
            client: RwLock::new(build_client(&settings)),
            global: Arc::new(RateLimiter::new(settings.speed_limit)),
            settings: RwLock::new(settings),
            store,
            jobs: Mutex::new(jobs),
            events,
            had_work: AtomicBool::new(false),
            warm: Mutex::new(HashMap::new()),
            ip_clients: Mutex::new(HashMap::new()),
        });
        let engine = Engine(inner);
        tokio::spawn(engine.clone().ticker());
        engine.pump();
        engine
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.0.events.subscribe()
    }

    fn emit(&self, e: EngineEvent) {
        let _ = self.0.events.send(e);
    }

    pub fn settings(&self) -> Settings {
        self.0.settings.read().clone()
    }

    pub fn set_settings(&self, s: Settings) -> Settings {
        let s = s.sanitized();
        let old = std::mem::replace(&mut *self.0.settings.write(), s.clone());
        if old.proxy != s.proxy || old.user_agent != s.user_agent {
            *self.0.client.write() = build_client(&s);
            self.0.ip_clients.lock().clear();
        }
        self.0.global.set_rate(s.speed_limit);
        self.0.store.save_settings(&s);
        self.emit(EngineEvent::Settings(s.clone()));
        self.pump();
        s
    }

    pub fn list(&self) -> Vec<DownloadInfo> {
        let jobs = self.0.jobs.lock();
        let mut v: Vec<_> = jobs.values().map(Job::info).collect();
        v.sort_by_key(|d| std::cmp::Reverse(d.record.created_at));
        v
    }

    pub fn get(&self, id: &str) -> Option<DownloadInfo> {
        self.0.jobs.lock().get(id).map(Job::info)
    }

    fn default_dir(&self, name: &str) -> PathBuf {
        let s = self.0.settings.read();
        let base = PathBuf::from(&s.download_dir);
        if s.use_category_folders {
            base.join(Category::from_file_name(name).folder())
        } else {
            base
        }
    }

    async fn resolve(&self, req: &AddRequest) -> Result<Resolved, String> {
        let url = req.url.trim().to_string();
        let parsed = url::Url::parse(&url).map_err(|_| "invalid URL".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("unsupported scheme: {}", parsed.scheme()));
        }
        let kind = req.kind.unwrap_or_else(|| Kind::detect(&url, None));
        if kind == Kind::Site {
            // First video-site download: fetch the helpers automatically so
            // users get HD (video + audio) without touching any setting.
            if tools::ytdlp().is_none() || (tools::ffmpeg().is_none() && cfg!(windows)) {
                Box::pin(self.install_tools()).await.map_err(|e| format!("could not install video tools: {e}"))?;
            }
            let r = site::resolve(&url, req.quality.as_deref()).await?;
            let stem: String = util::sanitize(&r.title).chars().take(150).collect();
            return Ok(Resolved {
                file_name: format!("{stem}.{}", r.ext),
                final_url: url.clone(),
                url,
                kind,
                total: r.approx_size,
                resumable: true,
                content_type: None,
                etag: None,
                last_modified: None,
                qualities: r.qualities,
            });
        }
        if kind != Kind::Http {
            return Ok(Resolved {
                file_name: stream_name(&url, req.file_name.as_deref()),
                final_url: url.clone(),
                url,
                kind,
                total: None,
                resumable: kind == Kind::Hls,
                content_type: None,
                etag: None,
                last_modified: None,
                qualities: vec![],
            });
        }
        let client = self.0.client.read().clone();
        let headers = header_map(&req.referrer, &req.cookies, &req.user_agent, &req.headers);
        let mut p = http::probe(&client, &url, &headers).await.map_err(|e| e.to_string())?;
        if let Some(resp) = p.response.take() {
            let mut warm = self.0.warm.lock();
            warm.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(15));
            warm.insert(url.clone(), (Instant::now(), resp));
        }
        if p.kind != Kind::Http {
            return Ok(Resolved {
                file_name: stream_name(&p.final_url, req.file_name.as_deref()),
                url,
                kind: p.kind,
                total: None,
                resumable: p.kind == Kind::Hls,
                content_type: p.content_type,
                etag: None,
                last_modified: None,
                final_url: p.final_url,
                qualities: vec![],
            });
        }
        let file_name = req.file_name.as_deref().map(util::sanitize).filter(|s| !s.is_empty()).unwrap_or(p.file_name);
        Ok(Resolved {
            url,
            kind: Kind::Http,
            file_name,
            total: p.size,
            resumable: p.resumable,
            content_type: p.content_type,
            etag: p.etag,
            last_modified: p.last_modified,
            final_url: p.final_url,
            qualities: vec![],
        })
    }

    /// Resolve a video page in the background so the download dialog is
    /// instant when the user clicks (called when a video page is opened).
    pub fn prefetch(&self, url: &str) {
        site::prefetch(url);
    }

    /// Inspect a URL without adding it (used by the confirm dialog).
    pub async fn probe(&self, req: AddRequest) -> Result<ProbeInfo, String> {
        let r = self.resolve(&req).await?;
        let dir = req.dir.clone().map(PathBuf::from).unwrap_or_else(|| self.default_dir(&r.file_name));
        Ok(ProbeInfo {
            url: r.url,
            final_url: r.final_url,
            category: Category::from_file_name(&r.file_name),
            file_name: r.file_name,
            size: r.total,
            resumable: r.resumable,
            content_type: r.content_type,
            kind: r.kind,
            dir: dir.to_string_lossy().into_owned(),
            qualities: r.qualities,
        })
    }

    pub async fn add(&self, req: AddRequest) -> Result<DownloadInfo, String> {
        let r = self.resolve(&req).await?;
        let dir = req.dir.clone().filter(|d| !d.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| self.default_dir(&r.file_name));
        let path = util::unique_path(&dir, &r.file_name);
        let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(r.file_name);
        let record = Record {
            id: uuid::Uuid::new_v4().to_string(),
            url: r.url,
            kind: r.kind,
            category: Category::from_file_name(&file_name),
            file_name,
            dir: dir.to_string_lossy().into_owned(),
            total: r.total,
            downloaded: 0,
            status: if req.start_paused { Status::Paused } else { Status::Queued },
            resumable: r.resumable,
            segments: vec![],
            hls_done: 0,
            hls_count: 0,
            hls_bytes: 0,
            etag: r.etag,
            last_modified: r.last_modified,
            referrer: req.referrer,
            cookies: req.cookies,
            user_agent: req.user_agent,
            headers: req.headers,
            connections: req.connections.unwrap_or_else(|| auto_connections(r.total)).clamp(1, MAX_CONNECTIONS),
            speed_limit: req.speed_limit.unwrap_or(0),
            quality: req.quality.clone(),
            error: None,
            created_at: util::now_ms(),
            finished_at: None,
        };
        self.0.store.upsert(&record);
        let live = Arc::new(Live::new(0, record.total, &[], record.speed_limit));
        let job = Job { record, live, cancel: None, speed: 0.0, last_bytes: 0 };
        let info = job.info();
        self.0.jobs.lock().insert(info.record.id.clone(), job);
        self.emit(EngineEvent::Added(info.clone()));
        self.pump();
        Ok(self.get(&info.record.id).unwrap_or(info))
    }

    /// Start queued downloads while below the concurrency limit.
    fn pump(&self) {
        let max = self.0.settings.read().max_concurrent as usize;
        let next: Vec<DownloadId> = {
            let jobs = self.0.jobs.lock();
            let running = jobs.values().filter(|j| j.running()).count();
            let mut queued: Vec<&Job> = jobs.values().filter(|j| j.record.status == Status::Queued && !j.running()).collect();
            queued.sort_by_key(|j| j.record.created_at);
            queued.into_iter().take(max.saturating_sub(running)).map(|j| j.record.id.clone()).collect()
        };
        for id in next {
            self.start(&id);
        }
    }

    fn start(&self, id: &str) {
        let (rec, live, cancel, info) = {
            let mut jobs = self.0.jobs.lock();
            let Some(job) = jobs.get_mut(id) else { return };
            if job.running() || job.record.status == Status::Completed {
                return;
            }
            let r = &job.record;
            let live = Arc::new(Live::new(r.downloaded, r.total, &r.segments, r.speed_limit));
            live.hls_done.store(r.hls_done, Ordering::Relaxed);
            live.hls_bytes.store(r.hls_bytes, Ordering::Relaxed);
            let cancel = CancellationToken::new();
            job.live = live.clone();
            job.cancel = Some(cancel.clone());
            job.last_bytes = r.downloaded;
            job.speed = 0.0;
            job.record.status = Status::Connecting;
            job.record.error = None;
            (job.record.clone(), live, cancel, job.info())
        };
        self.0.had_work.store(true, Ordering::Relaxed);
        self.0.store.upsert(&info.record);
        self.emit(EngineEvent::Updated(info));
        let engine = self.clone();
        tokio::spawn(async move {
            let id = rec.id.clone();
            let res = engine.run_job(rec, live.clone(), cancel).await;
            engine.finish(&id, live, res).await;
        });
    }

    async fn run_job(&self, rec: Record, live: Arc<Live>, cancel: CancellationToken) -> Result<Option<String>, DlError> {
        let client = self.0.client.read().clone();
        let headers = header_map(&rec.referrer, &rec.cookies, &rec.user_agent, &rec.headers);
        let path = rec.path();
        let part = util::part_path(&path);
        match rec.kind {
            Kind::Http => {
                let first = match self.0.warm.lock().remove(&rec.url) {
                    Some((t, r)) if t.elapsed() < Duration::from_secs(15) && rec.downloaded == 0 => Some(r),
                    _ => None,
                };
                let clients = self.clients_for(&rec.url, client).await;
                let ctx = Arc::new(http::Ctx {
                    clients,
                    first: Mutex::new(first),
                    url: rec.url.clone(),
                    headers,
                    live,
                    global: self.0.global.clone(),
                    cancel,
                    max_conns: rec.connections,
                    resumable: rec.resumable,
                    etag: rec.etag.clone(),
                    last_modified: rec.last_modified.clone(),
                });
                http::run(ctx, part.clone()).await?;
                finalize(&part, &path).map(Some)
            }
            Kind::Hls => {
                let ctx = Arc::new(HlsCtx {
                    client,
                    url: rec.url.clone(),
                    headers,
                    live,
                    global: self.0.global.clone(),
                    cancel,
                    conns: rec.connections.min(16),
                });
                let done = hls::run(ctx, part.clone(), rec.hls_done, rec.hls_bytes).await?;
                if done.ext == "ts" {
                    self.set_status(&rec.id, Status::Merging);
                    let tmp = path.with_extension("remux.mp4");
                    if hls::remux(&part, &tmp).await {
                        let _ = std::fs::remove_file(&part);
                        return finalize(&tmp, &path).map(Some);
                    }
                    let _ = std::fs::remove_file(&tmp);
                    return finalize(&part, &path.with_extension("ts")).map(Some);
                }
                finalize(&part, &path).map(Some)
            }
            Kind::Site => {
                let res = self.run_site(&rec, client, live, cancel, &path).await;
                if res.is_err() {
                    // Media URLs expire; resolve again on the next attempt.
                    site::forget(&rec.url);
                }
                res
            }
            Kind::Dash => {
                let ctx = Arc::new(HlsCtx {
                    client,
                    url: rec.url.clone(),
                    headers,
                    live,
                    global: self.0.global.clone(),
                    cancel,
                    conns: 1,
                });
                hls::run_dash(ctx, part.clone()).await?;
                finalize(&part, &path).map(Some)
            }
        }
    }

    async fn run_site(
        &self,
        rec: &Record,
        client: Client,
        live: Arc<Live>,
        cancel: CancellationToken,
        path: &Path,
    ) -> Result<Option<String>, DlError> {
        let r = site::resolve(&rec.url, rec.quality.as_deref()).await.map_err(DlError::Other)?;
        let path = path.with_extension(&r.ext);
        let ctx = site::SiteCtx { client, live, global: self.0.global.clone(), cancel, conns: rec.connections };
        if r.needs_ytdlp() {
            let out = site::ytdlp_download(&ctx, &rec.url, rec.quality.as_deref(), &path).await?;
            return Ok(out.file_name().map(|s| s.to_string_lossy().into_owned()));
        }
        let parts = site::download(&ctx, &r, &path, rec.hls_done as usize, &rec.segments).await?;
        if !r.needs_remux() {
            return finalize(&parts[0], &path).map(Some);
        }
        self.set_status(&rec.id, Status::Merging);
        let tmp = path.with_extension(format!("merging.{}", r.ext));
        site::merge(&parts, &tmp).await?;
        for p in &parts {
            let _ = std::fs::remove_file(p);
        }
        finalize(&tmp, &path).map(Some)
    }

    /// Install yt-dlp and (on Windows) ffmpeg into the private tools folder.
    /// ffmpeg is fetched through the engine itself, so it shows up – and is
    /// accelerated – like any other download.
    pub async fn install_tools(&self) -> Result<ToolsStatus, String> {
        let client = self.0.client.read().clone();
        if tools::ytdlp().is_none() {
            tools::install_ytdlp(&client).await?;
            self.emit(EngineEvent::Tools(tools::status()));
        }
        if tools::ffmpeg().is_none() && cfg!(windows) {
            let dir = tools::dir().ok_or("no tools dir")?.to_string_lossy().into_owned();
            let d = self
                .add(AddRequest { url: tools::FFMPEG_ZIP.into(), dir: Some(dir), file_name: Some("ffmpeg-tools.zip".into()), ..Default::default() })
                .await?;
            let id = d.record.id.clone();
            if self.get(&id).map(|d| d.record.status) == Some(Status::Queued) {
                self.resume(&id);
            }
            let done = loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                match self.get(&id) {
                    Some(d) if d.record.status == Status::Completed => break d,
                    Some(d) if d.record.status == Status::Failed => return Err(d.record.error.unwrap_or_default()),
                    Some(d) if d.record.status == Status::Paused => return Err("cancelled".into()),
                    None => return Err("cancelled".into()),
                    _ => {}
                }
            };
            let zip = done.record.path();
            let res = tokio::task::spawn_blocking(move || tools::extract_ffmpeg(&zip)).await.map_err(|e| e.to_string())?;
            self.remove(&id, true);
            res?;
        }
        let st = tools::status();
        self.emit(EngineEvent::Tools(st.clone()));
        Ok(st)
    }

    /// CDNs answer with several server IPs. Spreading connections across them
    /// lifts per-server throughput caps. Falls back to the shared client.
    async fn clients_for(&self, url: &str, base: Client) -> Vec<Client> {
        let s = self.settings();
        if !s.proxy.trim().is_empty() {
            return vec![base];
        }
        let Ok(u) = url::Url::parse(url) else { return vec![base] };
        let Some(host) = u.host_str().map(str::to_string) else { return vec![base] };
        if host.parse::<std::net::IpAddr>().is_ok() || host == "localhost" {
            return vec![base];
        }
        if let Some((t, c)) = self.0.ip_clients.lock().get(&host) {
            if t.elapsed() < Duration::from_secs(300) {
                return c.clone();
            }
        }
        let port = u.port_or_known_default().unwrap_or(443);
        let addrs: Vec<std::net::SocketAddr> =
            match tokio::time::timeout(Duration::from_secs(2), tokio::net::lookup_host((host.as_str(), port))).await {
                Ok(Ok(a)) => a.collect(),
                _ => return vec![base],
            };
        // Prefer IPv4 when available (IPv6 may be advertised but unroutable).
        let v4: Vec<_> = addrs.iter().copied().filter(|a| a.is_ipv4()).collect();
        let mut ips = if v4.is_empty() { addrs } else { v4 };
        ips.dedup();
        ips.truncate(4);
        if ips.len() < 2 {
            return vec![base];
        }
        let clients: Vec<Client> = ips.iter().map(|a| build_client_pinned(&s, Some((&host, *a)))).collect();
        tracing::debug!("{host}: spreading over {} server IPs", clients.len());
        self.0.ip_clients.lock().insert(host, (Instant::now(), clients.clone()));
        clients
    }

    fn set_status(&self, id: &str, status: Status) {
        let info = {
            let mut jobs = self.0.jobs.lock();
            let Some(job) = jobs.get_mut(id) else { return };
            job.record.status = status;
            job.info()
        };
        self.emit(EngineEvent::Updated(info));
    }

    async fn finish(&self, id: &str, live: Arc<Live>, res: Result<Option<String>, DlError>) {
        let reason = live.stop_reason.load(Ordering::Relaxed);
        if reason == STOP_REMOVE || reason == STOP_REMOVE_DELETE {
            // Job was already removed from the map.
            return;
        }
        let mut restart = false;
        let info = {
            let mut jobs = self.0.jobs.lock();
            let Some(job) = jobs.get_mut(id) else { return };
            job.cancel = None;
            job.speed = 0.0;
            let r = &mut job.record;
            r.segments = live.snapshot();
            r.downloaded = live.downloaded.load(Ordering::Relaxed);
            r.total = live.total().or(r.total);
            r.hls_done = live.hls_done.load(Ordering::Relaxed);
            r.hls_count = live.hls_count.load(Ordering::Relaxed);
            r.hls_bytes = live.hls_bytes.load(Ordering::Relaxed);
            match res {
                Ok(name) => {
                    if let Some(p) = name {
                        r.file_name = p;
                    }
                    r.status = Status::Completed;
                    r.finished_at = Some(util::now_ms());
                    r.total = Some(r.downloaded.max(r.total.unwrap_or(0)));
                    r.segments.clear();
                    r.error = None;
                }
                Err(DlError::Cancelled) => r.status = Status::Paused,
                Err(DlError::RangeIgnored) if r.resumable => {
                    tracing::info!("{}: server ignored ranges, restarting single-stream", r.file_name);
                    r.resumable = false;
                    r.segments.clear();
                    r.downloaded = 0;
                    r.status = Status::Queued;
                    restart = true;
                }
                Err(e) => {
                    r.status = Status::Failed;
                    r.error = Some(e.to_string());
                }
            }
            job.info()
        };
        self.0.store.upsert(&info.record);
        self.emit(EngineEvent::Updated(info));
        if restart {
            self.start(id);
        }
        self.pump();
        self.check_queue_finished();
    }

    fn check_queue_finished(&self) {
        let busy = self.0.jobs.lock().values().any(|j| j.running() || j.record.status == Status::Queued);
        if !busy && self.0.had_work.swap(false, Ordering::Relaxed) {
            self.emit(EngineEvent::QueueFinished);
            let action = self.0.settings.read().after_queue;
            if action != AfterQueue::Nothing {
                run_power_action(action);
            }
        }
    }

    pub fn pause(&self, id: &str) {
        let info = {
            let mut jobs = self.0.jobs.lock();
            let Some(job) = jobs.get_mut(id) else { return };
            if let Some(c) = &job.cancel {
                job.live.stop_reason.store(STOP_PAUSE, Ordering::Relaxed);
                c.cancel();
                return;
            }
            if job.record.status != Status::Queued {
                return;
            }
            job.record.status = Status::Paused;
            job.info()
        };
        self.0.store.upsert(&info.record);
        self.emit(EngineEvent::Updated(info));
    }

    /// Start (or restart) a download immediately, ignoring the queue limit.
    pub fn resume(&self, id: &str) {
        self.start(id);
    }

    pub fn pause_all(&self) {
        let ids: Vec<_> = self.0.jobs.lock().keys().cloned().collect();
        for id in ids {
            self.pause(&id);
        }
    }

    /// Put every unfinished download back into the queue.
    pub fn resume_all(&self) {
        let changed: Vec<DownloadInfo> = {
            let mut jobs = self.0.jobs.lock();
            jobs.values_mut()
                .filter(|j| !j.running() && matches!(j.record.status, Status::Paused | Status::Failed))
                .map(|j| {
                    j.record.status = Status::Queued;
                    j.info()
                })
                .collect()
        };
        for i in changed {
            self.0.store.upsert(&i.record);
            self.emit(EngineEvent::Updated(i));
        }
        self.pump();
    }

    pub fn remove(&self, id: &str, delete_file: bool) {
        let Some(job) = self.0.jobs.lock().remove(id) else { return };
        self.0.store.delete(id);
        let path = job.record.path();
        let part = util::part_path(&path);
        let completed = job.record.status == Status::Completed;
        if let Some(c) = job.cancel {
            job.live.stop_reason.store(if delete_file { STOP_REMOVE_DELETE } else { STOP_REMOVE }, Ordering::Relaxed);
            c.cancel();
            // Give the task a moment to close the file handle before deleting.
            tokio::spawn(async move {
                for _ in 0..50 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if job.live.active.load(Ordering::Relaxed) == 0 && (!delete_file || std::fs::remove_file(&part).is_ok() || !part.exists()) {
                        break;
                    }
                }
            });
        } else if delete_file {
            let _ = std::fs::remove_file(&part);
            if completed {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.emit(EngineEvent::Removed(id.to_string()));
        self.pump();
    }

    /// Remove every completed download from the list (files are kept).
    pub fn clear_completed(&self) {
        let ids: Vec<_> = self
            .0
            .jobs
            .lock()
            .values()
            .filter(|j| j.record.status == Status::Completed)
            .map(|j| j.record.id.clone())
            .collect();
        for id in ids {
            self.remove(&id, false);
        }
    }

    pub fn set_speed_limit(&self, id: &str, limit: u64) {
        let info = {
            let mut jobs = self.0.jobs.lock();
            let Some(job) = jobs.get_mut(id) else { return };
            job.record.speed_limit = limit;
            job.live.limiter.set_rate(limit);
            job.info()
        };
        self.0.store.upsert(&info.record);
        self.emit(EngineEvent::Updated(info));
    }

    /// Total current download speed in bytes/s.
    pub fn total_speed(&self) -> f64 {
        self.0.jobs.lock().values().filter(|j| j.running()).map(|j| j.speed).sum()
    }

    async fn ticker(self) {
        let mut iv = tokio::time::interval(Duration::from_millis(250));
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last = Instant::now();
        let mut tick: u64 = 0;
        let mut last_minute = String::new();
        loop {
            iv.tick().await;
            // Stop when the engine has been dropped everywhere else.
            if Arc::strong_count(&self.0) <= 1 {
                return;
            }
            tick += 1;
            let dt = last.elapsed().as_secs_f64().max(0.001);
            last = Instant::now();
            let mut progress = Vec::new();
            let mut persist = Vec::new();
            let mut updated = Vec::new();
            {
                let mut jobs = self.0.jobs.lock();
                for job in jobs.values_mut().filter(|j| j.running()) {
                    let d = job.live.downloaded.load(Ordering::Relaxed);
                    let inst = d.saturating_sub(job.last_bytes) as f64 / dt;
                    job.last_bytes = d;
                    job.speed = if job.speed == 0.0 { inst } else { job.speed * 0.75 + inst * 0.25 };
                    if job.speed < 1.0 {
                        job.speed = 0.0;
                    }
                    job.record.downloaded = d;
                    job.record.total = job.live.total().or(job.record.total);
                    job.record.hls_done = job.live.hls_done.load(Ordering::Relaxed);
                    job.record.hls_count = job.live.hls_count.load(Ordering::Relaxed);
                    job.record.hls_bytes = job.live.hls_bytes.load(Ordering::Relaxed);
                    if job.record.status == Status::Connecting && d > 0 {
                        job.record.status = Status::Downloading;
                        updated.push(job.info());
                    }
                    let segments = job.live.snapshot();
                    if tick.is_multiple_of(8) {
                        job.record.segments = segments.clone();
                        persist.push(job.record.clone());
                    }
                    let info = job.info();
                    progress.push(Progress {
                        id: info.record.id,
                        downloaded: d,
                        total: info.record.total,
                        speed: info.speed,
                        eta: info.eta,
                        active_connections: info.active_connections,
                        segments,
                        status: info.record.status,
                    });
                }
            }
            for u in updated {
                self.emit(EngineEvent::Updated(u));
            }
            if !progress.is_empty() {
                self.emit(EngineEvent::Progress(progress));
            }
            if !persist.is_empty() {
                self.0.store.upsert_many(&persist);
            }
            if tick.is_multiple_of(4) {
                self.check_schedule(&mut last_minute);
            }
        }
    }

    fn check_schedule(&self, last_minute: &mut String) {
        let sched = self.0.settings.read().schedule.clone();
        if !sched.enabled {
            return;
        }
        let now = chrono::Local::now().format("%H:%M").to_string();
        if *last_minute == now {
            return;
        }
        *last_minute = now.clone();
        if now == sched.start {
            tracing::info!("scheduler: starting queue");
            self.resume_all();
        } else if !sched.stop.is_empty() && now == sched.stop {
            tracing::info!("scheduler: pausing all");
            self.pause_all();
        }
    }
}

/// Move the finished part file into place. Returns the final file name.
fn finalize(part: &Path, path: &Path) -> Result<String, DlError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let target = if path.exists() { util::unique_path(dir, &name) } else { path.to_path_buf() };
    std::fs::rename(part, &target)?;
    Ok(target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name))
}

fn run_power_action(action: AfterQueue) {
    let (prog, args): (&str, &[&str]) = match action {
        AfterQueue::Nothing => return,
        #[cfg(windows)]
        AfterQueue::Shutdown => ("shutdown", &["/s", "/t", "60", "/c", "Velox: downloads finished"]),
        #[cfg(windows)]
        AfterQueue::Sleep => ("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        #[cfg(target_os = "macos")]
        AfterQueue::Shutdown => ("osascript", &["-e", "tell app \"System Events\" to shut down"]),
        #[cfg(target_os = "macos")]
        AfterQueue::Sleep => ("pmset", &["sleepnow"]),
        #[cfg(all(unix, not(target_os = "macos")))]
        AfterQueue::Shutdown => ("systemctl", &["poweroff"]),
        #[cfg(all(unix, not(target_os = "macos")))]
        AfterQueue::Sleep => ("systemctl", &["suspend"]),
    };
    let _ = std::process::Command::new(prog).args(args).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_streams() {
        assert_eq!(stream_name("https://a.com/x/movie.m3u8", None), "movie.mp4");
        assert!(stream_name("https://a.com/x/index.m3u8", None).starts_with("video-"));
    }
}
