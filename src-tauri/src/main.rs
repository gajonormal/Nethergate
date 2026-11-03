#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use nethergated::{daemon::Client as Session, library::Preview};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
mod background;
mod notifications;
struct LibraryWarning(std::sync::Mutex<Option<String>>);
#[tauri::command]
async fn choose_jar_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Escolher JAR local")
            .add_filter("Java JAR", &["jar"])
            .blocking_pick_file()
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn choose_data_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Escolher pasta existente")
            .blocking_pick_folder()
            .map(|p| {
                p.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_eula() -> Result<(), String> {
    open_external("https://aka.ms/MinecraftEULA")
}

#[tauri::command]
fn open_duckdns() -> Result<(), String> {
    open_external("https://www.duckdns.org/")
}
#[tauri::command]
fn open_playit(url: String) -> Result<(), String> {
    let claim = url.strip_prefix("https://playit.gg/claim/");
    if url != "https://playit.gg/account/tunnels"
        && !claim.is_some_and(|c| c.len() == 10 && c.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Ligação playit inválida".into());
    }
    open_external(&url)
}

#[tauri::command]
fn open_cloud_page(url: String) -> Result<(), String> {
    if !nethergated::cloud::accounts::allowed_browser_url(&url) {
        return Err("Ligação cloud inválida".into());
    }
    open_external(&url)
}

fn open_external(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    nethergated::process::hide_window(&mut command);
    command.arg(url).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn hub_call(
    state: tauri::State<'_, Arc<Session>>,
    action: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let session = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        session
            .dispatch(&action, payload)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn library_info(
    state: tauri::State<'_, Arc<Session>>,
    warning: tauri::State<'_, LibraryWarning>,
) -> Result<serde_json::Value, String> {
    let session = state.inner().clone();
    let warning = warning
        .0
        .lock()
        .map_err(|error| format!("Não foi possível obter o lock do aviso da biblioteca: {error}"))?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut value = session.info().map_err(|e| format!("{e:#}"))?;
        if warning.is_some() {
            value["warning"] = serde_json::json!(warning);
        }
        Ok(value)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn pick_library(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Session>>,
) -> Result<Option<Preview>, String> {
    let session = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("Escolher biblioteca Nethergate existente")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|e| e.to_string())?;
        session
            .preview(&path)
            .map(Some)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn open_library(
    state: tauri::State<'_, Arc<Session>>,
    path: String,
    fingerprint: String,
    warning: tauri::State<'_, LibraryWarning>,
) -> Result<serde_json::Value, String> {
    let session = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        session
            .open(std::path::Path::new(&path), &fingerprint)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?;
    if result.is_ok() {
        *warning.0.lock().map_err(|error| {
            format!("Não foi possível obter o lock do aviso da biblioteca: {error}")
        })? = None;
    }
    result
}

#[tauri::command]
async fn background_status(
    session: tauri::State<'_, Arc<Session>>,
    state: tauri::State<'_, Arc<background::State>>,
) -> Result<serde_json::Value, String> {
    let client = session.inner().clone();
    let mut status = state.status();
    let (info, servers) = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let info = client.info().map_err(|e| format!("{e:#}"))?;
        let servers = client
            .dispatch("list", serde_json::json!({}))
            .map_err(|e| format!("{e:#}"))?;
        Ok((info, servers))
    })
    .await
    .map_err(|e| e.to_string())??;
    status["busy"] = info["busy"].clone();
    status["independent"] = info["independent"].clone();
    status["servers"] = servers;
    Ok(status)
}
#[tauri::command]
fn background_settings(
    state: tauri::State<'_, Arc<background::State>>,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    state.save(enabled)
}
#[tauri::command]
async fn detach_panel(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<background::State>>,
) -> Result<(), String> {
    if state.closing.load(Ordering::SeqCst) {
        return Err("O motor já está a parar".into());
    }
    state.detach_once.store(true, Ordering::SeqCst);
    if let Err(error) = window.close() {
        state.detach_once.store(false, Ordering::SeqCst);
        return Err(error.to_string());
    }
    Ok(())
}
#[tauri::command]
async fn shutdown_engine(
    app: tauri::AppHandle,
    session: tauri::State<'_, Arc<Session>>,
    state: tauri::State<'_, Arc<background::State>>,
) -> Result<(), String> {
    if state.closing.swap(true, Ordering::SeqCst) {
        return Err("O motor já está a parar".into());
    }
    let client = session.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        client.shutdown().map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|value| value);
    if let Err(error) = result {
        state.closing.store(false, Ordering::SeqCst);
        return Err(error);
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn notification_settings(
    state: tauri::State<'_, Arc<notifications::State>>,
    enabled: Option<bool>,
) -> Result<serde_json::Value, String> {
    if let Some(enabled) = enabled {
        state.save(enabled)
    } else {
        Ok(state.status())
    }
}
fn main() {
    let engine_args: Vec<_> = std::env::args_os().skip(1).collect();
    if engine_args.first().is_some_and(|v| v == "--engine") {
        if engine_args.len() != 5
            || engine_args[1] != "--engine-root"
            || engine_args[3] != "--engine-preferences"
        {
            std::process::exit(2);
        }
        let _guard = match nethergated::daemon::process_guard() {
            Ok(guard) => guard,
            Err(_) => std::process::exit(1),
        };
        let result = nethergated::daemon::serve(
            engine_args[2].clone().into(),
            engine_args[4].clone().into(),
        );
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if let Err(error) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let exe = std::env::current_exe()?;
            let mut preferences=app.path().app_config_dir()?.join("library.json");
            let (saved,warning)=match nethergated::library::selected(&preferences) {
                Ok(path)=>(path,None),
                Err(_)=>(None,Some("A biblioteca guardada não está disponível. Foi aberta a biblioteca inicial; podes escolher novamente a pasta.".to_string()))
            };
            let mut arguments = std::env::args_os().skip(1);
            let requested_data = if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--data-dir")) {
                Some(std::path::PathBuf::from(arguments.next().ok_or_else(||std::io::Error::other("Falta a pasta de dados depois de --data-dir"))?))
            } else { None };
            let root = if let Some(path) = requested_data {
                if !path.is_absolute() || !path.join("hub-data/registry.json").is_file() {
                    return Err(std::io::Error::other("A biblioteca existente deve indicar uma pasta absoluta com hub-data/registry.json").into());
                }
                preferences=path.join("hub-data/desktop-preferences/library.json");path
            } else if let Some(path)=saved { path
            } else if let Some(home) = std::env::var_os("NETHERGATE_HOME") {
                std::path::PathBuf::from(home)
            } else if let Some(project) = nethergated::storage::development_root(&exe) {
                project
            } else {
                // Worlds must survive uninstall, including the optional removal of app caches.
                let local = app.path().app_local_data_dir()?;
                local.parent().ok_or_else(||std::io::Error::other("Pasta de dados do Windows indisponível"))?.join("NetherGateData")
            };
            std::fs::create_dir_all(&root)?;
            let notification_path=preferences.with_file_name("notifications.json");
            let background_state=Arc::new(background::State::new(preferences.with_file_name("background.json")));
            let session=Arc::new(Session::connect_or_spawn(&exe,&root,&preferences)?);
            let notification_state=Arc::new(notifications::State::new(notification_path));
            notifications::run(app.handle().clone(),Arc::downgrade(&session),notification_state.clone());
            app.manage(session);
            app.manage(notification_state);
            app.manage(background_state);
            app.manage(LibraryWarning(std::sync::Mutex::new(warning)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![hub_call, open_cloud_page, choose_data_folder, choose_jar_file, open_eula, open_duckdns, open_playit,library_info,pick_library,open_library,notification_settings,background_status,background_settings,detach_panel,shutdown_engine])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state=window.state::<Arc<background::State>>();
                if !state.closing.load(Ordering::SeqCst) && state.detach_once.swap(false,Ordering::SeqCst) {return;}
                api.prevent_close();
                if !state.closing.load(Ordering::SeqCst) {
                    let _ = window.emit("engine-close-requested", ());
                }
            }
        })
        .run(tauri::generate_context!())
    {
        eprintln!("Não foi possível executar Nethergate: {error}");
        std::process::exit(1);
    }
}
