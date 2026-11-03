//! Hash identification and reversible management of untracked JAR files.
use super::*;
use crate::config;
const PENDING_MANUAL: &str = ".nethergate-manual-operation.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disabled {
    id: String,
    filename: String,
    sha512: String,
    size: u64,
}
fn disabled_dir(directory: &Path) -> Result<PathBuf> {
    secure_folder(directory, &[".nethergate-manual-disabled"])
}
fn disabled(directory: &Path) -> Result<Vec<Disabled>> {
    let path = directory.join(".nethergate-manual-disabled/index.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    ensure!(
        backups::ordinary(&path)?.len() < 1024 * 1024,
        "Índice de JARs desativados inválido"
    );
    let records: Vec<Disabled> = serde_json::from_slice(&fs::read(path)?)?;
    let mut ids = std::collections::HashSet::new();
    for r in &records {
        identifier(&r.id)?;
        backups::component(&r.filename)?;
        ensure!(
            ids.insert(&r.id)
                && r.sha512.len() == 128
                && r.sha512.bytes().all(|b| b.is_ascii_hexdigit()),
            "Índice inválido"
        );
    }
    Ok(records)
}
fn source(directory: &Path, name: &str) -> Result<PathBuf> {
    backups::component(name)?;
    ensure!(
        name.to_ascii_lowercase().ends_with(".jar"),
        "Escolhe um JAR"
    );
    let current = inventory(directory)?;
    ensure!(
        current["external"]
            .as_array()
            .context("Inventário inválido")?
            .iter()
            .any(|v| v.as_str() == Some(name)),
        "Este ficheiro não é um JAR manual atual"
    );
    let path = plugin_dir(directory)?.join(name);
    let meta = backups::ordinary(&path)?;
    ensure!(
        meta.is_file() && meta.len() > 0 && meta.len() <= 128 * 1024 * 1024,
        "O JAR deve ter até 128 MiB"
    );
    Ok(path)
}
pub fn list(directory: &Path) -> Result<Value> {
    let inv = inventory(directory)?;
    let mut rows = vec![];
    for name in inv["external"].as_array().context("Inventário inválido")? {
        let name = name.as_str().context("Nome inválido")?;
        let path = source(directory, name)?;
        rows.push(
            json!({"filename":name,"size":fs::metadata(&path)?.len(),"sha512":digest(&path)?}),
        );
    }
    for i in 0..rows.len() {
        let hash = rows[i]["sha512"].clone();
        let mut duplicates: Vec<_> = rows
            .iter()
            .filter(|r| r["sha512"] == hash)
            .map(|r| r["filename"].clone())
            .collect();
        for record in load(directory)? {
            if record.sha512.eq_ignore_ascii_case(
                hash.as_str()
                    .context("O inventário JAR não contém um hash SHA-512 válido")?,
            ) && intact(directory, &record)
            {
                duplicates.push(json!(record.filename));
            }
        }
        rows[i]["duplicates"] = json!(duplicates);
    }
    Ok(json!({"files":rows,"disabled":disabled(directory)?}))
}
pub fn file_preview(path: &Path) -> Result<Value> {
    crate::backup_destination::directory(path.parent().context("Caminho inválido")?)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Nome inválido")?;
    backups::component(name)?;
    let meta = backups::ordinary(path)?;
    ensure!(
        name.to_ascii_lowercase().ends_with(".jar")
            && meta.is_file()
            && meta.len() > 0
            && meta.len() <= 128 * 1024 * 1024,
        "Escolhe um JAR de até 128 MiB"
    );
    let hash = digest(path)?;
    verify_download(path, meta.len(), &hash)?;
    Ok(json!({"filename":name,"size":meta.len(),"sha512":hash}))
}
pub fn add(directory: &Path, path: &Path, expected: &str) -> Result<Value> {
    ensure_no_pending(directory)?;
    let review = file_preview(path)?;
    ensure!(
        review["sha512"].as_str() == Some(expected),
        "O JAR mudou desde a revisão"
    );
    let folder = plugin_dir(directory)?;
    fs::create_dir_all(&folder)?;
    let name = review["filename"]
        .as_str()
        .context("A revisão do JAR não contém um nome de ficheiro válido")?;
    let target = folder.join(name);
    ensure!(!target.exists(), "Já existe um JAR com este nome");
    for entry in fs::read_dir(&folder)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".jar")
        {
            backups::ordinary(&entry.path())?;
            ensure!(
                digest(&entry.path())? != expected,
                "Já existe uma cópia deste JAR no servidor"
            );
        }
    }
    let staging = folder.join(format!(".manual-{}.partial", crate::transfer::key()?));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    std::io::copy(
        &mut fs::File::open(path)?.take(128 * 1024 * 1024 + 1),
        &mut output,
    )?;
    output.sync_all()?;
    drop(output);
    verify_download(
        &staging,
        review["size"]
            .as_u64()
            .context("A revisão do JAR não contém um tamanho válido")?,
        expected,
    )?;
    ensure!(!target.exists(), "O destino foi ocupado");
    fs::rename(&staging, &target)?;
    Ok(review)
}
fn identify(directory: &Path, name: &str, release: Version, info: Value) -> Result<Plan> {
    let cfg = Config::load(directory)?;
    let path = source(directory, name)?;
    let hash = digest(&path)?;
    identifier(&release.id)?;
    identifier(&release.project_id)?;
    ensure!(
        info["id"].as_str() == Some(&release.project_id),
        "Projeto incorreto"
    );
    ensure!(
        compatible_with(&release, &cfg.minecraft, cfg.software.loader()),
        "O JAR não declara uma release estável para este Minecraft/software"
    );
    let file = release
        .files
        .iter()
        .find(|f| {
            f.hashes["sha512"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(&hash))
        })
        .context("O hash não corresponde aos ficheiros da versão")?;
    ensure!(
        file.size == fs::metadata(&path)?.len(),
        "O tamanho do JAR não corresponde ao catálogo"
    );
    ensure!(
        !load(directory)?
            .iter()
            .any(|r| r.project_id == release.project_id),
        "Este projeto já está gerido; resolve a cópia duplicada antes de associar"
    );
    ensure!(
        !plugin_dir(directory)?
            .join(format!("nethergate-{}.jar", release.project_id))
            .exists(),
        "O destino gerido já existe"
    );
    let server_side = info["server_side"].as_str().unwrap_or("unknown");
    ensure!(
        cfg.software != Software::Fabric || ["required", "optional"].contains(&server_side),
        "O autor não declara suporte para servidor dedicado"
    );
    let mut plan = Plan {
        missing_projects: vec![],
        software: cfg.software,
        distribution: Distribution {
            required_projects: release
                .dependencies
                .iter()
                .filter(|d| d.dependency_type == "required")
                .filter_map(|d| d.project_id.clone())
                .collect(),
            url: file.url.clone(),
            sha1: file.hashes["sha1"].as_str().unwrap_or("").into(),
            size: file.size,
            client_side: info["client_side"].as_str().unwrap_or("unknown").into(),
            server_side: server_side.into(),
        },
        project_id: release.project_id,
        title: info["title"]
            .as_str()
            .unwrap_or(name)
            .chars()
            .take(120)
            .collect(),
        version_id: release.id,
        version_number: release.version_number,
        minecraft: cfg.minecraft,
        source_filename: name.into(),
        size: file.size,
        sha512: hash,
        url: file.url.clone(),
        previous: None,
        blockers: vec![],
        fingerprint: String::new(),
    };
    // Existing download validation also constrains the catalogue URL and hash.
    let candidate = Version {
        id: plan.version_id.clone(),
        project_id: plan.project_id.clone(),
        version_number: plan.version_number.clone(),
        version_type: "release".into(),
        game_versions: vec![plan.minecraft.clone()],
        loaders: vec![cfg.software.loader().into()],
        files: vec![file.clone()],
        dependencies: vec![],
    };
    primary(&candidate)?;
    plan.fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &json!({"plan":plan,"inventory":inventory(directory)?})
        )?)
    );
    Ok(plan)
}
pub fn preview(directory: &Path, name: &str, consent: bool) -> Result<Plan> {
    ensure!(
        consent,
        "Autoriza a consulta: só o hash SHA-512 é enviado ao Modrinth"
    );
    let path = source(directory, name)?;
    let hash = digest(&path)?;
    let http = client()?;
    let release: Version = serde_json::from_value(get(
        &http,
        &format!("version_file/{hash}"),
        &[("algorithm", "sha512".into())],
    )?)
    .context("O Modrinth não reconheceu este JAR")?;
    let info = get(&http, &format!("project/{}", release.project_id), &[])?;
    identify(directory, name, release, info)
}
fn adopt_plan(directory: &Path, name: &str, plan: &Plan) -> Result<Value> {
    ensure_no_pending(directory)?;
    let source = source(directory, name)?;
    ensure!(
        digest(&source)? == plan.sha512,
        "O JAR mudou desde a revisão"
    );
    let records = load(directory)?;
    ensure!(
        !records.iter().any(|r| r.project_id == plan.project_id),
        "Projeto já gerido"
    );
    let snapshot = secure_folder(
        directory,
        &[".nethergate-manual-history", &crate::transfer::key()?],
    )?;
    let target = plugin_dir(directory)?.join(format!("nethergate-{}.jar", plan.project_id));
    ensure!(!target.exists(), "O destino já existe");
    config::write_json(
        &directory.join(PENDING_MANUAL),
        &json!({"source":source,"target":target,"snapshot":snapshot}),
    )?;
    let mut moved = false;
    let mut copied = false;
    let result = (|| -> Result<()> {
        fs::rename(&source, snapshot.join("original.jar"))?;
        moved = true;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        copied = true;
        std::io::copy(
            &mut fs::File::open(snapshot.join("original.jar"))?,
            &mut out,
        )?;
        out.sync_all()?;
        drop(out);
        verify_download(&target, plan.size, &plan.sha512)?;
        let mut next = records.clone();
        next.push(Installed {
            distribution: plan.distribution.clone(),
            project_id: plan.project_id.clone(),
            title: plan.title.clone(),
            version_id: plan.version_id.clone(),
            version_number: plan.version_number.clone(),
            filename: target
                .file_name()
                .context("O destino do JAR não contém um nome de ficheiro")?
                .to_string_lossy()
                .into(),
            sha512: plan.sha512.clone(),
            minecraft: plan.minecraft.clone(),
        });
        crate::hub::replace_json(&directory.join(registry(directory)?), &next)?;
        Ok(())
    })();
    if let Err(e) = result {
        let rollback = (|| -> Result<()> {
            if copied {
                fs::rename(&target, snapshot.join("failed-copy.jar"))?;
            }
            if moved {
                fs::rename(snapshot.join("original.jar"), &source)?;
            }
            fs::remove_file(directory.join(PENDING_MANUAL))?;
            Ok(())
        })();
        if let Err(r) = rollback {
            bail!(
                "Associação interrompida: {e:#}; recuperação: {r:#}. Cópia: {}",
                snapshot.display()
            );
        }
        return Err(e);
    }
    fs::remove_file(directory.join(PENDING_MANUAL))?;
    Ok(
        json!({"snapshot":snapshot,"projectId":plan.project_id,"filename":target.file_name().context("O destino do JAR não contém um nome de ficheiro")?}),
    )
}
pub fn adopt(directory: &Path, name: &str, expected: &str, consent: bool) -> Result<Value> {
    let plan = preview(directory, name, consent)?;
    ensure!(
        plan.fingerprint == expected,
        "O plano mudou; identifica novamente o JAR"
    );
    adopt_plan(directory, name, &plan)
}
pub fn disable(directory: &Path, name: &str, expected: &str) -> Result<Value> {
    ensure_no_pending(directory)?;
    let source = source(directory, name)?;
    let hash = digest(&source)?;
    ensure!(hash == expected, "O JAR mudou desde a revisão");
    let folder = disabled_dir(directory)?;
    let id = crate::transfer::key()?;
    let target = folder.join(format!("{id}.jar"));
    let mut records = disabled(directory)?;
    records.push(Disabled {
        id: id.clone(),
        filename: name.into(),
        sha512: hash,
        size: fs::metadata(&source)?.len(),
    });
    config::write_json(
        &directory.join(PENDING_MANUAL),
        &json!({"source":source,"target":target,"operation":"disable"}),
    )?;
    if let Err(e) = fs::rename(&source, &target) {
        fs::remove_file(directory.join(PENDING_MANUAL))?;
        return Err(e.into());
    }
    if let Err(e) = crate::hub::replace_json(&folder.join("index.json"), &records) {
        fs::rename(&target, &source).context("Recuperação pendente no diário")?;
        fs::remove_file(directory.join(PENDING_MANUAL))?;
        return Err(e);
    }
    fs::remove_file(directory.join(PENDING_MANUAL))?;
    Ok(json!({"id":id,"disabled":true}))
}
pub fn enable(directory: &Path, id: &str) -> Result<Value> {
    ensure_no_pending(directory)?;
    identifier(id)?;
    let mut records = disabled(directory)?;
    let record = records
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .context("JAR desativado não encontrado")?;
    let folder = disabled_dir(directory)?;
    let source = folder.join(format!("{id}.jar"));
    backups::ordinary(&source)?;
    verify_download(&source, record.size, &record.sha512)?;
    let active = plugin_dir(directory)?;
    fs::create_dir_all(&active)?;
    let target = active.join(&record.filename);
    ensure!(
        !target.exists(),
        "Já existe um ficheiro com o nome original"
    );
    for entry in fs::read_dir(&active)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".jar")
        {
            backups::ordinary(&entry.path())?;
            ensure!(
                digest(&entry.path())? != record.sha512,
                "Já existe uma cópia deste JAR ativa com outro nome"
            );
        }
    }
    config::write_json(
        &directory.join(PENDING_MANUAL),
        &json!({"source":source,"target":target,"operation":"enable"}),
    )?;
    if let Err(e) = fs::rename(&source, &target) {
        fs::remove_file(directory.join(PENDING_MANUAL))?;
        return Err(e.into());
    }
    records.retain(|r| r.id != id);
    if let Err(e) = crate::hub::replace_json(&folder.join("index.json"), &records) {
        fs::rename(&target, &source).context("Recuperação pendente no diário")?;
        fs::remove_file(directory.join(PENDING_MANUAL))?;
        return Err(e);
    }
    fs::remove_file(directory.join(PENDING_MANUAL))?;
    Ok(json!({"enabled":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ng-manual-{}", crate::transfer::key().unwrap()));
        fs::create_dir_all(root.join("plugins")).unwrap();
        config::write_json(
            &root.join("nethergate.json"),
            &Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap(),
        )
        .unwrap();
        fs::write(root.join("plugins/manual.jar"), b"PK\x03\x04data").unwrap();
        root
    }
    fn plan(root: &Path) -> Plan {
        let hash = digest(&root.join("plugins/manual.jar")).unwrap();
        let release:Version=serde_json::from_value(json!({"id":"version1","project_id":"project1","version_number":"1.0","version_type":"release","game_versions":["1.21.11"],"loaders":["paper"],"dependencies":[],"files":[{"filename":"manual.jar","url":"https://cdn.modrinth.com/data/project1/versions/version1/manual.jar","size":8,"primary":true,"hashes":{"sha512":hash}}]})).unwrap();
        identify(
            root,
            "manual.jar",
            release,
            json!({"id":"project1","title":"Fixture","server_side":"required"}),
        )
        .unwrap()
    }
    #[test]
    fn reversible_disable_enable_and_import_reject_stale_or_duplicate_files() {
        let root = fixture();
        let hash = digest(&root.join("plugins/manual.jar")).unwrap();
        assert!(disable(&root, "manual.jar", "stale").is_err());
        let v = disable(&root, "manual.jar", &hash).unwrap();
        assert!(!root.join("plugins/manual.jar").exists());
        assert_eq!(
            list(&root).unwrap()["disabled"].as_array().unwrap().len(),
            1
        );
        enable(&root, v["id"].as_str().unwrap()).unwrap();
        assert_eq!(digest(&root.join("plugins/manual.jar")).unwrap(), hash);
        let external = root.join("external.jar");
        fs::write(&external, b"PK\x03\x04other").unwrap();
        let review = file_preview(&external).unwrap();
        add(&root, &external, review["sha512"].as_str().unwrap()).unwrap();
        assert!(external.exists());
        assert!(add(&root, &external, review["sha512"].as_str().unwrap()).is_err());
        assert!(source(&root, "../escape.jar").is_err());
        assert!(preview(&root, "manual.jar", false).is_err());
    }
    #[test]
    fn adoption_moves_one_active_copy_and_rolls_back_registry_errors() {
        let root = fixture();
        let p = plan(&root);
        fs::create_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        assert!(adopt_plan(&root, "manual.jar", &p).is_err());
        assert!(root.join("plugins/manual.jar").exists());
        assert!(!root.join("plugins/nethergate-project1.jar").exists());
        assert!(!root.join(PENDING_MANUAL).exists());
        fs::remove_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        adopt_plan(&root, "manual.jar", &p).unwrap();
        assert!(!root.join("plugins/manual.jar").exists());
        let records = load(&root).unwrap();
        assert_eq!(records.len(), 1);
        assert!(intact(&root, &records[0]));
        assert!(list(&root).unwrap()["files"].as_array().unwrap().is_empty());
    }
}
