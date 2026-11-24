//! End-to-end tests against a tiny in-process HTTP/1.1 server.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use velox_core::{AddRequest, Engine, EngineEvent, Settings, Status};

struct Server {
    url: String,
    data: Arc<Vec<u8>>,
    conns: Arc<AtomicUsize>,
}

/// Serve `data` at `/file.bin`. `ranges` toggles Range support; `throttle`
/// limits bytes/s *per connection* so multi-connection speedup is observable.
async fn serve(size: usize, ranges: bool, throttle: Option<usize>) -> Server {
    let data: Arc<Vec<u8>> = Arc::new((0..size).map(|i| (i * 31 % 251) as u8).collect());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let conns = Arc::new(AtomicUsize::new(0));
    let (d, c) = (data.clone(), conns.clone());
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let (d, c) = (d.clone(), c.clone());
            tokio::spawn(async move {
                c.fetch_add(1, Ordering::SeqCst);
                loop {
                    let mut buf = vec![0u8; 8192];
                    let mut req = Vec::new();
                    loop {
                        let n = match sock.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        req.extend_from_slice(&buf[..n]);
                        if req.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&req).to_lowercase();
                    let range = text.lines().find_map(|l| l.strip_prefix("range: bytes=")).map(|r| r.trim().to_string());
                    let total = d.len();
                    let (status, start, end) = match (ranges, range) {
                        (true, Some(r)) => {
                            let (a, b) = r.split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { total - 1 } else { b.parse().unwrap() };
                            ("206 Partial Content", a, b.min(total - 1) + 1)
                        }
                        _ => ("200 OK", 0, total),
                    };
                    let mut head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"file.bin\"\r\n",
                        end - start
                    );
                    if status.starts_with("206") {
                        head += &format!("Content-Range: bytes {start}-{}/{total}\r\nAccept-Ranges: bytes\r\n", end - 1);
                    }
                    head += "\r\n";
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let body = &d[start..end];
                    let chunk = 16 * 1024;
                    for part in body.chunks(chunk) {
                        if sock.write_all(part).await.is_err() {
                            return;
                        }
                        if let Some(bps) = throttle {
                            tokio::time::sleep(Duration::from_secs_f64(part.len() as f64 / bps as f64)).await;
                        }
                    }
                }
            });
        }
    });
    Server { url: format!("http://{addr}/file.bin"), data, conns }
}

fn engine(dir: &std::path::Path, conns: u16) -> Engine {
    let e = Engine::in_memory();
    e.set_settings(Settings {
        download_dir: dir.to_string_lossy().into_owned(),
        use_category_folders: false,
        connections: conns,
        ..Settings::default()
    });
    e
}

async fn wait_for(e: &Engine, id: &str, want: Status, secs: u64) -> velox_core::DownloadInfo {
    let mut rx = e.subscribe();
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(d) = e.get(id) {
            if d.record.status == want {
                return d;
            }
            if d.record.status == Status::Failed && want != Status::Failed {
                panic!("download failed: {:?}", d.record.error);
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            panic!("timeout waiting for {want:?}; now {:?}", e.get(id).map(|d| d.record.status));
        }
        match tokio::time::timeout(left.min(Duration::from_millis(200)), rx.recv()).await {
            Ok(Ok(EngineEvent::Updated(_))) | Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_is_byte_exact() {
    let srv = serve(24 * 1024 * 1024 + 123, true, Some(4 * 1024 * 1024)).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), 16);
    let d = e.add(AddRequest { url: srv.url.clone(), ..Default::default() }).await.unwrap();
    assert!(d.record.resumable);
    assert_eq!(d.record.total, Some(srv.data.len() as u64));
    let done = wait_for(&e, &d.record.id, Status::Completed, 60).await;
    let bytes = std::fs::read(done.record.path()).unwrap();
    assert_eq!(bytes.len(), srv.data.len());
    assert!(bytes == *srv.data, "content mismatch");
    assert!(srv.conns.load(Ordering::SeqCst) >= 8, "expected parallel connections");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume() {
    // 8 MB at 1 MB/s per connection × 4 connections ≈ 2 s total.
    let srv = serve(8 * 1024 * 1024, true, Some(1024 * 1024)).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), 4);
    let d = e.add(AddRequest { url: srv.url.clone(), ..Default::default() }).await.unwrap();
    let id = d.record.id.clone();
    wait_for(&e, &id, Status::Downloading, 10).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    e.pause(&id);
    let paused = wait_for(&e, &id, Status::Paused, 10).await;
    assert!(paused.record.downloaded > 0 && paused.record.downloaded < srv.data.len() as u64);
    assert!(!paused.record.segments.is_empty());
    e.resume(&id);
    let done = wait_for(&e, &id, Status::Completed, 30).await;
    let bytes = std::fs::read(done.record.path()).unwrap();
    assert!(bytes == *srv.data, "content mismatch after resume");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_without_range_support() {
    let srv = serve(3 * 1024 * 1024 + 7, false, None).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), 16);
    let d = e.add(AddRequest { url: srv.url.clone(), ..Default::default() }).await.unwrap();
    assert!(!d.record.resumable);
    let done = wait_for(&e, &d.record.id, Status::Completed, 30).await;
    assert!(std::fs::read(done.record.path()).unwrap() == *srv.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn more_connections_are_faster() {
    // Each connection is throttled to 2 MB/s; 16 MB file.
    let size = 16 * 1024 * 1024;
    let run = |conns: u16| async move {
        let srv = serve(size, true, Some(2 * 1024 * 1024)).await;
        let dir = tempfile::tempdir().unwrap();
        let e = engine(dir.path(), conns);
        let t = Instant::now();
        let d = e.add(AddRequest { url: srv.url.clone(), ..Default::default() }).await.unwrap();
        let done = wait_for(&e, &d.record.id, Status::Completed, 60).await;
        assert!(std::fs::read(done.record.path()).unwrap() == *srv.data);
        t.elapsed()
    };
    let one = run(1).await;
    let many = run(16).await;
    eprintln!("1 connection: {one:?}, 16 connections: {many:?}");
    assert!(many * 4 < one, "16 connections should be >4x faster ({one:?} vs {many:?})");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_speed_limit_is_respected() {
    let srv = serve(3 * 1024 * 1024, true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), 8);
    let mut s = e.settings();
    s.speed_limit = 1024 * 1024; // 1 MB/s
    e.set_settings(s);
    let t = Instant::now();
    let d = e.add(AddRequest { url: srv.url.clone(), ..Default::default() }).await.unwrap();
    wait_for(&e, &d.record.id, Status::Completed, 30).await;
    let el = t.elapsed();
    assert!(el >= Duration::from_millis(2300), "finished too fast for the limit: {el:?}");
}
