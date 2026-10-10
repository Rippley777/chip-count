//! The desktop shell owns OS integration; all accounting lives in chip-core.
use chip_core::{LocalIndex, UserError};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(all(feature = "launch-agent", not(feature = "app-store")))]
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "macos")]
mod source_access;
use tauri_plugin_notification::NotificationExt;

#[derive(Deserialize)]
pub struct Request {
    command: String,
    #[serde(default = "empty_object")]
    args: Value,
    #[serde(default)]
    demo: bool,
}
fn empty_object() -> Value {
    json!({})
}

struct DesktopState {
    real: Arc<Mutex<LocalIndex>>,
    demo: Arc<Mutex<LocalIndex>>,
    paused: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    pricing: Arc<chip_pricing::PricingUpdater>,
    close_to_tray: Arc<AtomicBool>,
    notifications: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
    #[cfg(feature = "app-store")]
    access: Mutex<source_access::SourceAccess>,
}

#[tauri::command]
async fn dispatch(app: AppHandle, request: Request) -> Result<Value, UserError> {
    let state = app.state::<DesktopState>();
    let engine = if request.demo {
        state.demo.clone()
    } else {
        state.real.clone()
    };
    #[cfg(feature = "app-store")]
    if request.command == "settings_save"
        && request.args["settings"].get("launch_at_login").is_some()
    {
        return Err(UserError::native(
            "settings_save",
            "Launch at login is unavailable in the App Store 1.0 build.".into(),
        ));
    }
    let operation = request.command.clone();
    let worker_operation = operation.clone();
    let app_worker = app.clone();
    let pricing = state.pricing.clone();
    let should_wake = !request.demo
        && !matches!(
            request.command.as_str(),
            "snapshot" | "session" | "compare" | "export"
        );
    let result = tauri::async_runtime::spawn_blocking(move || {
        if request.command == "pricing_refresh" {
            if request.demo {
                return Err(UserError::new(
                    "pricing_unavailable",
                    "Live pricing refresh is unavailable in demo mode.",
                    "Exit demo mode to refresh API prices.",
                    "demo",
                ));
            }
            return pricing
                .refresh(&engine, true)
                .map_err(|e| UserError::from_error("pricing_refresh", &e));
        }
        let mut engine = engine.lock().map_err(|_| {
            UserError::new(
                "index_unavailable",
                "The index worker stopped.",
                "Restart Chip Count and retry.",
                "Index mutex was poisoned",
            )
        })?;
        #[cfg(feature = "app-store")]
        if !request.demo {
            let state = app_worker.state::<DesktopState>();
            let mut access = state.access.lock().map_err(|_| {
                UserError::native("source_save", "Source access is unavailable.".into())
            })?;
            if !request.command.starts_with("index_") {
                let engine = engine
                    .engine()
                    .map_err(|e| UserError::from_error(&worker_operation, &e))?;
                let _leases = access
                    .refresh(engine)
                    .map_err(|e| UserError::native("source_save", e))?;
                if request.command == "source_save" {
                    let mut args = request.args;
                    access
                        .prepare_save(engine, &mut args)
                        .map_err(|e| UserError::native("source_save", e))?;
                    return Ok(json!({"ok":true}));
                }
            }
        }
        #[cfg(not(feature = "app-store"))]
        let _ = app_worker;
        engine
            .dispatch(&request.command, request.args)
            .map_err(|e| UserError::from_error(&worker_operation, &e))
    })
    .await
    .map_err(|e| {
        UserError::new(
            "index_unavailable",
            "The index worker stopped.",
            "Retry the operation or restart Chip Count.",
            e.to_string(),
        )
    })??;
    if should_wake {
        let _ = state.wake.try_send(());
    }
    Ok(result)
}

#[tauri::command]
async fn desktop_settings(app: AppHandle, settings: DesktopSettings) -> Result<Value, UserError> {
    desktop_settings_inner(app, settings).await
}
#[tauri::command]
async fn compact(app: AppHandle) -> Result<Value, UserError> {
    compact_inner(app)
        .await
        .map_err(|e| UserError::native("compact", e))
}
#[tauri::command]
async fn reveal_path(app: AppHandle, path: String) -> Result<Value, UserError> {
    reveal_path_inner(app, path)
        .await
        .map_err(|e| UserError::native("reveal_path", e))
}
#[tauri::command]
async fn save_export(
    app: AppHandle,
    content: String,
    filename: String,
) -> Result<Value, UserError> {
    save_export_inner(app, content, filename)
        .await
        .map_err(|e| UserError::native("save_export", e))
}
#[tauri::command]
async fn select_source(app: AppHandle, directory: bool) -> Result<Value, UserError> {
    select_source_inner(app, directory)
        .await
        .map_err(|e| UserError::native("select_source", e))
}
#[tauri::command]
async fn restore_index(app: AppHandle) -> Result<Value, UserError> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let selected = on_main(&app, source_access::restore_panel)
            .map_err(|e| UserError::native("restore_index", e))?;
        #[cfg(target_os = "macos")]
        let Some(selected) = selected
        else {
            return Ok(json!({"restored":false}));
        };
        #[cfg(target_os = "macos")]
        let path = selected.path.clone();
        #[cfg(not(target_os = "macos"))]
        let path = {
            let Some(selected) = app
                .dialog()
                .file()
                .add_filter("Chip Count backup", &["sqlite"])
                .blocking_pick_file()
            else {
                return Ok(json!({"restored":false}));
            };
            selected
                .into_path()
                .map_err(|e| UserError::native("restore_index", e.to_string()))?
        };
        let state = app.state::<DesktopState>();
        let mut index = state.real.lock().map_err(|_| {
            UserError::native(
                "restore_index",
                "The index worker stopped; restart Chip Count.".into(),
            )
        })?;
        let backup = index
            .restore(&path)
            .map_err(|e| UserError::new("restore_failed", "The selected backup could not be restored.", "Select a compatible Chip Count SQLite backup, check index write access, and retry. The original index and recovery copies are retained.", format!("{e:#}")))?;
        let _ = state.wake.try_send(());
        Ok(json!({"restored":true,"backup":backup}))
    })
    .await
    .map_err(|e| UserError::native("restore_index", e.to_string()))?
}

fn emit_status(app: &AppHandle, paused: bool) {
    let _ = app.emit(
        "index-updated",
        json!({"at":chrono::Utc::now().to_rfc3339(),"paused":paused}),
    );
}

#[tauri::command]
fn monitoring(app: AppHandle, paused: Option<bool>) -> Value {
    let state = app.state::<DesktopState>();
    if let Some(paused) = paused {
        state.paused.store(paused, Ordering::Relaxed);
        let _ = state.wake.try_send(());
        emit_status(&app, paused);
    }
    json!({"paused":state.paused.load(Ordering::Relaxed)})
}

#[derive(Deserialize)]
struct DesktopSettings {
    launch_at_login: Option<bool>,
    close_to_tray: Option<bool>,
    notifications: Option<bool>,
}

async fn desktop_settings_inner(
    app: AppHandle,
    settings: DesktopSettings,
) -> Result<Value, UserError> {
    #[cfg(any(feature = "app-store", not(feature = "launch-agent")))]
    if settings.launch_at_login.is_some() {
        return Err(UserError::new(
            "invalid_input",
            "Launch at login is unavailable in this build.",
            "Use the other desktop preferences in Settings.",
            "launch_at_login",
        ));
    }
    let state = app.state::<DesktopState>();
    let mut saved = serde_json::Map::new();
    if let Some(v) = settings.launch_at_login {
        saved.insert("launch_at_login".into(), json!(v));
    }
    if let Some(v) = settings.close_to_tray {
        saved.insert("close_to_tray".into(), json!(v));
    }
    if let Some(v) = settings.notifications {
        saved.insert("notifications".into(), json!(v));
    }
    #[cfg(all(feature = "launch-agent", not(feature = "app-store")))]
    let previous_login = if let Some(enabled) = settings.launch_at_login {
        let manager = app.autolaunch();
        let previous = manager
            .is_enabled()
            .map_err(|e| UserError::native("desktop_settings", e.to_string()))?;
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|e| {
            UserError::native(
                "desktop_settings",
                format!("Could not change launch at login: {e}"),
            )
        })?;
        Some(previous)
    } else {
        None
    };
    let engine = state.real.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine
            .lock()
            .map_err(|_| {
                UserError::new(
                    "index_unavailable",
                    "The index worker stopped.",
                    "Restart Chip Count and retry.",
                    "Index mutex was poisoned",
                )
            })?
            .dispatch("settings_save", json!({"settings":saved}))
            .map_err(|e| UserError::from_error("settings_save", &e))
    })
    .await
    .map_err(|e| UserError::native("desktop_settings", e.to_string()))
    .and_then(|v| v);
    if let Err(mut error) = result {
        #[cfg(all(feature = "launch-agent", not(feature = "app-store")))]
        if let Some(previous) = previous_login {
            let manager = app.autolaunch();
            let rollback = if previous {
                manager.enable()
            } else {
                manager.disable()
            };
            if let Err(rollback) = rollback {
                error.next_steps.push_str(" Check Chip Count in macOS Login Items; its previous system preference could not be restored.");
                error
                    .diagnostics
                    .push_str(&format!("\nLogin Items restore: {rollback}"));
            }
        }
        #[cfg(any(feature = "app-store", not(feature = "launch-agent")))]
        let _ = &mut error;
        return Err(error);
    }
    if let Some(v) = settings.close_to_tray {
        state.close_to_tray.store(v, Ordering::Relaxed);
    }
    if let Some(v) = settings.notifications {
        state.notifications.store(v, Ordering::Relaxed);
    }
    Ok(json!({"ok":true}))
}

fn open_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

async fn compact_inner(app: AppHandle) -> Result<Value, String> {
    if let Some(window) = app.get_webview_window("compact") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    } else {
        WebviewWindowBuilder::new(
            &app,
            "compact",
            WebviewUrl::App("index.html?compact=1".into()),
        )
        .title("Chip Count · Monitor")
        .inner_size(430.0, 550.0)
        .min_inner_size(360.0, 320.0)
        .always_on_top(true)
        .resizable(true)
        .build()
        .map_err(|e| e.to_string())?;
    }
    Ok(json!({"ok":true}))
}

/// Canonical comparisons prevent `..` and symlinks from escaping an indexed root.
fn allowed_reveal(path: &Path, snapshot: &Value) -> Result<PathBuf, String> {
    let target = path
        .canonicalize()
        .map_err(|_| "This path no longer exists or cannot be accessed.".to_string())?;
    let roots = ["sources", "projects"]
        .into_iter()
        .flat_map(|key| snapshot[key].as_array().into_iter().flatten())
        .filter_map(|row| row["path"].as_str())
        .filter_map(|p| Path::new(p).canonicalize().ok());
    for root in roots {
        if target == root || (root.is_dir() && target.starts_with(&root)) {
            return Ok(target);
        }
    }
    Err("Only indexed source files and known project folders can be revealed.".into())
}

async fn reveal_path_inner(app: AppHandle, path: String) -> Result<Value, String> {
    let engine = app.state::<DesktopState>().real.clone();
    #[cfg(feature = "app-store")]
    let app_worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut index = engine
            .lock()
            .map_err(|_| "The local index is unavailable.".to_string())?;
        let engine = index.engine().map_err(|e| e.to_string())?;
        #[cfg(feature = "app-store")]
        let _leases = app_worker
            .state::<DesktopState>()
            .access
            .lock()
            .map_err(|_| "Source access is unavailable.")?
            .refresh(engine)?;
        let snapshot = engine
            .dispatch("snapshot", json!({}))
            .map_err(|e| e.to_string())?;
        let target = allowed_reveal(Path::new(&path), &snapshot)?;
        #[cfg(feature = "app-store")]
        {
            let state = app_worker.state::<DesktopState>();
            let access = state
                .access
                .lock()
                .map_err(|_| "Source access is unavailable.")?;
            if !access.allows(&target) {
                return Err("Choose this root in Sources before revealing it in Finder.".into());
            }
        }
        #[cfg(target_os = "macos")]
        on_main(&app, move || source_access::reveal(&target))?;
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(target_os = "windows")]
            let status = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", target.display()))
                .status();
            #[cfg(target_os = "linux")]
            let status = std::process::Command::new("xdg-open")
                .arg(if target.is_dir() {
                    target.as_path()
                } else {
                    target.parent().unwrap_or(&target)
                })
                .status();
            if !status
                .map_err(|e| format!("Could not open the file manager: {e}"))?
                .success()
            {
                return Err("The system file manager could not reveal this path.".into());
            }
        }
        Ok(json!({"ok":true}))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn save_export_inner(
    app: AppHandle,
    content: String,
    filename: String,
) -> Result<Value, String> {
    if content.len() > 64 * 1024 * 1024 {
        return Err(
            "This export exceeds the 64 MB desktop limit. Select a smaller date range.".into(),
        );
    }
    let filename = Path::new(&filename)
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty() && !v.chars().any(char::is_control))
        .unwrap_or("chip-count.json")
        .to_string();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let selected = on_main(&app, move || source_access::save_panel(&filename))?;
        #[cfg(target_os = "macos")]
        let Some(selected) = selected
        else {
            return Ok(json!({"saved":false}));
        };
        #[cfg(target_os = "macos")]
        let path = selected.path.clone();
        #[cfg(not(target_os = "macos"))]
        let path = {
            let extension = if filename.ends_with(".csv") {
                "csv"
            } else {
                "json"
            };
            let selected = app
                .dialog()
                .file()
                .set_file_name(&filename)
                .add_filter("Usage export", &[extension])
                .blocking_save_file();
            let Some(selected) = selected else {
                return Ok(json!({"saved":false}));
            };
            selected
                .into_path()
                .map_err(|_| "Select a local export file.")?
        };
        let engine = app.state::<DesktopState>().real.clone();
        let mut index = engine.lock().map_err(|_| "The local index is unavailable.")?;
        if let Some(parent) = index.path().parent() {
            if let (Ok(directory), Ok(destination)) = (parent.canonicalize(), path.parent().unwrap_or(Path::new("/")).canonicalize()) {
                if destination.starts_with(directory) {
                    return Err("Choose a destination outside Chip Count's index directory to preserve your database and recovery copies.".into());
                }
            }
        }
        let engine = index.engine().map_err(|e| e.to_string())?;
        #[cfg(feature = "app-store")]
        let _leases = app
            .state::<DesktopState>()
            .access
            .lock()
            .map_err(|_| "Source access is unavailable.")?
            .refresh(engine)?;
        #[cfg(not(feature = "app-store"))]
        let _ = &engine;
        ensure_export_destination(&path, &engine.source_records().map_err(|e| e.to_string())?)?;
        // The only writable destination comes directly from the native dialog.
        atomic_export(&path, content.as_bytes())?;
        Ok(json!({"saved":true,"path":path.to_string_lossy()}))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn atomic_export(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.with_file_name(format!(
        ".chip-count-export-{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Could not save export; the previous destination was kept: {e}"))
}

/// Native UI work is scheduled from a blocking worker, never blocks Tauri's UI thread.
#[cfg(target_os = "macos")]
fn on_main<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(work());
    })
    .map_err(|e| e.to_string())?;
    rx.recv()
        .map_err(|_| "The native dialog closed unexpectedly.".to_string())?
}

async fn select_source_inner(app: AppHandle, directory: bool) -> Result<Value, String> {
    #[cfg(feature = "app-store")]
    return tauri::async_runtime::spawn_blocking(move || {
        let value = on_main(&app, move || source_access::choose(directory))?;
        let state = app.state::<DesktopState>();
        let result = state
            .access
            .lock()
            .map_err(|_| "Source access is unavailable.")?
            .selected(value);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(not(feature = "app-store"))]
    {
        let _ = (app, directory);
        Err("Use the native dialog in this build.".into())
    }
}

/// Export must never overwrite a transcript or write within any configured source.
fn ensure_export_destination(path: &Path, sources: &[Value]) -> Result<(), String> {
    let target = if path.exists() {
        path.canonicalize()
    } else {
        path.parent()
            .unwrap_or(Path::new("/"))
            .canonicalize()
            .map(|p| p.join(path.file_name().unwrap_or_default()))
    }
    .map_err(|e| format!("The export destination is unavailable: {e}"))?;
    for source in sources {
        if let Some(root) = source["path"].as_str() {
            let root = Path::new(root)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(root));
            if target == root || target.starts_with(&root) {
                return Err(
                    "Save the export outside your source folders to keep original logs unchanged."
                        .into(),
                );
            }
        }
    }
    if target.extension().is_some_and(|e| e == "jsonl") {
        return Err("Save exports as JSON or CSV; JSONL source logs are read-only.".into());
    }
    Ok(())
}

fn watch_changed_roots(
    watcher: &mut Option<RecommendedWatcher>,
    current: &mut BTreeSet<PathBuf>,
    roots: Vec<PathBuf>,
) {
    let next: BTreeSet<_> = roots
        .into_iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect();
    if let Some(watcher) = watcher {
        for old in current.difference(&next) {
            let _ = watcher.unwatch(old);
        }
        let mut watched: BTreeSet<_> = current.intersection(&next).cloned().collect();
        for new in next.difference(current) {
            if watcher.watch(new, RecursiveMode::Recursive).is_ok() {
                watched.insert(new.clone());
            }
        }
        *current = watched;
    } else {
        current.clear();
    }
}

fn notification_history(path: &Path) -> BTreeSet<String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
fn persist_notification_history(path: &Path, ids: &BTreeSet<String>) {
    if let Ok(bytes) = serde_json::to_vec(ids) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

fn start_monitor(
    app: AppHandle,
    receiver: mpsc::Receiver<()>,
    history_path: PathBuf,
    summary: MenuItem<tauri::Wry>,
    pause_item: MenuItem<tauri::Wry>,
) {
    std::thread::Builder::new()
        .name("chip-count-indexer".into())
        .spawn(move || {
            let state = app.state::<DesktopState>();
            let wake = state.wake.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                    if event.is_ok_and(|event| {
                        event.paths.iter().any(|path| {
                            path.extension().is_some_and(|ext| ext == "jsonl") || path.is_dir()
                        })
                    }) {
                        let _ = wake.try_send(());
                    }
                })
                .ok();
            let mut roots = BTreeSet::new();
            #[cfg(feature = "app-store")]
            let mut _watch_leases = Vec::new();
            let mut notified = notification_history(&history_path);
            let mut first_pass = true;
            while !state.stopping.load(Ordering::Relaxed) {
                let paused = state.paused.load(Ordering::Relaxed);
                let _ = pause_item.set_text(if paused {
                    "Resume monitoring"
                } else {
                    "Pause monitoring"
                });
                if !paused {
                    let result = (|| {
                        let mut index = state
                            .real
                            .lock()
                            .map_err(|_| "The local index is unavailable.".to_string())?;
                        let engine = index.engine().map_err(|e| e.to_string())?;
                        #[cfg(feature = "app-store")]
                        let leases = state
                            .access
                            .lock()
                            .map_err(|_| "Source access is unavailable.")?
                            .refresh(engine)?;
                        engine.reconcile().map_err(|e| e.to_string())?;
                        watch_changed_roots(&mut watcher, &mut roots, engine.watch_roots());
                        // Unwatch first, then release previous grants. Watchers retain their scopes
                        // across polling, sleep/wake, and paused monitoring.
                        #[cfg(feature = "app-store")]
                        {
                            _watch_leases = leases;
                        }
                        engine
                            .dispatch("snapshot", json!({}))
                            .map_err(|e| e.to_string())
                    })();
                    match result {
                        Ok(snapshot) => {
                            let total = snapshot["today"]["total"].as_u64().unwrap_or(0);
                            let cost = snapshot["today"]["cost"].as_f64().unwrap_or(0.0);
                            let active = snapshot["active_sessions"].as_u64().unwrap_or(0);
                            let unpriced =
                                snapshot["today"]["unpriced_tokens"].as_u64().unwrap_or(0);
                            let estimate = if unpriced == 0 {
                                format!("${cost:.2} API estimate")
                            } else if cost == 0.0 {
                                format!("{unpriced} unpriced tokens")
                            } else {
                                format!("${cost:.2} known + {unpriced} unpriced tokens")
                            };
                            let _ = summary.set_text(format!(
                                "{active} active · {total} tokens today · {estimate}"
                            ));
                            if let Some(tray) = app.tray_by_id("chip-count") {
                                let _ = tray.set_tooltip(Some(format!(
                                    "Chip Count · {active} active · Today: {estimate}"
                                )));
                            }
                            if let Some(v) = snapshot["settings"]["close_to_tray"].as_bool() {
                                state.close_to_tray.store(v, Ordering::Relaxed);
                            }
                            if let Some(v) = snapshot["settings"]["notifications"].as_bool() {
                                state.notifications.store(v, Ordering::Relaxed);
                            }
                            let mut changed = false;
                            for alert in snapshot["alerts"].as_array().into_iter().flatten() {
                                if let Some(id) = alert["id"].as_str() {
                                    if notified.insert(id.to_string()) {
                                        changed = true;
                                        if !first_pass
                                            && state.notifications.load(Ordering::Relaxed)
                                        {
                                            let message = alert["message"].as_str().unwrap_or(
                                                "A local usage budget crossed its threshold.",
                                            );
                                            let _ = app
                                                .notification()
                                                .builder()
                                                .title("Chip Count · Budget alert")
                                                .body(message)
                                                .show();
                                        }
                                    }
                                }
                            }
                            if changed {
                                persist_notification_history(&history_path, &notified);
                            }
                            first_pass = false;
                            emit_status(&app, state.paused.load(Ordering::Relaxed));
                        }
                        Err(error) => {
                            let _ = app.emit("index-error", UserError::new("index_unavailable", "Background indexing could not continue.", "Retry the scan in Sources, check source diagnostics and disk access, or restart Chip Count to open index recovery.", error));
                        }
                    }
                }
                // Source removal/disable/revocation must also retire watchers while paused.
                #[cfg(feature = "app-store")]
                if paused {
                    if let Ok(mut index) = state.real.lock() {
                        if let Ok(engine) = index.engine() {
                            if let Ok(mut access) = state.access.lock() {
                                if let Ok(leases) = access.refresh(engine) {
                                    watch_changed_roots(
                                        &mut watcher,
                                        &mut roots,
                                        engine.watch_roots(),
                                    );
                                    _watch_leases = leases;
                                }
                            }
                        }
                    }
                }
                // Checkpoints make this cheap; polling also recovers missed notifications/sleep.
                if receiver.recv_timeout(Duration::from_secs(3)).is_ok() {
                    std::thread::sleep(Duration::from_millis(150));
                }
                // Coalesce a burst of filesystem changes into one pass.
                while receiver.try_recv().is_ok() {}
            }
            // Drop native watchers before the final scopes on normal app exit.
            drop(watcher);
            #[cfg(feature = "app-store")]
            {
                _watch_leases.clear();
                if let Ok(mut access) = state.access.lock() {
                    access.clear();
                };
            }
        })
        .expect("could not start index worker");
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init());
    #[cfg(all(feature = "launch-agent", not(feature = "app-store")))]
    let builder = builder.plugin(
        tauri_plugin_autostart::Builder::new()
            .macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent)
            .build(),
    );
    builder
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .setup(|app| {
            #[cfg(feature = "app-store")]
            let directory = app.path().app_data_dir()?;
            #[cfg(not(feature = "app-store"))]
            let directory = std::env::var_os("CHIP_COUNT_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            let real = Arc::new(Mutex::new(LocalIndex::open(
                &directory.join("chip-count.sqlite"),
                cfg!(feature = "app-store"),
            )));
            #[cfg(feature = "app-store")]
            let access = source_access::SourceAccess::default();
            let demo = Arc::new(Mutex::new(LocalIndex::demo()?));
            let (wake, receiver) = mpsc::sync_channel(1);
            let snapshot_result = real.lock().unwrap().dispatch("snapshot", json!({}));
            if let Err(error) = &snapshot_result {
                let error = UserError::from_error("open_index", error);
                app.dialog()
                    .message(format!(
                        "{}\n\n{}\n\n{}",
                        error.message, error.next_steps, error.diagnostics
                    ))
                    .title("Chip Count · Index recovery")
                    .show(|_| {});
            }
            let snapshot = snapshot_result.unwrap_or(json!({}));
            app.manage(DesktopState {
                real,
                demo,
                #[cfg(feature = "app-store")]
                access: Mutex::new(access),
                wake,
                paused: Arc::new(AtomicBool::new(false)),
                stopping: Arc::new(AtomicBool::new(false)),
                pricing: Arc::new(chip_pricing::PricingUpdater::default()),
                close_to_tray: Arc::new(AtomicBool::new(
                    snapshot["settings"]["close_to_tray"]
                        .as_bool()
                        .unwrap_or(false),
                )),
                notifications: Arc::new(AtomicBool::new(
                    snapshot["settings"]["notifications"]
                        .as_bool()
                        .unwrap_or(false),
                )),
            });
            let state = app.state::<DesktopState>();
            let pricing_app = app.handle().clone();
            state
                .pricing
                .clone()
                .start(state.real.clone(), state.stopping.clone(), move || {
                    let _ = pricing_app.state::<DesktopState>().wake.try_send(());
                    emit_status(
                        &pricing_app,
                        pricing_app
                            .state::<DesktopState>()
                            .paused
                            .load(Ordering::Relaxed),
                    );
                })?;
            // Extend Tauri's default menu, retaining native Edit, Window and Services behavior.
            let app_menu = Menu::default(app.handle())?;
            let settings =
                MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            #[cfg(target_os = "macos")]
            if let Some(tauri::menu::MenuItemKind::Submenu(menu)) = app_menu.items()?.first() {
                menu.insert(&settings, 1)?;
            }
            #[cfg(not(target_os = "macos"))]
            app_menu.append(&settings)?;
            for item in app_menu.items()? {
                if let tauri::menu::MenuItemKind::Submenu(menu) = item {
                    if menu.text()? == "Help" {
                        menu.append(&MenuItem::with_id(
                            app,
                            "help",
                            "Chip Count Help",
                            true,
                            None::<&str>,
                        )?)?;
                        menu.append(&MenuItem::with_id(
                            app,
                            "diagnostics",
                            "Source diagnostics",
                            true,
                            None::<&str>,
                        )?)?;
                    }
                }
            }
            app.set_menu(app_menu)?;
            app.on_menu_event(|app, event| {
                let action = match event.id.as_ref() {
                    "settings" => "settings",
                    "help" => "help",
                    "diagnostics" => "sources",
                    _ => return,
                };
                open_main(app);
                let _ = app.emit_to("main", "navigate", action);
            });
            let summary =
                MenuItem::with_id(app, "summary", "Indexing local usage…", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Chip Count", true, None::<&str>)?;
            let compact =
                MenuItem::with_id(app, "compact", "Open compact monitor", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "Pause monitoring", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Chip Count", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&summary, &open, &compact, &pause, &quit])?;
            TrayIconBuilder::with_id("chip-count")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => open_main(app),
                    "compact" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = crate::compact(app).await;
                        });
                    }
                    "pause" => {
                        let paused = !app.state::<DesktopState>().paused.load(Ordering::Relaxed);
                        monitoring(app.clone(), Some(paused));
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        open_main(tray.app_handle());
                    }
                })
                .build(app)?;
            start_monitor(
                app.handle().clone(),
                receiver,
                directory.join("notified-alerts.json"),
                summary,
                pause,
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && window
                        .state::<DesktopState>()
                        .close_to_tray
                        .load(Ordering::Relaxed)
                {
                    api.prevent_close();
                    let _ = window.hide();
                } else if window.label() == "main" {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dispatch,
            monitoring,
            desktop_settings,
            compact,
            reveal_path,
            save_export,
            select_source,
            restore_index
        ])
        .build(tauri::generate_context!())
        .expect("Chip Count could not start")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<DesktopState>();
                state.stopping.store(true, Ordering::Relaxed);
                let _ = state.wake.try_send(());
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                open_main(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_export_retains_destination_and_removes_temporary_file() {
        let root = std::env::temp_dir().join(format!("chip-atomic-export-{}", std::process::id()));
        std::fs::create_dir_all(root.join("destination")).unwrap();
        std::fs::write(root.join("destination/original.json"), b"keep me").unwrap();
        assert!(atomic_export(&root.join("destination"), b"new content").is_err());
        assert_eq!(
            std::fs::read(root.join("destination/original.json")).unwrap(),
            b"keep me"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::write(root.join("export.json"), b"old").unwrap();
        atomic_export(&root.join("export.json"), b"complete new report").unwrap();
        assert_eq!(
            std::fs::read(root.join("export.json")).unwrap(),
            b"complete new report"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn export_cannot_overwrite_sources_or_follow_symlinks_into_them() {
        let root = std::env::temp_dir().join(format!("chip-export-{}", std::process::id()));
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/session.jsonl"), "source").unwrap();
        let sources = vec![json!({"path":root.join("logs")})];
        assert!(ensure_export_destination(&root.join("export.json"), &sources).is_ok());
        assert!(ensure_export_destination(&root.join("logs/export.json"), &sources).is_err());
        assert!(ensure_export_destination(&root.join("logs/session.jsonl"), &sources).is_err());
        assert!(ensure_export_destination(&root.join("other.jsonl"), &sources).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("logs"), root.join("linked")).unwrap();
            assert!(ensure_export_destination(&root.join("linked/export.csv"), &sources).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(root.join("logs/session.jsonl")).unwrap(),
            "source"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reveal_rejects_paths_outside_known_roots() {
        let root = std::env::temp_dir().join(format!("chip-reveal-{}", std::process::id()));
        std::fs::create_dir_all(root.join("source")).unwrap();
        std::fs::write(root.join("source/log.jsonl"), "").unwrap();
        std::fs::write(root.join("private.txt"), "").unwrap();
        let snapshot = json!({"sources":[{"path":root.join("source")}],"projects":[]});
        assert!(allowed_reveal(&root.join("source/log.jsonl"), &snapshot).is_ok());
        assert!(allowed_reveal(&root.join("source/../private.txt"), &snapshot).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("private.txt"), root.join("source/link")).unwrap();
            assert!(allowed_reveal(&root.join("source/link"), &snapshot).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
