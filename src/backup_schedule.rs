//! Persisted offline backup policy. No server is stopped by the scheduler.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub keep: usize,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 60,
            keep: 7,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(15..=1440).contains(&self.interval_minutes) || !(2..=50).contains(&self.keep) {
            bail!("Escolhe um intervalo de 15 a 1440 minutos e conserva entre 2 e 50 backups automáticos");
        }
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    pub policy: Policy,
    pub next_due: Option<u64>,
    pub last_success: Option<u64>,
    pub last_backup_id: Option<String>,
    pub error: Option<String>,
    pub cleanup_warning: Option<String>,
}
pub fn now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        bail!("Identificador de servidor inválido");
    }
    Ok(root
        .join("hub-data/backup-schedules")
        .join(format!("{id}.json")))
}
pub fn load(root: &Path, id: &str) -> Result<Schedule> {
    let path = path(root, id)?;
    if !path.exists() {
        return Ok(Schedule::default());
    }
    let data = fs::read(path)?;
    if data.len() > 32768 {
        bail!("Configuração de backups inválida");
    }
    let schedule: Schedule =
        serde_json::from_slice(&data).context("Não foi possível ler o agendamento de backups")?;
    schedule.policy.validate()?;
    Ok(schedule)
}
pub fn persist(root: &Path, id: &str, value: &Schedule) -> Result<()> {
    value.policy.validate()?;
    let path = path(root, id)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração do agendamento não tem pasta pai")?,
    )?;
    crate::hub::replace_json(&path, value)
}
pub fn save(root: &Path, id: &str, policy: Policy, timestamp: u64) -> Result<Schedule> {
    policy.validate()?;
    let mut value = load(root, id)?;
    value.next_due = if policy.enabled {
        Some(
            value
                .last_success
                .map(|last| last.saturating_add(policy.interval_minutes as u64 * 60000))
                .unwrap_or(timestamp),
        )
    } else {
        None
    };
    value.policy = policy;
    persist(root, id, &value)?;
    Ok(value)
}
impl Schedule {
    pub fn due(&self, timestamp: u64) -> bool {
        self.policy.enabled && self.next_due.is_none_or(|due| due <= timestamp)
    }
    pub fn succeeded(&mut self, id: String, timestamp: u64, warning: Option<String>) {
        self.last_success = Some(timestamp);
        self.last_backup_id = Some(id);
        self.error = None;
        self.cleanup_warning = warning;
        self.next_due = self
            .policy
            .enabled
            .then_some(timestamp.saturating_add(self.policy.interval_minutes as u64 * 60000));
    }
    pub fn failed(&mut self, error: String, timestamp: u64) {
        self.error = Some(error);
        self.next_due = self
            .policy
            .enabled
            .then_some(timestamp.saturating_add(300000));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedules_wait_for_due_time_and_failures_back_off() {
        let mut s = Schedule::default();
        assert!(!s.due(100));
        s.policy.enabled = true;
        s.next_due = Some(100);
        assert!(!s.due(99));
        assert!(s.due(100));
        s.succeeded("copy".into(), 100, None);
        assert_eq!(s.next_due, Some(3600100));
        s.failed("disk full".into(), 200);
        assert_eq!(s.next_due, Some(300200));
        assert!(!s.due(201));
        s.policy.enabled = false;
        assert!(!s.due(u64::MAX));
        assert!(Policy {
            enabled: true,
            interval_minutes: 1,
            keep: 7
        }
        .validate()
        .is_err());
        assert!(Policy {
            enabled: true,
            interval_minutes: 60,
            keep: 1
        }
        .validate()
        .is_err());
    }
}
