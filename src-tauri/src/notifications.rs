use nethergated::{
    daemon::Client as Session,
    notifications::{Monitor, Settings},
};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    thread,
    time::Duration,
};
use tauri_plugin_notification::NotificationExt;

pub struct State {
    path: PathBuf,
    settings: Mutex<(Settings, u64)>,
    error: Mutex<Option<String>>,
    submitted: AtomicU64,
}
impl State {
    pub fn new(path: PathBuf) -> Self {
        let (settings, error) = match fs::read(&path) {
            Ok(bytes) if bytes.len() <= 16384 => match serde_json::from_slice(&bytes) {
                Ok(settings) => (settings, None),
                Err(_) => (
                    Settings::default(),
                    Some(
                        "As preferências de notificações são inválidas. Escolhe novamente a opção."
                            .into(),
                    ),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (Settings::default(), None)
            }
            _ => (
                Settings::default(),
                Some("Não foi possível ler as preferências de notificações.".into()),
            ),
        };
        Self {
            path,
            settings: Mutex::new((settings, 0)),
            error: Mutex::new(error),
            submitted: AtomicU64::new(0),
        }
    }
    pub fn status(&self) -> Value {
        json!({"enabled":self.settings.lock().unwrap_or_else(std::sync::PoisonError::into_inner).0.enabled,"error":*self.error.lock().unwrap_or_else(std::sync::PoisonError::into_inner),"submitted":self.submitted.load(Ordering::Relaxed)})
    }
    pub fn save(&self, enabled: bool) -> Result<Value, String> {
        let mut settings = self
            .settings
            .lock()
            .map_err(|error| format!("Não foi possível obter o lock das notificações: {error}"))?;
        fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| "O ficheiro de notificações não tem pasta pai".to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        let value = Settings { enabled };
        let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&value).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(temp, &self.path).map_err(|e| e.to_string())?;
        *settings = (value, settings.1 + 1);
        drop(settings);
        *self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(self.status())
    }
}
pub fn run(app: tauri::AppHandle, session: Weak<Session>, state: Arc<State>) {
    thread::spawn(move || {
        let mut monitor = Monitor::default();
        let mut generation = 0;
        loop {
            thread::sleep(Duration::from_secs(3));
            let Some(session) = session.upgrade() else {
                break;
            };
            let (enabled, next) = {
                let settings = state
                    .settings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (settings.0.enabled, settings.1)
            };
            if generation != next || !enabled {
                monitor = Monitor::default();
                generation = next;
            }
            if !enabled {
                continue;
            }
            let Ok(snapshot) = session.notification_snapshot() else {
                continue;
            };
            for event in monitor.update(snapshot).into_iter().take(5) {
                if !state
                    .settings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0
                    .enabled
                {
                    break;
                }
                if app
                    .notification()
                    .builder()
                    .title(&event.title)
                    .body(&event.body)
                    .show()
                    .is_err()
                {
                    *state.error.lock().unwrap_or_else(std::sync::PoisonError::into_inner)=Some("O Windows não confirmou a notificação. Usa a aplicação instalada e verifica as notificações do sistema.".into());
                } else {
                    state.submitted.fetch_add(1, Ordering::Relaxed);
                    *state
                        .error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                }
            }
        }
    });
}
