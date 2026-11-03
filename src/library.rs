//! Existing-library selection, validation and atomic switching. Never moves worlds.
use crate::{
    config::{self, Config},
    hub::{self, Hub},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

#[derive(Deserialize)]
struct Entry {
    id: String,
    name: String,
    directory: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub root: PathBuf,
    pub servers: Vec<Profile>,
    pub fingerprint: String,
    pub external_directories: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Profile {
    name: String,
    software: config::Software,
    version: String,
    port: u16,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    schema: u8,
    root: PathBuf,
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("O ficheiro de biblioteca é demasiado grande");
    }
    Ok(bytes)
}
fn local(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("Escolhe uma pasta absoluta neste computador");
    }
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if (text.starts_with("\\\\") && !text.starts_with("\\\\?\\"))
            || text.to_ascii_uppercase().starts_with("\\\\?\\UNC\\")
        {
            bail!("A biblioteca tem de estar num disco local");
        }
    }
    Ok(())
}
pub fn inspect(path: &Path, check_lock: bool) -> Result<Preview> {
    local(path)?;
    let root = path
        .canonicalize()
        .context("A pasta da biblioteca não está disponível")?;
    local(&root)?;
    let raw = read(&root.join("hub-data/registry.json"), 1_048_576)
        .context("Escolhe a pasta da biblioteca que contém hub-data/registry.json")?;
    let entries: Vec<Entry> =
        serde_json::from_slice(&raw).context("O registo de servidores desta pasta é inválido")?;
    if entries.len() > 1000 {
        bail!("O registo tem demasiados servidores");
    }
    let _guard = if check_lock {
        Some(
            config::lock(&root.join("hub-data"))
                .context("Esta biblioteca já está aberta noutra instância")?,
        )
    } else {
        None
    };
    let mut fingerprint = Sha256::new();
    fingerprint.update(root.to_string_lossy().as_bytes());
    fingerprint.update(raw);
    let mut ids = HashSet::new();
    let mut dirs = HashSet::new();
    let mut servers = Vec::new();
    let mut external = false;
    for entry in entries {
        if entry.id.is_empty()
            || !entry
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || !ids.insert(entry.id)
        {
            bail!("O registo contém identificadores de servidor inválidos ou repetidos");
        }
        if entry.name.trim().is_empty() || entry.name.chars().any(char::is_control) {
            bail!("O registo contém um nome de servidor inválido");
        }
        local(&entry.directory)?;
        let directory = entry
            .directory
            .canonicalize()
            .with_context(|| format!("A pasta do servidor {} não está disponível", entry.name))?;
        local(&directory)?;
        if !dirs.insert(directory.clone()) {
            bail!("O registo aponta mais de uma vez para a mesma pasta de servidor");
        }
        external |= !directory.starts_with(&root);
        let bytes = read(&directory.join("nethergate.json"), 65536)?;
        let cfg: Config =
            serde_json::from_slice(&bytes).context("A configuração de um servidor é inválida")?;
        cfg.validate()?;
        fingerprint.update(directory.to_string_lossy().as_bytes());
        fingerprint.update(bytes);
        servers.push(Profile {
            name: entry.name,
            software: cfg.software,
            version: cfg.minecraft,
            port: cfg.port,
        });
    }
    Ok(Preview {
        root,
        servers,
        fingerprint: format!("{:x}", fingerprint.finalize()),
        external_directories: external,
    })
}
pub fn selected(preferences: &Path) -> Result<Option<PathBuf>> {
    if !preferences.exists() {
        return Ok(None);
    }
    let value: Preference = serde_json::from_slice(&read(preferences, 16384)?)?;
    if value.schema != 1 {
        bail!("Versão desconhecida da seleção de biblioteca");
    }
    Ok(Some(inspect(&value.root, false)?.root))
}
fn save(preferences: &Path, root: &Path) -> Result<()> {
    fs::create_dir_all(
        preferences
            .parent()
            .context("Falta a pasta de preferências")?,
    )?;
    hub::replace_json(
        preferences,
        &Preference {
            schema: 1,
            root: root.into(),
        },
    )
}
pub struct Session {
    hub: RwLock<Arc<Hub>>,
    preferences: PathBuf,
    warning: RwLock<Option<String>>,
}
impl Session {
    pub fn new(root: PathBuf, preferences: PathBuf, warning: Option<String>) -> Result<Self> {
        Ok(Self {
            hub: RwLock::new(Hub::new(root)?),
            preferences,
            warning: RwLock::new(warning),
        })
    }
    pub fn current(&self) -> Arc<Hub> {
        self.hub
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn notification_snapshot(&self) -> Result<crate::notifications::Snapshot> {
        self.hub
            .read()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .notification_snapshot()
    }
    pub fn dispatch(&self, action: &str, payload: Value) -> Result<Value> {
        let hub = self
            .hub
            .read()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        hub::dispatch(&hub, action, payload)
    }
    pub fn info(&self) -> Result<Value> {
        let hub = self
            .hub
            .read()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        Ok(
            json!({"root":hub.root(),"serverCount":hub.list()?.len(),"busy":hub.busy(),"warning":*self.warning.read().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?}),
        )
    }
    pub fn preview(&self, path: &Path) -> Result<Preview> {
        local(path)?;
        let hub = self
            .hub
            .read()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        let same = path.canonicalize().ok().as_deref() == Some(hub.root());
        inspect(path, !same)
    }
    pub fn open(&self, path: &Path, expected: &str) -> Result<Value> {
        local(path)?;
        let mut current = self.hub.try_write().map_err(|_| {
            anyhow::anyhow!("Aguarda o pedido em curso antes de mudar de biblioteca")
        })?;
        let same = path.canonicalize().ok().as_deref() == Some(current.root());
        let candidate = inspect(path, !same)?;
        if candidate.fingerprint != expected {
            bail!(
                "A biblioteca mudou desde a pré-visualização. Escolhe-a novamente antes de abrir"
            );
        }
        if same {
            save(&self.preferences, &candidate.root)?;
            *self
                .warning
                .write()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))? = None;
            return Ok(json!({"root":candidate.root,"changed":false}));
        }
        current.pause_background()?;
        let result = (|| -> Result<Arc<Hub>> {
            let next = Hub::new_paused(candidate.root.clone())?;
            save(&self.preferences, &candidate.root)?;
            Ok(next)
        })();
        match result {
            Ok(next) => {
                *current = next;
                current.resume_background();
                *self
                    .warning
                    .write()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))? =
                    None;
                Ok(json!({"root":candidate.root,"changed":true}))
            }
            Err(error) => {
                current.resume_background();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "nethergate-library-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("hub-data")).unwrap();
        fs::write(root.join("hub-data/registry.json"), "[]").unwrap();
        root
    }
    #[test]
    fn opens_existing_library_persists_and_rejects_stale_preview() {
        let first = fixture("first");
        let second = fixture("second");
        let preferences = first.join("prefs/library.json");
        let session = Session::new(first.clone(), preferences.clone(), None).unwrap();
        let preview = session.preview(&second).unwrap();
        fs::write(second.join("hub-data/registry.json"), "[ ]").unwrap();
        assert!(session.open(&preview.root, &preview.fingerprint).is_err());
        assert_eq!(
            session.info().unwrap()["root"],
            json!(first.canonicalize().unwrap())
        );
        let preview = session.preview(&second).unwrap();
        session.open(&preview.root, &preview.fingerprint).unwrap();
        assert_eq!(
            session.info().unwrap()["root"],
            json!(second.canonicalize().unwrap())
        );
        assert!(first.join("hub-data/registry.json").is_file());
        drop(session);
        assert_eq!(
            selected(&preferences).unwrap(),
            Some(second.canonicalize().unwrap())
        );
    }
    #[test]
    fn refuses_locked_malformed_and_missing_libraries() {
        let root = fixture("locked");
        let guard = config::lock(&root.join("hub-data")).unwrap();
        assert!(inspect(&root, true).is_err());
        drop(guard);
        fs::write(root.join("hub-data/registry.json"), "{}").unwrap();
        assert!(inspect(&root, true).is_err());
        assert!(inspect(Path::new("relative"), true).is_err());
    }
    #[test]
    fn failed_preference_write_keeps_original_hub_open() {
        let first = fixture("original");
        let second = fixture("target");
        let bad = first.join("not-a-directory");
        fs::write(&bad, b"sentinel").unwrap();
        let session = Session::new(first.clone(), bad.join("library.json"), None).unwrap();
        let preview = session.preview(&second).unwrap();
        assert!(session.open(&preview.root, &preview.fingerprint).is_err());
        assert_eq!(
            session.info().unwrap()["root"],
            json!(first.canonicalize().unwrap())
        );
        assert_eq!(fs::read(&bad).unwrap(), b"sentinel");
    }
}
