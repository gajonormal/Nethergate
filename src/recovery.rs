use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
}
#[derive(Default)]
pub struct Recovery {
    attempts: u8,
    ready: Option<Instant>,
    due: Option<Instant>,
}
impl Recovery {
    pub fn start(&mut self, automatic: bool) {
        self.ready = None;
        self.due = None;
        if !automatic {
            self.attempts = 0;
        }
    }
    pub fn ready(&mut self) {
        self.ready = Some(Instant::now());
    }
    pub fn failure(&mut self, unexpected: bool, enabled: bool, now: Instant) -> Option<Duration> {
        let ready = self.ready.take()?;
        self.due = None;
        if !unexpected || !enabled {
            return None;
        }
        if now.saturating_duration_since(ready) >= Duration::from_secs(600) {
            self.attempts = 0;
        }
        if self.attempts >= 3 {
            return None;
        }
        self.attempts += 1;
        let delay = Duration::from_secs(30u64 << (self.attempts - 1));
        self.due = Some(now + delay);
        Some(delay)
    }
    pub fn is_due(&self) -> bool {
        self.due.is_some_and(|due| Instant::now() >= due)
    }
    pub fn cancel(&mut self) {
        self.due = None;
        self.ready = None;
    }
    pub fn view(&self) -> Value {
        json!({"attempt":self.attempts,"maximumAttempts":3,"remainingSeconds":self.due.map(|due|due.saturating_duration_since(Instant::now()).as_secs()),"pending":self.due.is_some()})
    }
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        bail!("Identificador inválido");
    }
    Ok(root.join("hub-data/recovery").join(format!("{id}.json")))
}
pub fn load(root: &Path, id: &str) -> Result<Policy> {
    let path = path(root, id)?;
    if !path.exists() {
        return Ok(Policy::default());
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 16384 {
        bail!("Política de recuperação inválida");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn save(root: &Path, id: &str, enabled: bool) -> Result<()> {
    let path = path(root, id)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração de recuperação não tem pasta pai")?,
    )?;
    crate::hub::replace_json(&path, &Policy { enabled })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_recovers_ready_servers_and_limits_crash_loops() {
        let mut recovery = Recovery::default();
        let now = Instant::now();
        assert!(recovery.failure(true, true, now).is_none());
        for expected in [30, 60, 120] {
            recovery.start(true);
            recovery.ready();
            assert_eq!(
                recovery.failure(true, true, Instant::now()),
                Some(Duration::from_secs(expected))
            );
        }
        recovery.start(true);
        recovery.ready();
        assert!(recovery.failure(true, true, Instant::now()).is_none());
        recovery.start(false);
        recovery.ready();
        assert!(recovery.failure(false, true, Instant::now()).is_none());
        recovery.ready();
        assert!(recovery.failure(true, false, Instant::now()).is_none());
    }
    #[test]
    fn cancel_clears_pending_and_healthy_sessions_reset_attempt_budget() {
        let mut recovery = Recovery::default();
        recovery.ready();
        recovery.failure(true, true, Instant::now());
        recovery.cancel();
        assert_eq!(recovery.view()["pending"], false);
        recovery.start(true);
        recovery.ready = Some(Instant::now() - Duration::from_secs(601));
        assert_eq!(
            recovery.failure(true, true, Instant::now()),
            Some(Duration::from_secs(30))
        );
    }
}
