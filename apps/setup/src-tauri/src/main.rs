//! Velox Setup – a custom, dark, single-window installer.
//!
//! Installs per user into `%LOCALAPPDATA%\Velox` (the same place the NSIS
//! bundle and the auto-updater use), creates shortcuts, registers in
//! "Apps & features", prepares the browser extension and can uninstall
//! everything again (`velox-setup.exe --uninstall`).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{Emitter, Window};

static APP_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/velox.exe.zst"));
static HOST_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/velox-host.exe.zst"));
include!(concat!(env!("OUT_DIR"), "/extension_files.rs"));

const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXTENSION_ID: &str = "kgajklbecejimohbelehglchfipdkkke";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Velox";
const SETUP_NAME: &str = "velox-setup.exe";

type R<T> = Result<T, String>;

fn install_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Velox")
}
fn extension_dir() -> PathBuf {
    install_dir().join("extension")
}
fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("dev.velox.app"))
}
fn start_menu_link() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs\Velox.lnk"))
}
fn desktop_link() -> Option<PathBuf> {
    dirs::desktop_dir().map(|d| d.join("Velox.lnk"))
}

fn uninstall_mode() -> bool {
    std::env::args().any(|a| a == "--uninstall")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    version: &'static str,
    dir: String,
    extension_dir: String,
    extension_id: &'static str,
    installed_version: Option<String>,
    uninstall: bool,
    lang: &'static str,
    payload: bool,
}

#[tauri::command]
fn info() -> Info {
    let lang = match sys_locale::get_locale() {
        Some(l) if l.to_ascii_lowercase().starts_with("tr") => "tr",
        _ => "en",
    };
    Info {
        version: VERSION,
        dir: install_dir().to_string_lossy().into_owned(),
        extension_dir: extension_dir().to_string_lossy().into_owned(),
        extension_id: EXTENSION_ID,
        installed_version: installed_version(),
        uninstall: uninstall_mode(),
        lang,
        payload: !APP_ZST.is_empty() && !HOST_ZST.is_empty(),
    }
}

#[cfg(windows)]
fn installed_version() -> Option<String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    if !install_dir().join("velox.exe").exists() {
        return None;
    }
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?.get_value("DisplayVersion").ok()
}
#[cfg(not(windows))]
fn installed_version() -> Option<String> {
    None
}

fn progress(w: Option<&Window>, pct: u32, step: &str) {
    if let Some(w) = w {
        let _ = w.emit("progress", json!({ "pct": pct, "step": step }));
    }
}

fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Stop a running Velox so its files can be replaced.
fn stop_running() {
    for exe in ["velox.exe", "velox-host.exe"] {
        let _ = hidden(std::process::Command::new("taskkill").args(["/F", "/T", "/IM", exe])).output();
    }
    std::thread::sleep(Duration::from_millis(400));
}

/// Write atomically, retrying briefly while Windows releases a file lock.
fn write_file(path: &Path, data: &[u8]) -> R<()> {
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut last = String::new();
    for _ in 0..20 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("{}: {last}", path.display()))
}

#[cfg(windows)]
fn shortcut(target: &Path, link: &Path) -> R<()> {
    if let Some(dir) = link.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut sl = mslnk::ShellLink::new(target).map_err(|e| e.to_string())?;
    sl.set_working_dir(target.parent().map(|p| p.to_string_lossy().into_owned()));
    sl.set_name(Some("Velox".into()));
    sl.create_lnk(link).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn register_uninstall(dir: &Path) -> R<()> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY).map_err(|e| e.to_string())?;
    let setup = dir.join(SETUP_NAME);
    let size_kb: u32 = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>() / 1024)
        .unwrap_or(0) as u32;
    let set = |k: &str, v: &str| key.set_value(k, &v.to_string()).map_err(|e| e.to_string());
    set("DisplayName", "Velox")?;
    set("DisplayVersion", VERSION)?;
    set("Publisher", "Ethem Demirkaya")?;
    set("DisplayIcon", &dir.join("velox.exe").to_string_lossy())?;
    set("InstallLocation", &dir.to_string_lossy())?;
    set("UninstallString", &format!("\"{}\" --uninstall", setup.display()))?;
    set("QuietUninstallString", &format!("\"{}\" --uninstall", setup.display()))?;
    set("URLInfoAbout", "https://github.com/ethemdemirkaya/velox")?;
    key.set_value("NoModify", &1u32).map_err(|e| e.to_string())?;
    key.set_value("NoRepair", &1u32).map_err(|e| e.to_string())?;
    key.set_value("EstimatedSize", &size_kb).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallOptions {
    desktop: bool,
    extension: bool,
}

fn do_install(w: Option<&Window>, opts: InstallOptions) -> R<()> {
    if APP_ZST.is_empty() || HOST_ZST.is_empty() {
        return Err("This installer was built without the Velox payload.".into());
    }
    let dir = install_dir();
    progress(w, 4, "closing");
    stop_running();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    progress(w, 12, "extracting");
    let app = zstd::decode_all(APP_ZST).map_err(|e| e.to_string())?;
    progress(w, 42, "copying");
    write_file(&dir.join("velox.exe"), &app)?;
    let host = zstd::decode_all(HOST_ZST).map_err(|e| e.to_string())?;
    write_file(&dir.join("velox-host.exe"), &host)?;

    progress(w, 64, "extension");
    if opts.extension {
        let ext = extension_dir();
        for (rel, data) in EXTENSION {
            let p = ext.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, data).map_err(|e| e.to_string())?;
        }
    }

    progress(w, 76, "shortcuts");
    #[cfg(windows)]
    {
        let target = dir.join("velox.exe");
        if let Some(l) = start_menu_link() {
            shortcut(&target, &l)?;
        }
        if opts.desktop {
            if let Some(l) = desktop_link() {
                shortcut(&target, &l)?;
            }
        }
    }

    progress(w, 88, "registering");
    if let Ok(me) = std::env::current_exe() {
        let dest = dir.join(SETUP_NAME);
        if me != dest {
            let _ = std::fs::copy(&me, &dest);
        }
    }
    #[cfg(windows)]
    register_uninstall(&dir)?;

    std::thread::sleep(Duration::from_millis(250));
    progress(w, 100, "done");
    Ok(())
}

#[tauri::command]
async fn install(window: Window, opts: InstallOptions) -> R<()> {
    tauri::async_runtime::spawn_blocking(move || do_install(Some(&window), opts)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn launch() -> R<()> {
    let exe = install_dir().join("velox.exe");
    std::process::Command::new(&exe).current_dir(install_dir()).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_path(path: String) -> R<()> {
    std::process::Command::new("explorer").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn remove_registry() {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
    for browser in [
        r"Software\Google\Chrome",
        r"Software\Microsoft\Edge",
        r"Software\Chromium",
        r"Software\BraveSoftware\Brave-Browser",
        r"Software\Vivaldi",
    ] {
        let _ = hkcu.delete_subkey_all(format!(r"{browser}\NativeMessagingHosts\com.velox.host"));
    }
    if let Ok(run) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", winreg::enums::KEY_ALL_ACCESS) {
        let _ = run.delete_value("Velox");
    }
}

fn do_uninstall(w: Option<&Window>, remove_data: bool) -> R<()> {
    let dir = install_dir();
    progress(w, 10, "closing");
    stop_running();
    progress(w, 40, "removing");
    for l in [start_menu_link(), desktop_link()].into_iter().flatten() {
        let _ = std::fs::remove_file(l);
    }
    #[cfg(windows)]
    remove_registry();
    let me = std::env::current_exe().ok();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if Some(&p) == me.as_ref() {
                continue;
            }
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
    progress(w, 80, "data");
    if remove_data {
        if let Some(d) = data_dir() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    // The uninstaller cannot delete itself while running: remove the folder
    // a moment after we exit.
    let _ = hidden(std::process::Command::new("cmd").args([
        "/C",
        &format!("ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"", dir.display()),
    ]))
    .spawn();
    progress(w, 100, "removed");
    Ok(())
}

#[tauri::command]
async fn uninstall(window: Window, remove_data: bool) -> R<()> {
    tauri::async_runtime::spawn_blocking(move || do_uninstall(Some(&window), remove_data)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    // Unattended mode for scripts / IT: `velox-setup.exe /S` installs with
    // defaults, `velox-setup.exe --uninstall /S` removes (keeping user data).
    if std::env::args().any(|a| a.eq_ignore_ascii_case("/S") || a == "--silent") {
        let res = if uninstall_mode() {
            do_uninstall(None, false)
        } else {
            do_install(None, InstallOptions { desktop: true, extension: true })
        };
        if let Err(e) = res {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![info, install, launch, open_path, uninstall, quit])
        .run(tauri::generate_context!())
        .expect("error while running Velox Setup");
}
