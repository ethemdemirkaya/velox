//! HLS master playlists from embedded players: quality list, picking a
//! quality, and CDNs that only answer requests carrying the player's Referer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use velox_core::{AddRequest, Engine, Kind, Settings, Status};

const SEGMENTS: usize = 12;
const PLAYER: &str = "http://player.test/embed/42";

struct Server {
    master: String,
    hits: Arc<Mutex<Vec<String>>>,
}

async fn serve() -> Server {
    let mut routes: HashMap<String, Vec<u8>> = HashMap::new();
    let mut master = String::from("#EXTM3U\n");
    for (h, bw) in [(360u32, 800_000u64), (1080, 5_000_000), (720, 2_500_000)] {
        master += &format!("#EXT-X-STREAM-INF:BANDWIDTH={bw},RESOLUTION={}x{h}\nv{h}/index.m3u8\n", h * 16 / 9);
        let mut media = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n");
        for i in 0..SEGMENTS {
            media += &format!("#EXTINF:6.0,\ns{i}.ts\n");
            routes.insert(format!("/v{h}/s{i}.ts"), vec![(h % 251) as u8; 64 * 1024]);
        }
        media += "#EXT-X-ENDLIST\n";
        routes.insert(format!("/v{h}/index.m3u8"), media.into_bytes());
    }
    routes.insert("/master.m3u8".into(), master.into_bytes());
    let routes = Arc::new(routes);
    let hits = Arc::new(Mutex::new(Vec::new()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h2 = hits.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let (routes, hits) = (routes.clone(), h2.clone());
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
                    let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                    // Hotlink protection: only the embedding player may fetch.
                    let allowed = text.contains(&format!("referer: {}", PLAYER.to_lowercase())) && text.contains("origin: http://player.test");
                    let resp = match routes.get(&path) {
                        Some(body) if allowed => {
                            hits.lock().unwrap().push(path.clone());
                            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            r.extend_from_slice(body);
                            r
                        }
                        Some(_) => b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_vec(),
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    if sock.write_all(&resp).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Server { master: format!("http://{addr}/master.m3u8"), hits }
}

fn request(url: &str, quality: Option<&str>) -> AddRequest {
    AddRequest {
        url: url.into(),
        kind: Some(Kind::Hls),
        file_name: Some("episode.mp4".into()),
        referrer: Some(PLAYER.into()),
        headers: vec![("Origin".into(), "http://player.test".into())],
        quality: quality.map(Into::into),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn master_playlist_offers_qualities_and_honours_choice() {
    let srv = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::in_memory();
    e.set_settings(Settings { download_dir: dir.path().to_string_lossy().into_owned(), use_category_folders: false, ..Settings::default() });

    let p = e.probe(request(&srv.master, None)).await.unwrap();
    let ids: Vec<_> = p.qualities.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, ["1080", "720", "360"], "best first");
    // 72 s × 2.5 Mbit/s ≈ 22.5 MB
    let s720 = p.qualities[1].size.unwrap();
    assert!((22_000_000..23_000_000).contains(&s720), "estimate {s720}");

    srv.hits.lock().unwrap().clear();
    let d = e.add(request(&srv.master, Some("720"))).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let r = e.get(&d.record.id).unwrap().record;
        assert!(r.status != Status::Failed, "failed: {:?}", r.error);
        if r.status == Status::Completed {
            break;
        }
        assert!(Instant::now() < deadline, "timeout, status {:?}", r.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let hits = srv.hits.lock().unwrap().clone();
    let segs: Vec<_> = hits.iter().filter(|h| h.ends_with(".ts")).collect();
    assert_eq!(segs.len(), SEGMENTS);
    assert!(segs.iter().all(|h| h.starts_with("/v720/")), "wrong variant: {segs:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_player_headers_are_rejected() {
    // Sanity check of the test server itself: without the player's Referer
    // the CDN refuses, so the test above really exercises header replay.
    let srv = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::in_memory();
    e.set_settings(Settings { download_dir: dir.path().to_string_lossy().into_owned(), use_category_folders: false, ..Settings::default() });
    let p = e.probe(AddRequest { url: srv.master.clone(), kind: Some(Kind::Hls), ..Default::default() }).await.unwrap();
    assert!(p.qualities.is_empty());
}

/// Players that keep audio in a separate rendition (EXT-X-MEDIA): without
/// handling it the download would be silent. Needs ffmpeg (skipped otherwise).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn separate_audio_rendition_is_merged() {
    let ff = |args: &[&str], cwd: &std::path::Path| {
        std::process::Command::new("ffmpeg").args(["-y", "-loglevel", "error"]).args(args).current_dir(cwd).status().map(|s| s.success()).unwrap_or(false)
    };
    let src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src.path().join("v")).unwrap();
    std::fs::create_dir_all(src.path().join("a")).unwrap();
    let hls = ["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"];
    let ok = ff(&[&["-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=3", "-an", "-c:v", "libx264", "-g", "25"][..], &hls, &["v/index.m3u8"]].concat(), src.path())
        && ff(&[&["-f", "lavfi", "-i", "sine=duration=3", "-vn", "-c:a", "aac"][..], &hls, &["a/index.m3u8"]].concat(), src.path());
    if !ok {
        eprintln!("ffmpeg not available – skipping");
        return;
    }
    std::fs::write(
        src.path().join("master.m3u8"),
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"main\",DEFAULT=YES,URI=\"a/index.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=300000,RESOLUTION=320x180,AUDIO=\"a\"\nv/index.m3u8\n",
    )
    .unwrap();

    // Static file server for the generated stream.
    let root = src.path().to_path_buf();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let root = root.clone();
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
                    let text = String::from_utf8_lossy(&req).to_string();
                    let path = text.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                    let resp = match std::fs::read(root.join(&path)) {
                        Ok(body) => {
                            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            r.extend_from_slice(&body);
                            r
                        }
                        Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    if sock.write_all(&resp).await.is_err() {
                        return;
                    }
                }
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let e = Engine::in_memory();
    e.set_settings(Settings { download_dir: dir.path().to_string_lossy().into_owned(), use_category_folders: false, ..Settings::default() });
    let d = e
        .add(AddRequest { url: format!("http://{addr}/master.m3u8"), kind: Some(Kind::Hls), file_name: Some("clip.mp4".into()), ..Default::default() })
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let rec = loop {
        let r = e.get(&d.record.id).unwrap().record;
        assert!(r.status != Status::Failed, "failed: {:?}", r.error);
        if r.status == Status::Completed {
            break r;
        }
        assert!(Instant::now() < deadline, "timeout, status {:?}", r.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let out = rec.path();
    let probe = std::process::Command::new("ffmpeg").arg("-i").arg(&out).output().unwrap();
    let info = String::from_utf8_lossy(&probe.stderr);
    assert!(info.contains("Video:") && info.contains("Audio:"), "missing a stream in {}:\n{info}", out.display());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "temporary files left: {leftovers:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_describes_streams_for_the_browser() {
    let srv = serve().await;
    let e = Engine::in_memory();
    let master = e.inspect(request(&srv.master, None)).await;
    assert!(master.ok && master.master);
    assert_eq!(master.height, Some(1080));
    assert_eq!(master.duration, Some(72.0));
    assert_eq!(master.children.len(), 3);
    let variant = srv.master.replace("master.m3u8", "v720/index.m3u8");
    assert!(master.children.contains(&variant), "{:?}", master.children);

    let v = e.inspect(request(&variant, None)).await;
    assert!(v.ok && !v.master);
    let v2 = e.inspect(request(&srv.master.replace("master.m3u8", "v360/index.m3u8"), None)).await;
    assert_eq!(v.fingerprint, v2.fingerprint, "same timeline → same fingerprint");

    let refused = e.inspect(AddRequest { url: srv.master.clone(), ..Default::default() }).await;
    assert!(!refused.ok);
}
