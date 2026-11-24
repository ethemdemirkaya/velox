//! Download a URL with the Velox engine and report throughput.
//!
//!     cargo run --release -p velox-core --example bench -- <url> [connections]

use std::time::{Duration, Instant};
use velox_core::{AddRequest, Engine, Settings, Status};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("usage: bench <url> [connections]");
    let conns: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(16);
    let dir = std::env::temp_dir().join("velox-bench");
    let _ = std::fs::remove_dir_all(&dir);

    let e = Engine::in_memory();
    e.set_settings(Settings {
        download_dir: dir.to_string_lossy().into_owned(),
        use_category_folders: false,
        connections: conns,
        ..Settings::default()
    });

    let t = Instant::now();
    let d = e.add(AddRequest { url, ..Default::default() }).await.expect("add");
    println!("{} · {:?} bytes · resumable={} · {conns} connections", d.record.file_name, d.record.total, d.record.resumable);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let d = e.get(&d.record.id).unwrap();
        let mb = d.record.downloaded as f64 / 1048576.0;
        print!("\r{mb:8.1} MB  {:7.2} MB/s  conns {:2}   ", d.speed / 1048576.0, d.active_connections);
        use std::io::Write;
        std::io::stdout().flush().ok();
        match d.record.status {
            Status::Completed => {
                let s = t.elapsed().as_secs_f64();
                println!("\ndone in {s:.2}s → average {:.2} MB/s", mb / s);
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
            Status::Failed => {
                println!("\nfailed: {:?}", d.record.error);
                std::process::exit(1);
            }
            _ => {}
        }
    }
}
