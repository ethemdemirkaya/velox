use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Video,
    Music,
    Documents,
    Programs,
    Compressed,
    Images,
    Other,
}

impl Category {
    pub fn folder(self) -> &'static str {
        match self {
            Category::Video => "Video",
            Category::Music => "Music",
            Category::Documents => "Documents",
            Category::Programs => "Programs",
            Category::Compressed => "Compressed",
            Category::Images => "Images",
            Category::Other => "Other",
        }
    }

    pub fn from_file_name(name: &str) -> Category {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        Self::from_ext(&ext)
    }

    pub fn from_ext(ext: &str) -> Category {
        match ext {
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "wmv" | "flv" | "m4v" | "ts" | "mpg" | "mpeg" | "3gp"
            | "m3u8" | "mpd" => Category::Video,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" | "wma" | "alac" => Category::Music,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "odt" | "rtf" | "csv"
            | "md" => Category::Documents,
            "exe" | "msi" | "msix" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" | "bat" | "sh" => {
                Category::Programs
            }
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "iso" | "img" | "tgz" => {
                Category::Compressed
            }
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "tiff" | "avif" | "heic" | "ico" => {
                Category::Images
            }
            _ => Category::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categorizes() {
        assert_eq!(Category::from_file_name("a.MP4"), Category::Video);
        assert_eq!(Category::from_file_name("setup.exe"), Category::Programs);
        assert_eq!(Category::from_file_name("noext"), Category::Other);
    }
}
