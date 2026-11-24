use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Make a string safe to use as a file name on every OS.
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_end_matches('.').to_string();
    if out.is_empty() {
        out = "download".into();
    }
    if out.chars().count() > 200 {
        let (stem, ext) = split_ext(&out);
        let keep: String = stem.chars().take(180).collect();
        out = format!("{keep}{ext}");
    }
    out
}

pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 10 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Parse a Content-Disposition header (RFC 6266 / 5987).
pub fn filename_from_disposition(v: &str) -> Option<String> {
    let mut plain = None;
    for part in v.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("filename*=") {
            let rest = part[10..].trim().trim_matches('"');
            let enc = rest.splitn(3, '\'').nth(2).unwrap_or(rest);
            let decoded = percent_decode_str(enc).decode_utf8_lossy().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        } else if lower.starts_with("filename=") {
            let raw = part[9..].trim().trim_matches('"');
            plain = Some(percent_decode_str(raw).decode_utf8_lossy().to_string());
        }
    }
    plain.filter(|s| !s.is_empty())
}

pub fn filename_from_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let seg = u.path_segments()?.rfind(|s| !s.is_empty())?;
    let name = percent_decode_str(seg).decode_utf8_lossy().to_string();
    (!name.is_empty()).then_some(name)
}

pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next()?.trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "application/zip" => "zip",
        "application/pdf" => "pdf",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/gzip" => "gz",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "text/plain" => "txt",
        "application/json" => "json",
        "application/x-iso9660-image" => "iso",
        _ => return None,
    })
}

/// `dir/name` – or `dir/name (1).ext` etc. if it (or its part file) already exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() && !part_path(&p).exists() {
        return p;
    }
    let (stem, ext) = split_ext(name);
    for i in 1..10_000 {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if !p.exists() && !part_path(&p).exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", uuid::Uuid::new_v4()))
}

pub fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".velox");
    PathBuf::from(s)
}

/// Positional write that is safe on a handle shared across threads.
pub fn write_at(f: &std::fs::File, buf: &[u8], off: u64) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let (mut buf, mut off) = (buf, off);
        while !buf.is_empty() {
            let n = f.seek_write(buf, off)?;
            if n == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            buf = &buf[n..];
            off += n as u64;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(buf, off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disposition() {
        assert_eq!(filename_from_disposition("attachment; filename=\"a b.zip\"").unwrap(), "a b.zip");
        assert_eq!(
            filename_from_disposition("attachment; filename=x.zip; filename*=UTF-8''%C3%A7%C3%B6p.zip").unwrap(),
            "çöp.zip"
        );
    }
    #[test]
    fn url_names() {
        assert_eq!(filename_from_url("https://x.com/a/b%20c.iso?x=1").unwrap(), "b c.iso");
        assert!(filename_from_url("https://x.com/").is_none());
    }
    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("a:b?.exe"), "a_b_.exe");
        assert_eq!(sanitize("  "), "download");
    }
}
