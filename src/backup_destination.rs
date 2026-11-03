//! Per-profile backup stores. An unavailable external disk never falls back silently.
use crate::backups;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Destination {
    base: PathBuf,
    folder: String,
    #[serde(default = "enabled")]
    enabled: bool,
}
fn enabled() -> bool {
    true
}

fn settings(root: &Path, id: &str) -> Result<PathBuf> {
    backups::component(id)?;
    Ok(root
        .join("hub-data/backup-destinations")
        .join(format!("{id}.json")))
}
/// Validate each ancestor as well as the leaf: do not follow junctions into another store.
pub(crate) fn directory(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "Escolhe uma pasta absoluta existente");
    for ancestor in path.ancestors() {
        ensure!(
            backups::ordinary(ancestor)?.is_dir(),
            "O destino não está disponível ou não é uma pasta normal"
        );
    }
    Ok(path.canonicalize()?)
}
fn load(root: &Path, id: &str) -> Result<Option<Destination>> {
    let path = settings(root, id)?;
    if !path.exists() {
        return Ok(None);
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 16384,
        "Destino de backups inválido"
    );
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}
pub fn resolve(root: &Path, id: &str) -> Result<PathBuf> {
    let Some(value) = load(root, id)? else {
        return Ok(root.join("hub-data/backups").join(id));
    };
    if !value.enabled {
        return Ok(root.join("hub-data/backups").join(id));
    }
    backups::component(&value.folder)?;
    ensure!(
        value.folder.starts_with("NetherGate-"),
        "Pasta de backups inválida"
    );
    let base = directory(&value.base).context("Volta a ligar o disco ou escolhe outro destino. Não foi usado o armazenamento local como alternativa")?;
    let store = base.join(&value.folder);
    directory(&store)?;
    let marker = store.join(".nethergate-store.json");
    backups::ordinary(&marker)?;
    ensure!(
        backups::ordinary(&marker)?.len() <= 4096,
        "Marcador inválido"
    );
    ensure!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&marker)?)?
            == serde_json::json!({"serverId":id,"format":1}),
        "A pasta deixou de corresponder ao destino autorizado"
    );
    Ok(store)
}
pub fn status(root: &Path, id: &str) -> Result<serde_json::Value> {
    let saved = load(root, id)?;
    let (path, error) = match resolve(root, id) {
        Ok(path) => (Some(path), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    Ok(
        serde_json::json!({"external":saved.as_ref().is_some_and(|d|d.enabled),"base":saved.map(|d|d.base),"path":path,"error":error,"localPath":root.join("hub-data/backups").join(id)}),
    )
}
pub fn save(root: &Path, id: &str, base: Option<&Path>, server: &Path) -> Result<()> {
    let settings = settings(root, id)?;
    let Some(base) = base else {
        if let Some(mut previous) = load(root, id)? {
            previous.enabled = false;
            crate::hub::replace_json(&settings, &previous)?;
        }
        return Ok(());
    };
    let base = directory(base)?;
    let server = server.canonicalize()?;
    ensure!(
        !base.starts_with(&server) && !server.starts_with(&base),
        "Escolhe uma pasta fora do servidor e dos seus antecessores"
    );
    if let Some(mut previous) = load(root, id)? {
        if directory(&previous.base).ok().as_ref() == Some(&base) {
            backups::component(&previous.folder)?;
            let store = directory(&base.join(&previous.folder))?;
            let marker = store.join(".nethergate-store.json");
            ensure!(
                backups::ordinary(&marker)?.len() <= 4096,
                "Marcador inválido"
            );
            ensure!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(marker)?)?
                    == serde_json::json!({"serverId":id,"format":1}),
                "Destino alterado"
            );
            previous.enabled = true;
            crate::hub::replace_json(&settings, &previous)?;
            return Ok(());
        }
    }
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("{e}"))?;
    let key: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let folder = format!("NetherGate-{key}-{id}");
    let store = base.join(&folder);
    fs::create_dir(&store)?;
    crate::config::write_json(
        &store.join(".nethergate-store.json"),
        &serde_json::json!({"serverId":id,"format":1}),
    )?;
    fs::create_dir_all(
        settings
            .parent()
            .context("A configuração do destino de backups não tem pasta pai")?,
    )?;
    crate::hub::replace_json(
        &settings,
        &Destination {
            base,
            folder,
            enabled: true,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_store_supports_verified_backup_restore_and_reselection() {
        let root = std::env::temp_dir().join(format!(
            "ng-destination-roundtrip-{}-{}",
            std::process::id(),
            crate::backup_schedule::now().unwrap()
        ));
        let server = root.join("server");
        let external = root.join("external");
        fs::create_dir_all(server.join("world")).unwrap();
        fs::create_dir_all(&external).unwrap();
        crate::config::write_json(
            &server.join("nethergate.json"),
            &crate::config::Config::new("1.21.11".into(), "https://example.org".into(), 21)
                .unwrap(),
        )
        .unwrap();
        fs::write(server.join("server.properties"), "level-name=world\n").unwrap();
        fs::write(server.join("world/level.dat"), "original").unwrap();
        save(&root, "one", Some(&external), &server).unwrap();
        let store = resolve(&root, "one").unwrap();
        let copy = backups::create(&store, "one", &server, "manual").unwrap();
        assert_eq!(backups::list(&store, "one").unwrap().len(), 1);
        save(&root, "one", None, &server).unwrap();
        save(&root, "one", Some(&external), &server).unwrap();
        assert_eq!(resolve(&root, "one").unwrap(), store);
        fs::write(server.join("world/level.dat"), "changed").unwrap();
        backups::restore(&store, "one", &server, &copy.id).unwrap();
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "original"
        );
    }
    #[test]
    fn destinations_are_isolated_missing_disks_fail_and_history_is_preserved() {
        let root = std::env::temp_dir().join(format!(
            "ng-destination-{}-{}",
            std::process::id(),
            crate::backup_schedule::now().unwrap()
        ));
        fs::create_dir_all(root.join("server")).unwrap();
        fs::create_dir_all(root.join("external")).unwrap();
        save(
            &root,
            "one",
            Some(&root.join("external")),
            &root.join("server"),
        )
        .unwrap();
        let first = resolve(&root, "one").unwrap();
        fs::write(first.join("keep"), "history").unwrap();
        save(
            &root,
            "two",
            Some(&root.join("external")),
            &root.join("server"),
        )
        .unwrap();
        assert_ne!(resolve(&root, "two").unwrap(), first);
        assert!(save(
            &root,
            "one",
            Some(&root.join("server")),
            &root.join("server")
        )
        .is_err());
        fs::rename(root.join("external"), root.join("disconnected")).unwrap();
        assert!(resolve(&root, "one").is_err());
        assert!(!root.join("external").exists());
        save(&root, "one", None, &root.join("server")).unwrap();
        assert_eq!(
            resolve(&root, "one").unwrap(),
            root.join("hub-data/backups/one")
        );
        assert!(root
            .join("disconnected")
            .join(first.file_name().unwrap())
            .join("keep")
            .exists());
    }
}
