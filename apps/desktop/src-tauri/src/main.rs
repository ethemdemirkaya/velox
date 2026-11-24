#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod ipc;
mod native_host;

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use velox_core::{AddRequest, DownloadInfo, Engine, EngineEvent, ProbeInfo, Settings, Status};

pub struct AppState {
    pub engine: Engine,
    pending: Mutex<HashMap<String, AddRequest>>,
    host_registered: bool,
}

type R<T> = Result<T, String>;

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// A download handed to us by the browser extension.
pub async fn capture(app: &AppHandle, req: AddRequest, silent: bool) {
    let state = app.state::<AppState>();
    if state.engine.settings().confirm_new && !silent {
        let token = uuid::Uuid::new_v4().simple().to_string();
        state.pending.lock().insert(token.clone(), req);
        let url = WebviewUrl::App(format!("index.html#/confirm/{token}").into());
        let res = WebviewWindowBuilder::new(app, format!("confirm-{token}"), url)
            .title("Velox")
            .inner_size(540.0, 392.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .always_on_top(true)
            .center()
            .focused(true)
            .background_color(tauri::window::Color(10, 10, 10, 255))
            .build();
        if let Err(e) = res {
            tracing::error!("confirm window: {e}");
        }
    } else {
        let engine = state.engine.clone();
        match engine.add(req).await {
            Ok(d) => notify(app, "Velox", &format!("⬇ {}", d.record.file_name)),
            Err(e) => notify(app, "Velox", &e),
        }
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let state = app.state::<AppState>();
    if state.engine.settings().notifications {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

// ───────────────────────────── commands ─────────────────────────────

#[tauri::command]
async fn list_downloads(s: State<'_, AppState>) -> R<Vec<DownloadInfo>> {
    Ok(s.engine.list())
}

#[tauri::command]
async fn add_download(s: State<'_, AppState>, req: AddRequest) -> R<DownloadInfo> {
    let e = s.engine.clone();
    e.add(req).await
}

#[tauri::command]
async fn probe_url(s: State<'_, AppState>, req: AddRequest) -> R<ProbeInfo> {
    let e = s.engine.clone();
    e.probe(req).await
}

#[tauri::command]
async fn pause(s: State<'_, AppState>, id: String) -> R<()> {
    s.engine.pause(&id);
    Ok(())
}

#[tauri::command]
async fn resume(s: State<'_, AppState>, id: String) -> R<()> {
    s.engine.resume(&id);
    Ok(())
}

#[tauri::command]
async fn remove(s: State<'_, AppState>, id: String, delete_file: bool) -> R<()> {
    s.engine.remove(&id, delete_file);
    Ok(())
}

#[tauri::command]
async fn pause_all(s: State<'_, AppState>) -> R<()> {
    s.engine.pause_all();
    Ok(())
}

#[tauri::command]
async fn resume_all(s: State<'_, AppState>) -> R<()> {
    s.engine.resume_all();
    Ok(())
}

#[tauri::command]
async fn clear_completed(s: State<'_, AppState>) -> R<()> {
    s.engine.clear_completed();
    Ok(())
}

#[tauri::command]
async fn set_download_limit(s: State<'_, AppState>, id: String, limit: u64) -> R<()> {
    s.engine.set_speed_limit(&id, limit);
    Ok(())
}

#[tauri::command]
async fn get_settings(s: State<'_, AppState>) -> R<Settings> {
    Ok(s.engine.settings())
}

#[tauri::command]
async fn set_settings(app: AppHandle, s: State<'_, AppState>, settings: Settings) -> R<Settings> {
    let old = s.engine.settings();
    if old.autostart != settings.autostart {
        let al = app.autolaunch();
        let r = if settings.autostart { al.enable() } else { al.disable() };
        if let Err(e) = r {
            tracing::warn!("autostart: {e}");
        }
    }
    Ok(s.engine.set_settings(settings))
}

fn path_of(s: &AppState, id: &str) -> R<std::path::PathBuf> {
    let d = s.engine.get(id).ok_or("not found")?;
    let p = d.record.path();
    if d.record.status == Status::Completed {
        Ok(p)
    } else {
        let mut part = p.into_os_string();
        part.push(".velox");
        Ok(part.into())
    }
}

#[tauri::command]
async fn open_file(app: AppHandle, s: State<'_, AppState>, id: String) -> R<()> {
    let p = path_of(&s, &id)?;
    app.opener().open_path(p.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn show_in_folder(app: AppHandle, s: State<'_, AppState>, id: String) -> R<()> {
    let p = path_of(&s, &id)?;
    if p.exists() {
        app.opener().reveal_item_in_dir(&p).map_err(|e| e.to_string())
    } else {
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    }
}

#[tauri::command]
async fn open_folder(app: AppHandle, path: String) -> R<()> {
    let _ = std::fs::create_dir_all(&path);
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> R<Option<String>> {
    Ok(app.dialog().file().blocking_pick_folder().map(|p| p.to_string()))
}

#[tauri::command]
async fn get_pending(s: State<'_, AppState>, token: String) -> R<AddRequest> {
    s.pending.lock().get(&token).cloned().ok_or_else(|| "expired".into())
}

#[tauri::command]
async fn resolve_pending(s: State<'_, AppState>, token: String, req: Option<AddRequest>) -> R<Option<DownloadInfo>> {
    s.pending.lock().remove(&token);
    match req {
        Some(r) => {
            let e = s.engine.clone();
            e.add(r).await.map(Some)
        }
        None => Ok(None),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    ffmpeg: bool,
    host_registered: bool,
    extension_id: &'static str,
    platform: &'static str,
}

#[tauri::command]
async fn app_info(s: State<'_, AppState>) -> R<AppInfo> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        ffmpeg: which_ffmpeg(),
        host_registered: s.host_registered,
        extension_id: native_host::EXTENSION_IDS[0],
        platform: std::env::consts::OS,
    })
}

fn which_ffmpeg() -> bool {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

#[tauri::command]
async fn check_update(app: AppHandle) -> R<Option<String>> {
    use tauri_plugin_updater::UpdaterExt;
    let up = app.updater().map_err(|e| e.to_string())?;
    Ok(up.check().await.map_err(|e| e.to_string())?.map(|u| u.version))
}

#[tauri::command]
async fn install_update(app: AppHandle) -> R<()> {
    use tauri_plugin_updater::UpdaterExt;
    let up = app.updater().map_err(|e| e.to_string())?;
    if let Some(u) = up.check().await.map_err(|e| e.to_string())? {
        app.state::<AppState>().engine.pause_all();
        u.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

/// Read clipboard text natively (the WebView clipboard API prompts for permission).
#[tauri::command]
async fn read_clipboard() -> R<String> {
    tokio::task::spawn_blocking(|| arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_clipboard(text: String) -> R<()> {
    tokio::task::spawn_blocking(move || arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn quit(app: AppHandle) -> R<()> {
    app.state::<AppState>().engine.pause_all();
    tokio::time::sleep(Duration::from_millis(300)).await;
    app.exit(0);
    Ok(())
}

// ───────────────────────────── setup ─────────────────────────────

fn fmt_speed(b: f64) -> String {
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.0} KB/s", b / 1024.0)
    } else {
        "idle".into()
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Velox", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume_all", "Resume all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &pause, &resume, &sep, &quit])?;
    let tray = TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or(tauri::Error::InvalidIcon(std::io::Error::other("no icon")))?)
        .tooltip("Velox")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let app = app.clone();
            let id = ev.id().as_ref().to_string();
            tauri::async_runtime::spawn(async move {
                let engine = app.state::<AppState>().engine.clone();
                match id.as_str() {
                    "show" => show_main(&app),
                    "pause_all" => engine.pause_all(),
                    "resume_all" => engine.resume_all(),
                    "quit" => {
                        engine.pause_all();
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        app.exit(0);
                    }
                    _ => {}
                }
            });
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    let engine = app.state::<AppState>().engine.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let s = engine.total_speed();
            let tip = if s > 1024.0 { format!("Velox — {}", fmt_speed(s)) } else { "Velox".into() };
            if tip != last {
                let _ = tray.set_tooltip(Some(&tip));
                last = tip;
            }
        }
    });
    Ok(())
}

fn forward_events(app: AppHandle) {
    let engine = app.state::<AppState>().engine.clone();
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let EngineEvent::Updated(d) = &ev {
                        let fresh = d.record.finished_at.map(|t| velox_now() - t < 1500).unwrap_or(false);
                        if d.record.status == Status::Completed && fresh {
                            notify(&app, "Download complete", &d.record.file_name);
                        } else if d.record.status == Status::Failed {
                            notify(&app, "Download failed", &d.record.file_name);
                        }
                    }
                    let _ = app.emit("engine", &ev);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

fn velox_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn main() {
    let background = std::env::args().any(|a| a == "--background");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            let engine = tauri::async_runtime::block_on(async { Engine::open(&data_dir) })
                .map_err(Box::<dyn std::error::Error>::from)?;
            let host_registered = native_host::register();
            app.manage(AppState { engine, pending: Mutex::new(HashMap::new()), host_registered });

            let handle = app.handle().clone();
            forward_events(handle.clone());
            setup_tray(&handle)?;
            clipboard::spawn(handle.clone());
            tauri::async_runtime::spawn(ipc::serve(handle.clone()));

            if !background {
                show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let state = window.app_handle().state::<AppState>();
                    if state.engine.settings().close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    } else {
                        state.engine.pause_all();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_downloads,
            add_download,
            probe_url,
            pause,
            resume,
            remove,
            pause_all,
            resume_all,
            clear_completed,
            set_download_limit,
            get_settings,
            set_settings,
            open_file,
            show_in_folder,
            open_folder,
            pick_folder,
            get_pending,
            resolve_pending,
            app_info,
            check_update,
            install_update,
            read_clipboard,
            write_clipboard,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Velox");
}
