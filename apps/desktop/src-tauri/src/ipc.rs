//! Local socket server (named pipe on Windows) used by `velox-host`.
//! Protocol: one JSON object per line in each direction.

use interprocess::local_socket::tokio::{prelude::*, Stream};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use velox_core::AddRequest;

use crate::AppState;

pub const SOCKET_NAME: &str = "velox-ipc.sock";

pub async fn serve(app: AppHandle) {
    let name = match SOCKET_NAME.to_ns_name::<GenericNamespaced>() {
        Ok(n) => n,
        Err(e) => return tracing::error!("ipc name: {e}"),
    };
    let listener = match ListenerOptions::new().name(name).create_tokio() {
        Ok(l) => l,
        Err(e) => return tracing::error!("ipc listen failed: {e}"),
    };
    loop {
        match listener.accept().await {
            Ok(conn) => {
                let app = app.clone();
                tokio::spawn(handle(app, conn));
            }
            Err(e) => {
                tracing::warn!("ipc accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

async fn handle(app: AppHandle, conn: Stream) {
    // `&Stream` implements both AsyncRead and AsyncWrite.
    let mut r = BufReader::new(&conn);
    let mut w = &conn;
    let mut line = String::new();
    while let Ok(n) = r.read_line(&mut line).await {
        if n == 0 {
            break;
        }
        let resp = process(&app, line.trim()).await;
        let mut out = resp.to_string();
        out.push('\n');
        if w.write_all(out.as_bytes()).await.is_err() {
            break;
        }
        line.clear();
    }
}

async fn process(app: &AppHandle, line: &str) -> Value {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return json!({ "ok": false, "error": "invalid json" });
    };
    let state = app.state::<AppState>();
    match msg.get("type").and_then(Value::as_str).unwrap_or("") {
        "ping" => {
            let s = state.engine.settings();
            json!({
                "ok": true,
                "app": true,
                "version": env!("CARGO_PKG_VERSION"),
                "capture": {
                    "enabled": s.capture_enabled,
                    "extensions": s.capture_extensions,
                    "minSizeKb": s.capture_min_size_kb,
                }
            })
        }
        "add" => match serde_json::from_value::<AddRequest>(msg.clone()) {
            Ok(req) if !req.url.is_empty() => {
                let silent = msg.get("silent").and_then(Value::as_bool).unwrap_or(false);
                crate::capture(app, req, silent).await;
                json!({ "ok": true })
            }
            _ => json!({ "ok": false, "error": "missing url" }),
        },
        "prefetch" => {
            if let Some(url) = msg.get("url").and_then(Value::as_str) {
                state.engine.prefetch(url);
            }
            json!({ "ok": true })
        }
        "show" => {
            crate::show_main(app);
            json!({ "ok": true })
        }
        other => json!({ "ok": false, "error": format!("unknown message type: {other}") }),
    }
}
