//! `velox-host` – Chrome/Edge/Brave native messaging host.
//!
//! The browser starts this tiny process for every `sendNativeMessage` call and
//! talks to it over stdin/stdout using 4-byte little-endian length-prefixed
//! JSON. We forward each message as one JSON line to the running Velox app
//! over a local socket (named pipe on Windows) and relay the answer back.
//! If the app is not running we launch it in the background first.

use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

pub const SOCKET_NAME: &str = "velox-ipc.sock";
const MAX_MSG: usize = 8 * 1024 * 1024;

fn read_message(r: &mut impl Read) -> io::Result<Option<Value>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MSG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_message(w: &mut impl Write, v: &Value) -> io::Result<()> {
    let data = serde_json::to_vec(v)?;
    w.write_all(&(data.len() as u32).to_le_bytes())?;
    w.write_all(&data)?;
    w.flush()
}

fn connect() -> io::Result<Stream> {
    let name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    Stream::connect(name)
}

fn launch_app() {
    let Ok(me) = std::env::current_exe() else { return };
    let Some(dir) = me.parent() else { return };
    let candidates: &[&str] = if cfg!(windows) { &["velox.exe", "Velox.exe"] } else { &["velox", "Velox"] };
    for c in candidates {
        let p = dir.join(c);
        if p.exists() {
            let mut cmd = std::process::Command::new(p);
            cmd.arg("--background");
            cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
            }
            let _ = cmd.spawn();
            return;
        }
    }
}

fn connect_or_launch(launch: bool) -> io::Result<Stream> {
    if let Ok(s) = connect() {
        return Ok(s);
    }
    if !launch {
        return connect();
    }
    launch_app();
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        std::thread::sleep(Duration::from_millis(150));
        match connect() {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() > deadline => return Err(e),
            Err(_) => {}
        }
    }
}

fn forward(msg: &Value) -> Value {
    let kind = msg.get("type").and_then(Value::as_str).unwrap_or("");
    // Background lookups must not spawn the app – only user actions do.
    let launch = !matches!(kind, "ping" | "inspect" | "prefetch");
    let stream = match connect_or_launch(launch) {
        Ok(s) => s,
        Err(e) => {
            if kind == "ping" {
                return json!({ "ok": true, "app": false, "host": env!("CARGO_PKG_VERSION") });
            }
            return json!({ "ok": false, "error": format!("Velox is not running: {e}") });
        }
    };
    let mut conn = BufReader::new(stream);
    let mut line = serde_json::to_string(msg).unwrap_or_default();
    line.push('\n');
    if let Err(e) = conn.get_mut().write_all(line.as_bytes()) {
        return json!({ "ok": false, "error": e.to_string() });
    }
    let mut resp = String::new();
    match conn.read_line(&mut resp) {
        Ok(0) | Err(_) => json!({ "ok": false, "error": "no response from Velox" }),
        Ok(_) => serde_json::from_str(&resp).unwrap_or_else(|_| json!({ "ok": false, "error": "bad response" })),
    }
}

fn main() {
    // The extension keeps one port open; requests are handled concurrently so
    // a slow one (launching the app, resolving a video) never blocks pings.
    let out = std::sync::Arc::new(std::sync::Mutex::new(io::stdout()));
    let mut input = io::stdin().lock();
    while let Ok(Some(msg)) = read_message(&mut input) {
        let out = out.clone();
        std::thread::spawn(move || {
            let mut resp = forward(&msg);
            if let Some(id) = msg.get("requestId") {
                resp["requestId"] = id.clone();
            }
            if let Ok(mut o) = out.lock() {
                let _ = write_message(&mut *o, &resp);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"type":"ping"})).unwrap();
        let v = read_message(&mut &buf[..]).unwrap().unwrap();
        assert_eq!(v["type"], "ping");
    }
}
