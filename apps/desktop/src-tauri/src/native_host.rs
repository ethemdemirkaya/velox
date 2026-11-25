//! Registers the `velox-host` native messaging host with Chromium browsers so
//! the extension can reach us. Runs on every start (cheap, idempotent) so the
//! path stays correct after updates or moving the install folder.

use std::path::{Path, PathBuf};

pub const HOST_NAME: &str = "com.velox.host";
/// IDs of the extension builds allowed to talk to the host. The first one is
/// pinned by the `key` in `extension/manifest.json` (unpacked / GitHub build).
pub const EXTENSION_IDS: &[&str] = &["kgajklbecejimohbelehglchfipdkkke"];

pub fn host_exe() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if cfg!(windows) { "velox-host.exe" } else { "velox-host" };
    let p = dir.join(name);
    p.exists().then_some(p)
}

fn manifest(host: &Path) -> String {
    let origins: Vec<String> = EXTENSION_IDS.iter().map(|id| format!("chrome-extension://{id}/")).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "Velox download manager bridge",
        "path": host,
        "type": "stdio",
        "allowed_origins": origins,
    }))
    .unwrap_or_default()
}

/// Returns true when the host is registered.
pub fn register() -> bool {
    // Development builds must never take over the browser bridge from the
    // installed app (their UI is served by a dev server that may be down).
    if cfg!(debug_assertions) {
        return host_exe().is_some();
    }
    let Some(host) = host_exe() else {
        tracing::warn!("velox-host binary not found next to the app; browser integration disabled");
        return false;
    };
    let json = manifest(&host);
    imp::register(&json)
}

#[cfg(windows)]
mod imp {
    use super::HOST_NAME;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    pub fn register(json: &str) -> bool {
        let Some(dir) = dirs::data_local_dir().map(|d| d.join("Velox")) else { return false };
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        let file = dir.join(format!("{HOST_NAME}.json"));
        if std::fs::write(&file, json).is_err() {
            return false;
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut ok = false;
        for browser in [
            r"Software\Google\Chrome",
            r"Software\Microsoft\Edge",
            r"Software\Chromium",
            r"Software\BraveSoftware\Brave-Browser",
            r"Software\Vivaldi",
        ] {
            let path = format!(r"{browser}\NativeMessagingHosts\{HOST_NAME}");
            if let Ok((key, _)) = hkcu.create_subkey(&path) {
                ok |= key.set_value("", &file.to_string_lossy().to_string()).is_ok();
            }
        }
        ok
    }
}

#[cfg(not(windows))]
mod imp {
    use super::HOST_NAME;

    pub fn register(json: &str) -> bool {
        let Some(home) = dirs::home_dir() else { return false };
        #[cfg(target_os = "macos")]
        let dirs = [
            "Library/Application Support/Google/Chrome",
            "Library/Application Support/Microsoft Edge",
            "Library/Application Support/Chromium",
            "Library/Application Support/BraveSoftware/Brave-Browser",
        ];
        #[cfg(not(target_os = "macos"))]
        let dirs = [".config/google-chrome", ".config/microsoft-edge", ".config/chromium", ".config/BraveSoftware/Brave-Browser"];
        let mut ok = false;
        for d in dirs {
            let dir = home.join(d).join("NativeMessagingHosts");
            if std::fs::create_dir_all(&dir).is_ok() {
                ok |= std::fs::write(dir.join(format!("{HOST_NAME}.json")), json).is_ok();
            }
        }
        ok
    }
}
