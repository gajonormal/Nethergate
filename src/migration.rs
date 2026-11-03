//! Forward-only migration into a new offline profile; the original is never upgraded.
use crate::{
    backups,
    config::{self, Config, Software},
    plugins, transfer,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn numeric(version: &str) -> Result<Vec<u32>> {
    ensure!(!version.is_empty() && version.len() < 64, "Versão inválida");
    version
        .split('.')
        .map(|p| {
            p.parse::<u32>()
                .context("Escolhe uma versão estável numérica de Minecraft")
        })
        .collect()
}
pub fn validate_upgrade(from: &str, to: &str) -> Result<u32> {
    let from = numeric(from)?;
    let to = numeric(to)?;
    ensure!(
        to > from,
        "Escolhe uma versão superior; downgrade de mundos não é suportado"
    );
    Ok(if to[0] >= 26 { 25 } else { 21 })
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub target: String,
    pub name: String,
    pub port: u16,
    pub fingerprint: String,
    pub acknowledged: bool,
}
pub fn versions(directory: &Path) -> Result<Vec<String>> {
    let cfg = Config::load(directory)?;
    let values = match cfg.software {
        Software::Paper => crate::paper::versions(&cfg.contact)?,
        Software::Fabric => crate::fabric::versions()?,
    };
    Ok(values
        .into_iter()
        .filter(|v| validate_upgrade(&cfg.minecraft, v).is_ok())
        .collect())
}
fn base_plan(directory: &Path, target: &str) -> Result<Value> {
    let cfg = Config::load(directory)?;
    let java = validate_upgrade(&cfg.minecraft, target)?;
    backups::ensure_no_pending(directory)?;
    plugins::ensure_no_pending(directory)?;
    let inventory = plugins::inventory(directory)?;
    ensure!(inventory["external"].as_array().context("Inventário inválido")?.is_empty(),"Identifica/associa ou desativa os JARs manuais antes de migrar; não é possível verificar a sua compatibilidade");
    let addons = if inventory["installed"]
        .as_array()
        .context("O inventário de migração não contém uma lista de extensões instaladas")?
        .is_empty()
    {
        json!({"items":[],"warnings":[]})
    } else {
        serde_json::to_value(plugins::solver::preview_for(directory, &[], Some(target))?)?
    };
    let mut plan = json!({"from":cfg.minecraft,"target":target,"javaMajor":java,"software":cfg.software,"sourceFingerprint":transfer::source_fingerprint(directory)?,"addons":addons,"fingerprint":"","warnings":["A migração cria um perfil novo e parado. O original fica preservado.","O Minecraft converte o mundo no primeiro arranque da nova cópia; testa-a antes de a partilhar.","Configurações privadas de plugins/mods, operadores e integrações de rede precisam de revisão no novo perfil."]});
    plan["fingerprint"] = json!(format!("{:x}", Sha256::digest(serde_json::to_vec(&plan)?)));
    Ok(plan)
}
pub fn preview(directory: &Path, target: &str) -> Result<Value> {
    ensure!(
        versions(directory)?.iter().any(|v| v == target),
        "A versão não está disponível no catálogo oficial deste software"
    );
    base_plan(directory, target)
}
fn prepare_with(
    root: &Path,
    id: &str,
    source: &Path,
    name: &str,
    input: &Input,
    expected: &Value,
    prepare_installation: impl FnOnce(&Path, &Config, &str) -> Result<()>,
) -> Result<(String, PathBuf, Value)> {
    ensure!(
        input.acknowledged,
        "Confirma que vais testar a nova cópia antes de substituir o servidor original"
    );
    ensure!(
        expected["fingerprint"].as_str() == Some(&input.fingerprint),
        "O plano mudou; revê novamente"
    );
    ensure!(
        transfer::source_fingerprint(source)?
            == expected["sourceFingerprint"]
                .as_str()
                .context("Plano inválido")?,
        "O servidor mudou desde a revisão"
    );
    let key = transfer::key()?;
    let parent = root.join("servers");
    fs::create_dir_all(&parent)?;
    crate::backup_destination::directory(&parent)?;
    let stage = parent.join(format!(".migration-{key}.partial"));
    let target = parent.join(&key);
    let packages = root.join("hub-data/migration-packages");
    fs::create_dir_all(&packages)?;
    let backup = if backups::world_ready(source)? {
        Some(backups::create(
            &crate::backup_destination::resolve(root, id)?,
            id,
            source,
            "before_migration",
        )?)
    } else {
        None
    };
    let export = transfer::export(source, name, &packages)?;
    let package = Path::new(export["path"].as_str().context("Pacote inválido")?);
    let reviewed = transfer::inspect(package)?;
    let mut cfg = Config::load(source)?;
    cfg.port = input.port;
    cfg.java = "auto".into();
    transfer::unpack(
        package,
        reviewed["fingerprint"]
            .as_str()
            .context("A revisão da migração não contém uma impressão digital válida")?,
        &stage,
        &cfg,
    )?;
    let previous = stage.join(".nethergate-previous-installation");
    fs::create_dir(&previous)?;
    for file in [cfg.software.jar(), cfg.software.manifest()] {
        if stage.join(file).exists() {
            fs::rename(stage.join(file), previous.join(file))?;
        }
    }
    cfg.minecraft = input.target.clone();
    cfg.java_major = validate_upgrade(
        expected["from"]
            .as_str()
            .context("A revisão da migração não contém a versão de origem")?,
        &input.target,
    )?;
    cfg.validate()?;
    crate::hub::replace_json(&stage.join("nethergate.json"), &cfg)?;
    prepare_installation(&stage, &cfg, &key)?;
    config::write_json(
        &stage.join("nethergate-migration.json"),
        &json!({"format":1,"from":expected["from"],"target":input.target,"originalProfile":id,"sourceFingerprint":expected["sourceFingerprint"],"backup":backup,"package":package,"firstStartPending":true}),
    )?;
    ensure!(
        transfer::source_fingerprint(source)?
            == expected["sourceFingerprint"]
                .as_str()
                .context("A revisão da migração não contém a impressão digital da origem")?,
        "O original mudou durante a cópia; o perfil novo não foi publicado"
    );
    fs::rename(&stage, &target)?;
    Ok((key, target, json!({"backup":backup,"package":package})))
}
pub fn prepare(
    root: &Path,
    id: &str,
    source: &Path,
    name: &str,
    input: &Input,
) -> Result<(String, PathBuf, Value)> {
    let plan = preview(source, &input.target)?;
    prepare_with(
        root,
        id,
        source,
        name,
        input,
        &plan,
        |directory, cfg, new_id| {
            match cfg.software {
                Software::Paper => crate::paper::install(directory, cfg)?,
                Software::Fabric => crate::fabric::install(directory, cfg)?,
            };
            if plugins::inventory(directory)?["installed"]
                .as_array()
                .context("O inventário não contém uma lista de extensões instaladas")?
                .is_empty()
            {
                return Ok(());
            }
            let set = plugins::solver::preview(directory, &[])?;
            ensure!(serde_json::to_value(&set.items)?==plan["addons"]["items"],"As versões dos mods mudaram durante a preparação; revê novamente. O original não foi alterado");
            if !set.items.is_empty() {
                plugins::solver::install(root, new_id, directory, &set.projects, &set.fingerprint)?;
            }
            Ok(())
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forward_versions_only() {
        assert_eq!(validate_upgrade("1.21.11", "26.1").unwrap(), 25);
        assert!(validate_upgrade("26.1", "1.21.11").is_err());
        assert!(validate_upgrade("1.21.11", "1.21.11").is_err());
        assert!(validate_upgrade("1.21.11", "26.1-snapshot").is_err());
    }
    #[test]
    #[ignore = "Downloads official target Paper into an isolated migration clone; does not start Minecraft"]
    fn real_paper_target_is_prepared_without_changing_the_source() {
        let root =
            std::env::temp_dir().join(format!("ng-migration-live-{}", transfer::key().unwrap()));
        let source = root.join("source");
        fs::create_dir_all(source.join("world")).unwrap();
        config::write_json(
            &source.join("nethergate.json"),
            &Config::new("1.21".into(), "https://example.org".into(), 21).unwrap(),
        )
        .unwrap();
        fs::write(source.join("server.properties"), "level-name=world\n").unwrap();
        fs::write(source.join("world/level.dat"), b"synthetic-not-started").unwrap();
        let target = versions(&source)
            .unwrap()
            .into_iter()
            .find(|v| v == "1.21.11")
            .expect("Official Paper 1.21.11 unavailable");
        let plan = preview(&source, &target).unwrap();
        let input = Input {
            target,
            name: "Migrated".into(),
            port: 25583,
            fingerprint: plan["fingerprint"].as_str().unwrap().into(),
            acknowledged: true,
        };
        let (_, directory, _) = prepare(&root, "source", &source, "Original", &input).unwrap();
        let cfg = Config::load(&directory).unwrap();
        let receipt: crate::paper::Installed =
            serde_json::from_slice(&fs::read(directory.join("paper.lock.json")).unwrap()).unwrap();
        assert_eq!(receipt.minecraft, cfg.minecraft);
        assert_eq!(
            crate::paper::hash_file(&directory.join("paper.jar")).unwrap(),
            receipt.sha256
        );
        assert_eq!(Config::load(&source).unwrap().minecraft, "1.21");
        assert_eq!(
            fs::read(source.join("world/level.dat")).unwrap(),
            b"synthetic-not-started"
        );
    }
    #[test]
    fn clone_migration_preserves_original_and_rejects_stale_plans() {
        let root = std::env::temp_dir().join(format!("ng-migrate-{}", transfer::key().unwrap()));
        let source = root.join("source");
        fs::create_dir_all(source.join("world")).unwrap();
        config::write_json(
            &source.join("nethergate.json"),
            &Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap(),
        )
        .unwrap();
        fs::write(source.join("world/level.dat"), b"old-world").unwrap();
        fs::write(source.join("server.properties"), "level-name=world\n").unwrap();
        let plan = base_plan(&source, "26.1").unwrap();
        let mut input = Input {
            target: "26.1".into(),
            name: "New".into(),
            port: 25582,
            fingerprint: "stale".into(),
            acknowledged: true,
        };
        assert!(prepare_with(
            &root,
            "fixture",
            &source,
            "Old",
            &input,
            &plan,
            |_, _, _| Ok(())
        )
        .is_err());
        input.fingerprint = plan["fingerprint"].as_str().unwrap().into();
        let (_, target, _) = prepare_with(
            &root,
            "fixture",
            &source,
            "Old",
            &input,
            &plan,
            |dir, cfg, _| {
                fs::write(dir.join(cfg.software.jar()), b"new-binary")?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(Config::load(&source).unwrap().minecraft, "1.21.11");
        assert_eq!(Config::load(&target).unwrap().minecraft, "26.1");
        assert_eq!(Config::load(&target).unwrap().java_major, 25);
        assert_eq!(
            fs::read(source.join("world/level.dat")).unwrap(),
            b"old-world"
        );
        assert_eq!(
            fs::read(target.join("world/level.dat")).unwrap(),
            b"old-world"
        );
        assert!(target.join("nethergate-migration.json").is_file());
        assert!(
            backups::list(&root.join("hub-data/backups/fixture"), "fixture")
                .unwrap()
                .iter()
                .any(|b| b.reason == "before_migration")
        );
    }
}
