//! HLS end-to-end: master playlist → variant → AES-128 segments, plus pause/resume.

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use velox_core::{AddRequest, Engine, Kind, Settings, Status};

const KEY: [u8; 16] = *b"velox-secret-key";
const SEGMENTS: usize = 40;
const SEG_SIZE: usize = 96 * 1024;

fn plain_segment(i: usize) -> Vec<u8> {
    (0..SEG_SIZE).map(|j| ((i * 7 + j * 13) % 251) as u8).collect()
}

fn encrypt(i: usize, data: &[u8]) -> Vec<u8> {
    // No IV attribute in the playlist → IV is the media sequence number.
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&(i as u64).to_be_bytes());
    let mut buf = data.to_vec();
    buf.resize(data.len() + 16, 0);
    let n = cbc::Encryptor::<aes::Aes128>::new(&KEY.into(), &iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .unwrap()
        .len();
    buf.truncate(n);
    buf
}

async fn serve(delay_ms: u64) -> String {
    let mut routes: HashMap<String, Vec<u8>> = HashMap::new();
    routes.insert("/master.m3u8".into(), b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=500000\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000\nhigh/index.m3u8\n".to_vec());
    let mut media = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\"\n");
    for i in 0..SEGMENTS {
        media += &format!("#EXTINF:4.0,\nseg{i}.ts\n");
        routes.insert(format!("/high/seg{i}.ts"), encrypt(i, &plain_segment(i)));
    }
    media += "#EXT-X-ENDLIST\n";
    routes.insert("/high/index.m3u8".into(), media.into_bytes());
    routes.insert("/low/index.m3u8".into(), b"#EXTM3U\n#EXT-X-ENDLIST\n".to_vec());
    routes.insert("/key.bin".into(), KEY.to_vec());
    let routes = Arc::new(routes);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let routes = routes.clone();
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
                    let line = String::from_utf8_lossy(&req);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    if path.ends_with(".ts") && delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let resp = match routes.get(&path) {
                        Some(body) => {
                            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
                            r.extend_from_slice(body);
                            r
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    if sock.write_all(&resp).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    format!("http://{addr}/master.m3u8")
}

fn expected() -> Vec<u8> {
    (0..SEGMENTS).flat_map(plain_segment).collect()
}

fn engine(dir: &std::path::Path) -> Engine {
    let e = Engine::in_memory();
    e.set_settings(Settings { download_dir: dir.to_string_lossy().into_owned(), use_category_folders: false, connections: 8, ..Settings::default() });
    e
}

async fn wait(e: &Engine, id: &str, want: Status, secs: u64) -> velox_core::DownloadInfo {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let d = e.get(id).unwrap();
        if d.record.status == want {
            return d;
        }
        assert!(d.record.status != Status::Failed || want == Status::Failed, "failed: {:?}", d.record.error);
        assert!(Instant::now() < deadline, "timeout waiting for {want:?}, now {:?}", d.record.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Output may have been remuxed to mp4 when ffmpeg is installed; compare only
/// when the raw transport stream was kept.
fn check_output(d: &velox_core::DownloadInfo) {
    let path = d.record.path();
    if path.extension().and_then(|e| e.to_str()) == Some("ts") {
        let got = std::fs::read(&path).unwrap();
        assert_eq!(got.len(), SEGMENTS * SEG_SIZE);
        assert!(got == expected(), "decrypted content mismatch");
    } else {
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_aes128_master_playlist() {
    let url = serve(0).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let d = e.add(AddRequest { url, kind: Some(Kind::Hls), file_name: Some("movie.mp4".into()), ..Default::default() }).await.unwrap();
    let done = wait(&e, &d.record.id, Status::Completed, 30).await;
    assert_eq!(done.record.hls_count, SEGMENTS as u64);
    check_output(&done);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_pause_resume_is_exact() {
    let url = serve(40).await;
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let d = e.add(AddRequest { url, kind: Some(Kind::Hls), file_name: Some("movie.mp4".into()), ..Default::default() }).await.unwrap();
    let id = d.record.id.clone();
    let deadline = Instant::now() + Duration::from_secs(20);
    while e.get(&id).unwrap().record.hls_done < 8 {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    e.pause(&id);
    let paused = wait(&e, &id, Status::Paused, 10).await;
    assert!(paused.record.hls_done > 0 && paused.record.hls_done < SEGMENTS as u64);
    assert_eq!(paused.record.hls_bytes, paused.record.hls_done * SEG_SIZE as u64, "resume point must match disk");
    e.resume(&id);
    let done = wait(&e, &id, Status::Completed, 30).await;
    check_output(&done);
}
