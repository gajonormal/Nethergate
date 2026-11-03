//! Portable, reviewed folder packages. Never restores machine paths or network credentials.
use crate::{
    backups,
    config::{self, Config, Software},
    gameplay::Gameplay,
    paper::hash_file,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const MANIFEST: &str = "nethergate-transfer.json";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub software: Software,
    pub minecraft: String,
    pub memory_mb: u32,
    pub java_major: u32,
    gameplay: Gameplay,
    primary_world: String,
    worlds: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    format: u32,
    profile: Profile,
    files: Vec<Record>,
}

pub(crate) fn key() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
pub(crate) fn source_fingerprint(directory: &Path) -> Result<String> {
    let cfg = Config::load(directory)?;
    let mut records = vec![];
    let worlds = if backups::world_ready(directory)? {
        backups::world_names(directory)?
    } else {
        vec![]
    };
    let _guards = backups::world_locks(directory, &worlds)?;
    for world in worlds {
        collect(directory, &world, &mut records, 0)?;
    }
    for file in [
        "nethergate.json",
        "server.properties",
        "whitelist.json",
        cfg.software.registry(),
        cfg.software.jar(),
        cfg.software.manifest(),
    ] {
        if directory.join(file).exists() {
            collect(directory, file, &mut records, 0)?;
        }
    }
    if directory.join(cfg.software.folder()).exists() {
        collect(directory, cfg.software.folder(), &mut records, 0)?;
    }
    if directory.join("config").exists() {
        collect(directory, "config", &mut records, 0)?;
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&records)?)
    ))
}
fn relative(value: &str) -> Result<PathBuf> {
    ensure!(
        !value.is_empty() && !value.contains('\\'),
        "Caminho inválido no pacote"
    );
    for part in value.split('/') {
        backups::component(part)?;
    }
    Ok(PathBuf::from(value))
}
fn collect(source: &Path, rel: &str, out: &mut Vec<Record>, depth: usize) -> Result<()> {
    ensure!(
        depth <= 64 && out.len() < 200_000,
        "Pacote demasiado complexo"
    );
    let path = source.join(relative(rel)?);
    let meta = backups::ordinary(&path)?;
    if meta.is_dir() {
        for item in fs::read_dir(&path)? {
            let item = item?;
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Nome inválido"))?;
            if name == "session.lock" {
                continue;
            }
            collect(source, &format!("{rel}/{name}"), out, depth + 1)?;
        }
    } else {
        out.push(Record {
            path: rel.into(),
            bytes: meta.len(),
            sha256: hash_file(&path)?,
        });
    }
    Ok(())
}
fn copy(source: &Path, to: &Path, records: &[Record]) -> Result<()> {
    for record in records {
        let rel = relative(&record.path)?;
        let from = source.join(&rel);
        for ancestor in from.ancestors().take_while(|p| *p != source) {
            backups::ordinary(ancestor)?;
        }
        ensure!(
            backups::ordinary(&from)?.len() == record.bytes,
            "Ficheiro alterado: {}",
            record.path
        );
        let dest = to.join(&rel);
        fs::create_dir_all(dest.parent().context("Caminho inválido")?)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)?;
        let bytes = std::io::copy(
            &mut fs::File::open(&from)?.take(record.bytes.saturating_add(1)),
            &mut output,
        )?;
        output.sync_all()?;
        ensure!(
            bytes == record.bytes
                && hash_file(&dest)? == record.sha256
                && hash_file(&from)? == record.sha256,
            "Falha de integridade: {}",
            record.path
        );
    }
    Ok(())
}
pub fn export(directory: &Path, name: &str, parent: &Path) -> Result<serde_json::Value> {
    backups::ensure_no_pending(directory)?;
    crate::plugins::ensure_no_pending(directory)?;
    let parent = crate::backup_destination::directory(parent)?;
    let source = directory.canonicalize()?;
    ensure!(
        !parent.starts_with(&source),
        "O pacote não pode ficar dentro do servidor"
    );
    let cfg = Config::load(directory)?;
    let worlds = if backups::world_ready(directory)? {
        backups::world_names(directory)?
    } else {
        ensure!(
            !directory.join(backups::primary_world(directory)?).exists(),
            "Mundo incompleto; resolve-o antes de exportar"
        );
        vec![]
    };
    let _locks = backups::world_locks(directory, &worlds)?;
    let mut records = vec![];
    for world in &worlds {
        collect(directory, world, &mut records, 0)?;
    }
    for file in [
        cfg.software.jar(),
        cfg.software.manifest(),
        cfg.software.registry(),
        "whitelist.json",
    ] {
        if directory.join(file).exists() {
            collect(directory, file, &mut records, 0)?;
        }
    }
    let addons = directory.join(cfg.software.folder());
    if addons.exists() {
        backups::ordinary(&addons)?;
        for entry in fs::read_dir(addons)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Nome inválido"))?;
            if name.to_ascii_lowercase().ends_with(".jar") {
                collect(
                    directory,
                    &format!("{}/{name}", cfg.software.folder()),
                    &mut records,
                    0,
                )?;
            }
        }
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    let package = Package {
        format: 1,
        profile: Profile {
            name: name.into(),
            software: cfg.software,
            minecraft: cfg.minecraft,
            memory_mb: cfg.memory_mb,
            java_major: cfg.java_major,
            gameplay: Gameplay::load(directory)?,
            primary_world: backups::primary_world(directory)?,
            worlds,
        },
        files: records,
    };
    let id = key()?;
    let partial = parent.join(format!("NetherGate-server-{id}.partial"));
    let target = parent.join(format!("NetherGate-server-{id}"));
    fs::create_dir(&partial)?;
    copy(directory, &partial.join("files"), &package.files)?;
    config::write_json(&partial.join(MANIFEST), &package)?;
    inspect(&partial)?;
    fs::rename(&partial, &target)?;
    Ok(
        serde_json::json!({"path":target,"fileCount":package.files.len(),"bytes":package.files.iter().map(|r|r.bytes).sum::<u64>()}),
    )
}
fn read(path: &Path) -> Result<(PathBuf, Package, Vec<u8>)> {
    let path = crate::backup_destination::directory(path)?;
    let manifest = path.join(MANIFEST);
    ensure!(
        backups::ordinary(&manifest)?.len() <= 32 * 1024 * 1024,
        "Manifesto demasiado grande"
    );
    let bytes = fs::read(manifest)?;
    let package: Package = serde_json::from_slice(&bytes)?;
    ensure!(
        package.format == 1 && package.files.len() <= 200_000,
        "Formato de transferência não suportado"
    );
    let p = &package.profile;
    let mut cfg = Config::new(
        p.minecraft.clone(),
        "https://example.org".into(),
        p.java_major,
    )?;
    cfg.memory_mb = p.memory_mb;
    cfg.validate()?;
    ensure!(
        p.memory_mb <= 65536
            && !p.name.is_empty()
            && p.name.chars().count() <= 60
            && !p.name.chars().any(char::is_control),
        "Perfil inválido"
    );
    p.gameplay.validate()?;
    backups::component(&p.primary_world)?;
    let mut worlds = HashSet::new();
    for world in &p.worlds {
        backups::component(world)?;
        ensure!(worlds.insert(world.to_lowercase()), "Mundos duplicados");
    }
    ensure!(
        p.worlds.is_empty() || p.worlds.contains(&p.primary_world),
        "Falta o mundo principal"
    );
    let mut names = HashSet::new();
    let mut total = 0u64;
    for record in &package.files {
        let rel = relative(&record.path)?;
        let parts: Vec<_> = record.path.split('/').collect();
        let allowed = (parts.len() >= 2 && p.worlds.contains(&parts[0].to_string()))
            || (parts.len() == 2
                && parts[0] == p.software.folder()
                && parts[1].to_ascii_lowercase().ends_with(".jar"))
            || (parts.len() == 1
                && [
                    p.software.jar(),
                    p.software.manifest(),
                    p.software.registry(),
                    "whitelist.json",
                ]
                .contains(&parts[0]));
        ensure!(
            allowed && names.insert(record.path.to_lowercase()),
            "Ficheiro inesperado ou duplicado no pacote"
        );
        ensure!(
            record.sha256.len() == 64 && record.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Hash inválido"
        );
        total = total
            .checked_add(record.bytes)
            .context("Tamanho inválido")?;
        ensure!(
            total <= 1024 * 1024 * 1024 * 1024,
            "Pacote superior a 1 TiB"
        );
        let file = path.join("files").join(rel);
        for ancestor in file.ancestors().take_while(|a| *a != path) {
            backups::ordinary(ancestor)?;
        }
        ensure!(
            backups::ordinary(&file)?.is_file()
                && fs::metadata(&file)?.len() == record.bytes
                && hash_file(&file)? == record.sha256,
            "Ficheiro alterado ou incompleto: {}",
            record.path
        );
    }
    for world in &p.worlds {
        ensure!(
            names.contains(&format!("{world}/level.dat").to_lowercase()),
            "Mundo sem level.dat"
        );
    }
    ensure!(
        names.contains(p.software.jar()) == names.contains(p.software.manifest()),
        "Instalação incompleta no pacote"
    );
    Ok((path, package, bytes))
}
pub fn inspect(path: &Path) -> Result<serde_json::Value> {
    let (path, p, bytes) = read(path)?;
    Ok(
        serde_json::json!({"path":path,"profile":p.profile,"fileCount":p.files.len(),"bytes":p.files.iter().map(|r|r.bytes).sum::<u64>(),"fingerprint":format!("{:x}",Sha256::digest(bytes)),"reconfigure":["Java e contacto neste computador","Porta e acesso LAN/internet","DuckDNS e conta playit","Agendamentos e destino de backups","Configurações privadas de plugins/mods e operadores"]}),
    )
}
pub fn unpack(path: &Path, expected: &str, target: &Path, cfg: &Config) -> Result<()> {
    let (path, p, bytes) = read(path)?;
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == expected,
        "O pacote mudou desde a revisão"
    );
    ensure!(
        cfg.minecraft == p.profile.minecraft && cfg.software == p.profile.software,
        "Perfil incompatível com o pacote"
    );
    fs::create_dir(target)?;
    copy(&path.join("files"), target, &p.files)?;
    config::write_json(&target.join("nethergate.json"), cfg)?;
    config::set_property(&target.join("eula.txt"), "eula", "true")?;
    for (key, value) in [
        ("server-ip", "127.0.0.1".to_string()),
        ("server-port", cfg.port.to_string()),
        ("online-mode", "true".into()),
        ("white-list", "true".into()),
        ("enforce-whitelist", "true".into()),
        ("enable-rcon", "false".into()),
        ("enable-query", "false".into()),
        ("level-name", p.profile.primary_world),
    ] {
        config::set_property(&target.join("server.properties"), key, &value)?;
    }
    p.profile.gameplay.save(target)?;
    let inventory = crate::plugins::inventory(target)?;
    ensure!(
        inventory["installed"]
            .as_array()
            .context("Inventário inválido")?
            .iter()
            .all(|r| r["intact"] == true),
        "JAR gerido inválido no pacote"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("ng-transfer-{}", key().unwrap()));
        let server = root.join("source");
        fs::create_dir_all(server.join("world")).unwrap();
        fs::create_dir(root.join("packages")).unwrap();
        let cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
        config::write_json(&server.join("nethergate.json"), &cfg).unwrap();
        fs::write(server.join("world/level.dat"), b"world-data").unwrap();
        fs::write(
            server.join("server.properties"),
            "level-name=world\nserver-ip=192.168.1.2\nrcon.password=secret\ndifficulty=hard\n",
        )
        .unwrap();
        fs::create_dir(server.join("config")).unwrap();
        fs::write(server.join("config/private.txt"), "secret").unwrap();
        fs::write(server.join("ops.json"), "private operators").unwrap();
        fs::write(server.join("whitelist.json"), "[]").unwrap();
        (root, server)
    }
    #[test]
    fn portable_roundtrip_rebuilds_machine_settings_and_excludes_private_files() {
        let (root, source) = fixture();
        let exported = export(&source, "Portable", &root.join("packages")).unwrap();
        let package = Path::new(exported["path"].as_str().unwrap());
        let review = inspect(package).unwrap();
        assert!(!package.join("files/config").exists());
        assert!(!package.join("files/ops.json").exists());
        assert!(!package.join("files/nethergate.json").exists());
        let mut cfg = Config::new("1.21.11".into(), "https://new.example.org".into(), 21).unwrap();
        cfg.port = 25570;
        cfg.java = "auto".into();
        let target = root.join("imported");
        unpack(
            package,
            review["fingerprint"].as_str().unwrap(),
            &target,
            &cfg,
        )
        .unwrap();
        assert_eq!(
            fs::read(target.join("world/level.dat")).unwrap(),
            b"world-data"
        );
        let props = fs::read_to_string(target.join("server.properties")).unwrap();
        assert!(props.contains("server-ip=127.0.0.1"));
        assert!(props.contains("server-port=25570"));
        assert!(props.contains("white-list=true"));
        assert!(!props.contains("secret"));
        assert!(!props.contains("192.168"));
        assert_eq!(Gameplay::load(&target).unwrap().difficulty, "hard");
        assert!(unpack(
            package,
            review["fingerprint"].as_str().unwrap(),
            &target,
            &cfg
        )
        .is_err());
        fs::write(package.join("files/world/level.dat"), "corrupt").unwrap();
        assert!(inspect(package).is_err());
    }
    #[test]
    fn package_paths_and_stale_reviews_are_rejected_before_publication() {
        let (root, source) = fixture();
        let exported = export(&source, "Portable", &root.join("packages")).unwrap();
        let package = Path::new(exported["path"].as_str().unwrap());
        let mut p: serde_json::Value =
            serde_json::from_slice(&fs::read(package.join(MANIFEST)).unwrap()).unwrap();
        let cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
        assert!(unpack(package, "old-plan", &root.join("never-created"), &cfg).is_err());
        assert!(!root.join("never-created").exists());
        p["files"][0]["path"] = serde_json::json!("../escape");
        fs::write(package.join(MANIFEST), serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(inspect(package).is_err());
        assert!(export(&source, "bad", &source.join("world")).is_err());
    }
}
