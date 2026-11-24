//! Watches the clipboard for download links and tells the UI about them.

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

pub fn looks_like_download(text: &str, exts: &[String]) -> bool {
    let t = text.trim();
    if t.len() > 4096 || t.contains(char::is_whitespace) {
        return false;
    }
    let Ok(u) = url::Url::parse(t) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let path = u.path().to_ascii_lowercase();
    let Some((_, ext)) = path.rsplit_once('.') else { return false };
    ext == "m3u8" || exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("velox-clipboard".into())
        .spawn(move || {
            let Ok(mut cb) = arboard::Clipboard::new() else { return };
            let mut last = cb.get_text().unwrap_or_default();
            loop {
                std::thread::sleep(Duration::from_millis(800));
                let settings = app.state::<AppState>().engine.settings();
                if !settings.clipboard_watch {
                    continue;
                }
                let Ok(text) = cb.get_text() else { continue };
                if text == last {
                    continue;
                }
                last = text.clone();
                if looks_like_download(&text, &settings.capture_extensions) {
                    let _ = app.emit("clipboard-url", text.trim().to_string());
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects() {
        let e = vec!["zip".to_string()];
        assert!(looks_like_download("https://x.org/a.zip", &e));
        assert!(looks_like_download("https://x.org/live/index.m3u8?t=1", &e));
        assert!(!looks_like_download("https://x.org/page.html", &e));
        assert!(!looks_like_download("hello world", &e));
    }
}
