//! Speed behaviour: a straggling connection must be replaced instead of
//! holding the whole download hostage.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use velox_core::{AddRequest, Engine, Settings, Status};

/// Range server where the *first* request for `slow_start` is throttled to a
/// crawl (a bad CDN node); every later request is fast.
async fn serve(data: Arc<Vec<u8>>, slow_start: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let used = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let (data, used) = (data.clone(), used.clone());
            tokio::spawn(async move {
                loop {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 4096];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    let text = String::from_utf8_lossy(&req).to_lowercase();
                    let total = data.len();
                    let (a, b) = text
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .map(|r| {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { total - 1 } else { b.parse().unwrap() };
                            (a, b.min(total - 1))
                        })
                        .unwrap_or((0, total - 1));
                    let slow = a == slow_start && !used.swap(true, Ordering::SeqCst);
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{total}\r\nAccept-Ranges: bytes\r\n\r\n",
                        b + 1 - a
                    );
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    for part in data[a..=b].chunks(16 * 1024) {
                        if sock.write_all(part).await.is_err() {
                            return;
                        }
                        // fast connections: ~4 MB/s each; the straggler: 32 KB/s
                        let bps = if slow { 32 * 1024 } else { 4 * 1024 * 1024 };
                        tokio::time::sleep(Duration::from_secs_f64(part.len() as f64 / bps as f64)).await;
                    }
                }
            });
        }
    });
    format!("http://{addr}/file.bin")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn straggler_connection_is_replaced() {
    let size = 48 * 1024 * 1024;
    let data: Arc<Vec<u8>> = Arc::new((0..size).map(|i| (i % 251) as u8).collect());
    // 8 connections → 6 MiB segments; make the 2nd one crawl.
    let url = serve(data.clone(), 6 * 1024 * 1024).await;
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::in_memory();
    e.set_settings(Settings { download_dir: dir.path().to_string_lossy().into_owned(), use_category_folders: false, ..Settings::default() });
    let t = Instant::now();
    let d = e.add(AddRequest { url, connections: Some(8), ..Default::default() }).await.unwrap();
    loop {
        let d = e.get(&d.record.id).unwrap();
        match d.record.status {
            Status::Completed => {
                assert!(std::fs::read(d.record.path()).unwrap() == *data, "content mismatch");
                break;
            }
            Status::Failed => panic!("{:?}", d.record.error),
            _ => {}
        }
        // Without replacement the straggler alone needs > 60 s.
        assert!(t.elapsed() < Duration::from_secs(25), "straggler was not replaced ({:?})", t.elapsed());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("finished in {:?}", t.elapsed());
}
