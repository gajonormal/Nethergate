use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

pub struct State {
    path: PathBuf,
    keep_running: Mutex<bool>,
    error: Mutex<Option<String>>,
    pub detach_once: AtomicBool,
    pub closing: AtomicBool,
}

impl State {
    pub fn new(path: PathBuf) -> Self {
        let (keep_running,error)=match fs::read(&path) {
            Ok(bytes) if bytes.len()<=4096=>match serde_json::from_slice::<Value>(&bytes).ok().and_then(|v|v["keepRunning"].as_bool()) {
                Some(enabled)=>(enabled,None),None=>(false,Some("A preferência de fecho é inválida. Foi mantida a paragem segura ao fechar.".into()))
            },
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(false,None),
            _=>(false,Some("Não foi possível ler a preferência de fecho. Foi mantida a paragem segura.".into())),
        };
        Self {
            path,
            keep_running: Mutex::new(keep_running),
            error: Mutex::new(error),
            detach_once: AtomicBool::new(false),
            closing: AtomicBool::new(false),
        }
    }
    pub fn keep_running(&self) -> bool {
        *self
            .keep_running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn status(&self) -> Value {
        json!({"keepRunning":self.keep_running(),"error":*self.error.lock().unwrap_or_else(std::sync::PoisonError::into_inner),"closing":self.closing.load(Ordering::SeqCst)})
    }
    pub fn save(&self, enabled: bool) -> Result<Value, String> {
        if self.closing.load(Ordering::SeqCst) {
            return Err("Aguarda a paragem do motor".into());
        }
        let mut current = self.keep_running.lock().map_err(|error| {
            format!("Não foi possível obter o lock da preferência de fecho: {error}")
        })?;
        let result = (|| -> std::io::Result<()> {
            fs::create_dir_all(self.path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "O ficheiro da preferência de fecho não tem pasta pai",
                )
            })?)?;
            let temp = self.path.with_extension("json.tmp");
            let mut file = fs::File::create(&temp)?;
            file.write_all(serde_json::to_string(&json!({"keepRunning":enabled}))?.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(temp, &self.path)?;
            Ok(())
        })();
        result.map_err(|e| format!("Não foi possível guardar a preferência: {e}"))?;
        *current = enabled;
        drop(current);
        *self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ng-close-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn default_and_corrupt_settings_preserve_graceful_close() {
        let path = fixture_path("default");
        let state = State::new(path.clone());
        assert!(!state.keep_running());
        fs::write(&path, "invalid").unwrap();
        let state = State::new(path);
        assert!(!state.keep_running());
        assert!(state.status()["error"].is_string());
    }
    #[test]
    fn saved_policy_survives_reopen_and_failed_writes_do_not_change_behavior() {
        let path = fixture_path("persist");
        let state = State::new(path.clone());
        state.save(true).unwrap();
        assert!(State::new(path).keep_running());
        state.closing.store(true, Ordering::SeqCst);
        assert!(state.save(false).is_err());
        assert!(state.keep_running());
        let parent = fixture_path("blocked-parent");
        fs::write(&parent, "file").unwrap();
        let blocked = State::new(parent.join("background.json"));
        assert!(blocked.save(true).is_err());
        assert!(!blocked.keep_running());
    }
    #[test]
    fn poisoned_preference_is_readable_but_saving_returns_an_error() {
        let path = fixture_path("poisoned");
        let state = State::new(path.clone());
        state.save(true).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.keep_running.lock().unwrap();
            panic!("simulated preference failure");
        }));
        assert!(state.keep_running());
        assert_eq!(state.status()["keepRunning"], true);
        assert!(state.save(false).unwrap_err().contains("lock"));
        assert!(State::new(path).keep_running());
    }
}
