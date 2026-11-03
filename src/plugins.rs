//! Reviewed, offline Modrinth Paper plugin installs with preserved rollback material.
pub mod manual;
pub mod solver;
use crate::{
    backups,
    config::{Config, Software},
};
use anyhow::{bail, ensure, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha512};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(test)]
const REGISTRY: &str = "nethergate-plugins.json";
const PENDING: &str = ".nethergate-plugin-change.json";
fn software(directory: &Path) -> Result<Software> {
    if directory.join("nethergate.json").exists() {
        Ok(Config::load(directory)?.software)
    } else {
        Ok(Software::Paper)
    }
}
fn registry(directory: &Path) -> Result<&'static str> {
    Ok(software(directory)?.registry())
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Distribution {
    #[serde(default)]
    pub required_projects: Vec<String>,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    pub client_side: String,
    pub server_side: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Installed {
    #[serde(default)]
    pub distribution: Distribution,
    pub project_id: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    pub sha512: String,
    pub minecraft: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Version {
    id: String,
    project_id: String,
    version_number: String,
    version_type: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<Download>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Download {
    url: String,
    filename: String,
    primary: bool,
    size: u64,
    hashes: Value,
}
#[derive(Clone, Serialize, Deserialize)]
struct Dependency {
    project_id: Option<String>,
    version_id: Option<String>,
    file_name: Option<String>,
    dependency_type: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub missing_projects: Vec<String>,
    pub software: Software,
    pub distribution: Distribution,
    pub project_id: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub minecraft: String,
    pub source_filename: String,
    pub size: u64,
    pub sha512: String,
    pub url: String,
    pub previous: Option<Installed>,
    pub blockers: Vec<String>,
    pub fingerprint: String,
}

fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c)),
        "Identificador de plugin inválido"
    );
    Ok(())
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!(
            "Nethergate/",
            env!("CARGO_PKG_VERSION"),
            " plugin manager"
        ))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn get(client: &Client, path: &str, query: &[(&str, String)]) -> Result<Value> {
    let response = client
        .get(format!("https://api.modrinth.com/v2/{path}"))
        .query(query)
        .timeout(Duration::from_secs(25))
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "Resposta Modrinth demasiado grande"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha512::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn verify_download(path: &Path, size: u64, hash: &str) -> Result<()> {
    ensure!(
        fs::metadata(path)?.len() == size && digest(path)?.eq_ignore_ascii_case(hash),
        "Falha de integridade no download do plugin (SHA-512/tamanho)"
    );
    let mut signature = [0u8; 4];
    fs::File::open(path)?.read_exact(&mut signature)?;
    ensure!(
        signature == [0x50, 0x4b, 3, 4],
        "O ficheiro descarregado não é um JAR reconhecido"
    );
    Ok(())
}
fn secure_folder(root: &Path, parts: &[&str]) -> Result<PathBuf> {
    let mut folder = root.to_path_buf();
    backups::ordinary(&folder)?;
    for part in parts {
        backups::component(part)?;
        folder = folder.join(part);
        if !folder.exists() {
            fs::create_dir(&folder)?;
        }
        ensure!(
            backups::ordinary(&folder)?.is_dir(),
            "Destino de cópia de plugins inválido"
        );
    }
    ensure!(
        folder.canonicalize()?.starts_with(root.canonicalize()?),
        "Destino fora do projeto"
    );
    Ok(folder)
}
fn load(directory: &Path) -> Result<Vec<Installed>> {
    let path = directory.join(registry(directory)?);
    if !path.exists() {
        return Ok(vec![]);
    }
    backups::ordinary(&path)?;
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() < 1024 * 1024,
        "Registo de plugins demasiado grande"
    );
    let records: Vec<Installed> = serde_json::from_slice(&bytes)?;
    let mut seen = std::collections::HashSet::new();
    for r in &records {
        identifier(&r.project_id)?;
        identifier(&r.version_id)?;
        ensure!(
            r.filename == format!("nethergate-{}.jar", r.project_id)
                && seen.insert(&r.project_id)
                && r.sha512.len() == 128
                && r.sha512.bytes().all(|c| c.is_ascii_hexdigit()),
            "Registo de plugins inválido"
        );
    }
    Ok(records)
}
fn plugin_dir(directory: &Path) -> Result<PathBuf> {
    let path = directory.join(software(directory)?.folder());
    if path.exists() {
        ensure!(
            backups::ordinary(&path)?.is_dir(),
            "A pasta plugins não é uma pasta normal"
        );
    }
    Ok(path)
}
fn intact(directory: &Path, r: &Installed) -> bool {
    plugin_dir(directory)
        .and_then(|p| {
            let p = p.join(&r.filename);
            backups::ordinary(&p)?;
            Ok(digest(&p)?.eq_ignore_ascii_case(&r.sha512))
        })
        .unwrap_or(false)
}
pub fn ensure_no_pending(directory: &Path) -> Result<()> {
    ensure!(!directory.join(".nethergate-manual-operation.json").exists(),"Há uma operação de JAR manual interrompida; consulta o diário antes de iniciar ou alterar ficheiros");
    ensure!(!directory.join(PENDING).exists(),"Há uma alteração de plugins interrompida. O diário .nethergate-plugin-change.json identifica a cópia anterior; recupera-a antes de iniciar ou alterar plugins");
    Ok(())
}
pub fn inventory(directory: &Path) -> Result<Value> {
    let records = load(directory)?;
    let folder = plugin_dir(directory)?;
    let mut external = vec![];
    if folder.exists() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".jar")
                && !records
                    .iter()
                    .any(|r| r.filename.eq_ignore_ascii_case(&name))
            {
                external.push(name)
            }
        }
    }
    external.sort();
    Ok(
        json!({"installed":records.iter().map(|r|json!({"plugin":r,"intact":intact(directory,r)})).collect::<Vec<_>>(),"external":external,"pending":directory.join(PENDING).exists()}),
    )
}
pub fn search(directory: &Path, query: &str) -> Result<Value> {
    ensure!(
        query.trim().chars().count() <= 100,
        "Pesquisa demasiado longa"
    );
    let cfg = Config::load(directory)?;
    let facets = json!([
        [if cfg.software == Software::Fabric {
            "project_type:mod"
        } else {
            "project_type:plugin"
        }],
        [format!("categories:{}", cfg.software.loader())],
        [format!("versions:{}", cfg.minecraft)]
    ])
    .to_string();
    let result = get(
        &client()?,
        "search",
        &[
            ("query", query.trim().into()),
            ("facets", facets),
            ("limit", "12".into()),
        ],
    )?;
    let hits = result["hits"]
        .as_array()
        .context("Resposta Modrinth sem resultados")?;
    Ok(
        json!({"hits":hits.iter().map(|h|json!({"projectId":h["project_id"],"title":h["title"],"description":h["description"],"author":h["author"],"downloads":h["downloads"]})).collect::<Vec<_>>(),"minecraft":cfg.minecraft}),
    )
}
fn compatible_with(version: &Version, minecraft: &str, loader: &str) -> bool {
    version.version_type == "release"
        && version.game_versions.iter().any(|v| v == minecraft)
        && version.loaders.iter().any(|l| l == loader)
}
#[cfg(test)]
fn compatible(version: &Version, minecraft: &str) -> bool {
    compatible_with(version, minecraft, "paper")
}
fn primary(version: &Version) -> Result<&Download> {
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .context("Versão sem ficheiro")?;
    backups::component(&file.filename)?;
    let url = reqwest::Url::parse(&file.url)?;
    let hash = file.hashes["sha512"]
        .as_str()
        .context("Download sem SHA-512")?;
    ensure!(
        file.filename.to_ascii_lowercase().ends_with(".jar")
            && url.scheme() == "https"
            && url.host_str() == Some("cdn.modrinth.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .path()
                .starts_with(&format!("/data/{}/", version.project_id))
            && url.path().ends_with(".jar")
            && file.size > 0
            && file.size <= 128 * 1024 * 1024
            && hash.len() == 128
            && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Download de plugin inválido"
    );
    Ok(file)
}
pub fn preview(directory: &Path, project: &str, version: Option<&str>) -> Result<Plan> {
    identifier(project)?;
    let cfg = Config::load(directory)?;
    let http = client()?;
    let info = get(&http, &format!("project/{project}"), &[])?;
    // v2 reports some mixed mod/plugin projects as "mod". The version's Paper
    // loader, exact game version and JAR file remain mandatory below.
    ensure!(
        (cfg.software == Software::Paper
            && matches!(info["project_type"].as_str(), Some("plugin" | "mod")))
            || (cfg.software == Software::Fabric && info["project_type"] == "mod"),
        "O projeto não disponibiliza plugins"
    );
    let project = info["id"].as_str().context("Projeto sem ID")?;
    identifier(project)?;
    let release: Version = if let Some(version) = version {
        identifier(version)?;
        serde_json::from_value(get(&http, &format!("version/{version}"), &[])?)?
    } else {
        let mut versions: Vec<Version> = serde_json::from_value(get(
            &http,
            &format!("project/{project}/version"),
            &[
                ("loaders", json!([cfg.software.loader()]).to_string()),
                ("game_versions", json!([cfg.minecraft]).to_string()),
            ],
        )?)?;
        // Modrinth lists versions newest first; filter again rather than trusting query facets.
        versions.retain(|v| compatible_with(v, &cfg.minecraft, cfg.software.loader()));
        versions.into_iter().next().context(
            "Não existe uma versão estável que declare suporte para este Paper/Minecraft",
        )?
    };
    ensure!(
        release.project_id == project
            && compatible_with(&release, &cfg.minecraft, cfg.software.loader()),
        "A versão não corresponde ao projeto ou ao Paper/Minecraft configurado"
    );
    identifier(&release.id)?;
    let file = primary(&release)?;
    let installed = load(directory)?;
    let previous = installed.iter().find(|r| r.project_id == project).cloned();
    let mut blockers = vec![];
    let mut distribution = Distribution {
        required_projects: vec![],
        url: file.url.clone(),
        sha1: file.hashes["sha1"].as_str().unwrap_or("").into(),
        size: file.size,
        client_side: info["client_side"].as_str().unwrap_or("unknown").into(),
        server_side: info["server_side"].as_str().unwrap_or("unknown").into(),
    };
    if cfg.software == Software::Fabric
        && !["required", "optional"].contains(&distribution.server_side.as_str())
    {
        blockers.push("O autor não declara suporte para servidor dedicado; este mod não será instalado no servidor".into());
    }
    if let Some(old) = &previous {
        if !intact(directory, old) {
            blockers.push("O JAR gerido está ausente ou foi alterado fora da aplicação; revê-o antes de atualizar".into())
        }
        if old.version_id == release.id {
            blockers.push("Esta versão já está instalada".into())
        }
    }
    let folder = plugin_dir(directory)?;
    if previous.is_none() && folder.join(format!("nethergate-{project}.jar")).exists() {
        blockers.push("Já existe um JAR nesse destino, fora do registo do gestor".into());
    }
    if folder.join(&file.filename).exists()
        && previous
            .as_ref()
            .is_none_or(|r| r.filename != file.filename)
    {
        blockers.push(
            "Já existe um JAR manual com o nome deste download. Evita instalar uma cópia duplicada"
                .into(),
        );
    }
    let mut missing_projects = vec![];
    ensure!(
        release.dependencies.len() <= 64,
        "Esta versão declara demasiadas dependências"
    );
    for dep in &release.dependencies {
        let matches = |r: &&Installed| {
            (dep.project_id.is_some() || dep.version_id.is_some())
                && dep.project_id.as_ref().is_none_or(|id| id == &r.project_id)
                && dep.version_id.as_ref().is_none_or(|id| id == &r.version_id)
        };
        let found = installed.iter().find(matches);
        if dep.dependency_type == "required" {
            if let Some(project) = dep
                .project_id
                .as_ref()
                .or_else(|| found.map(|r| &r.project_id))
            {
                identifier(project)?;
                distribution.required_projects.push(project.clone());
            }
        }
        let satisfied = found.is_some_and(|r| {
            dep.version_id.as_ref().is_none_or(|v| v == &r.version_id) && intact(directory, r)
        });
        let name = dep
            .project_id
            .as_deref()
            .or(dep.version_id.as_deref())
            .or(dep.file_name.as_deref())
            .unwrap_or("externa");
        if dep.dependency_type == "required" && !satisfied {
            if let Some(id) = &dep.project_id {
                missing_projects.push(id.clone());
            }
            blockers.push(format!("Instala primeiro a dependência obrigatória {name}. Dependências externas não são instaladas automaticamente"));
        }
        if dep.dependency_type == "incompatible" && found.is_some() {
            blockers.push(format!("Incompatível com o plugin instalado {name}"));
        }
    }
    let mut plan = Plan {
        missing_projects,
        software: cfg.software,
        distribution,
        project_id: project.into(),
        title: info["title"]
            .as_str()
            .unwrap_or(project)
            .chars()
            .take(120)
            .collect(),
        version_id: release.id.clone(),
        version_number: release.version_number.clone(),
        minecraft: cfg.minecraft,
        source_filename: file.filename.clone(),
        size: file.size,
        sha512: file.hashes["sha512"]
            .as_str()
            .context("A versão selecionada não contém um hash SHA-512 válido")?
            .to_ascii_lowercase(),
        url: file.url.clone(),
        previous,
        blockers,
        fingerprint: String::new(),
    };
    plan.fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&plan)?));
    Ok(plan)
}
fn copy_tree(source: &Path, destination: &Path, total: &mut u64) -> Result<()> {
    let metadata = backups::ordinary(source)?;
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            backups::component(name.to_str().context("Nome de plugin inválido")?)?;
            copy_tree(&entry.path(), &destination.join(name), total)?;
        }
    } else {
        *total = total.saturating_add(metadata.len());
        ensure!(
            *total <= 2 * 1024 * 1024 * 1024,
            "A pasta de plugins excede 2 GB; cria uma cópia externa antes de gerir este perfil"
        );
        let hash = digest(source)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        std::io::copy(&mut fs::File::open(source)?, &mut out)?;
        out.sync_all()?;
        ensure!(
            digest(destination)? == hash && digest(source)? == hash,
            "O plugin mudou durante o backup"
        );
    }
    Ok(())
}
fn commit(directory: &Path, download: &Path, snapshot: &Path, plan: &Plan) -> Result<()> {
    ensure_no_pending(directory)?;
    let folder = plugin_dir(directory)?;
    fs::create_dir_all(&folder)?;
    let target = folder.join(format!("nethergate-{}.jar", plan.project_id));
    let old = snapshot.join("previous-live.jar");
    if target.exists() {
        ensure!(
            plan.previous.as_ref().is_some_and(|r| intact(directory, r)),
            "Já existe um JAR neste destino; não será substituído"
        );
    }
    let mut records = load(directory)?;
    let previous_records = serde_json::to_vec(&records)?;
    crate::config::write_json(
        &directory.join(PENDING),
        &json!({"target":target,"snapshot":snapshot,"project":plan.project_id}),
    )?;
    let mut moved = false;
    let mut placed = false;
    let result = (|| -> Result<()> {
        if target.exists() {
            fs::rename(&target, &old)?;
            moved = true;
        }
        fs::rename(download, &target)?;
        placed = true;
        records.retain(|r| r.project_id != plan.project_id);
        records.push(Installed {
            distribution: plan.distribution.clone(),
            project_id: plan.project_id.clone(),
            title: plan.title.clone(),
            version_id: plan.version_id.clone(),
            version_number: plan.version_number.clone(),
            filename: format!("nethergate-{}.jar", plan.project_id),
            sha512: plan.sha512.clone(),
            minecraft: plan.minecraft.clone(),
        });
        crate::hub::replace_json(&directory.join(registry(directory)?), &records)?;
        Ok(())
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            if placed {
                ensure!(
                    digest(&target)? == plan.sha512,
                    "O destino mudou durante a recuperação"
                );
                fs::remove_file(&target)?;
            }
            if moved {
                fs::rename(&old, &target)?;
            }
            ensure!(
                serde_json::to_vec(&load(directory)?)? == previous_records,
                "O registo de plugins mudou durante a recuperação"
            );
            fs::remove_file(directory.join(PENDING))?;
            Ok(())
        })();
        if let Err(rollback) = rollback {
            bail!("A alteração falhou: {error:#}. Recuperação pendente: {rollback:#}. Cópia preservada em {}",snapshot.display())
        }
        return Err(error);
    }
    fs::remove_file(directory.join(PENDING))
        .context("Plugin instalado, mas falta concluir o diário da operação")?;
    Ok(())
}
pub fn install(
    root: &Path,
    server_id: &str,
    directory: &Path,
    project: &str,
    version: &str,
    expected: &str,
) -> Result<Value> {
    identifier(server_id)?;
    backups::ensure_no_pending(directory)?;
    ensure_no_pending(directory)?;
    let plan = preview(directory, project, Some(version))?;
    ensure!(
        plan.fingerprint == expected,
        "O plano de instalação mudou. Revê novamente os detalhes antes de confirmar"
    );
    ensure!(plan.blockers.is_empty(), "{}", plan.blockers.join("; "));
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("{e}"))?;
    let operation: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let cfg = Config::load(directory)?;
    let changes = if cfg.software == Software::Fabric {
        "mod-changes"
    } else {
        "plugin-changes"
    };
    let snapshot = secure_folder(root, &["hub-data", changes, server_id, &operation])?;
    let download = snapshot.join("download.jar.part");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&download)?;
    let response = client()?.get(&plan.url).send()?.error_for_status()?;
    let size = std::io::copy(&mut response.take(plan.size + 1), &mut output)?;
    output.flush()?;
    output.sync_all()?;
    drop(output);
    ensure!(
        size == plan.size && digest(&download)?.eq_ignore_ascii_case(&plan.sha512),
        "Falha de integridade no download do plugin (SHA-512/tamanho)"
    );
    verify_download(&download, plan.size, &plan.sha512)?;
    let folder = plugin_dir(directory)?;
    if folder.exists() {
        copy_tree(&folder, &snapshot.join(cfg.software.folder()), &mut 0)?;
    }
    if cfg.software == Software::Fabric && directory.join("config").exists() {
        copy_tree(&directory.join("config"), &snapshot.join("config"), &mut 0)?;
    }
    crate::config::write_json(&snapshot.join(registry(directory)?), &load(directory)?)?;
    let ready = backups::world_ready(directory)?;
    ensure!(
        ready || !directory.join(backups::primary_world(directory)?).exists(),
        "O mundo existe mas não tem level.dat. Resolve-o antes de instalar plugins"
    );
    let world = if ready {
        Some(backups::create(
            &crate::backup_destination::resolve(root, server_id)?,
            server_id,
            directory,
            if cfg.software == Software::Fabric {
                "before_mods"
            } else {
                "before_plugins"
            },
        )?)
    } else {
        None
    };
    crate::config::write_json(
        &snapshot.join("snapshot.json"),
        &json!({"plan":plan,"worldBackup":world}),
    )?;
    commit(directory, &download, &snapshot, &plan)?;
    Ok(
        json!({"installed":plan.title,"version":plan.version_number,"snapshot":snapshot,"worldBackup":world,"restartRequired":true}),
    )
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !crc
}
fn pack_zip(index: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        index.len() <= 1024 * 1024,
        "Índice do pacote demasiado grande"
    );
    let name = b"modrinth.index.json";
    let crc = crc32(index);
    let size = index.len() as u32;
    let mut out = vec![];
    out.extend(0x04034b50u32.to_le_bytes());
    for value in [20u16, 0x800, 0, 0, 33] {
        out.extend(value.to_le_bytes());
    }
    for value in [crc, size, size] {
        out.extend(value.to_le_bytes());
    }
    out.extend((name.len() as u16).to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(name);
    out.extend(index);
    let offset = out.len() as u32;
    out.extend(0x02014b50u32.to_le_bytes());
    for value in [20u16, 20, 0x800, 0, 0, 33] {
        out.extend(value.to_le_bytes());
    }
    for value in [crc, size, size] {
        out.extend(value.to_le_bytes());
    }
    for value in [name.len() as u16, 0, 0, 0, 0] {
        out.extend(value.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(name);
    let central = out.len() as u32 - offset;
    out.extend(0x06054b50u32.to_le_bytes());
    for value in [0u16, 0, 1, 1] {
        out.extend(value.to_le_bytes());
    }
    out.extend(central.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    Ok(out)
}
pub fn export_pack(root: &Path, id: &str, directory: &Path, name: &str) -> Result<Value> {
    identifier(id)?;
    ensure_no_pending(directory)?;
    let cfg = Config::load(directory)?;
    ensure!(
        cfg.software == Software::Fabric,
        "Pacotes de mods exigem um servidor Fabric"
    );
    let fabric = crate::fabric::verify(directory, &cfg)?;
    let inventory = inventory(directory)?;
    ensure!(
        inventory["external"]
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "Há JARs na pasta mods fora do gestor. O pacote não pode ser apresentado como completo"
    );
    let mut files = vec![];
    let mut server_only = 0;
    let records = load(directory)?;
    let mut required = records
        .iter()
        .filter(|r| r.distribution.client_side == "required")
        .map(|r| r.project_id.clone())
        .collect::<std::collections::HashSet<_>>();
    loop {
        let before = required.len();
        for record in &records {
            if required.contains(&record.project_id) {
                for dependency in &record.distribution.required_projects {
                    if records.iter().any(|r| {
                        &r.project_id == dependency && r.distribution.client_side != "unsupported"
                    }) {
                        required.insert(dependency.clone());
                    }
                }
            }
        }
        if required.len() == before {
            break;
        }
    }
    for record in records {
        ensure!(
            intact(directory, &record),
            "O mod {} está alterado ou ausente",
            record.title
        );
        if record.distribution.client_side == "unsupported" {
            server_only += 1;
            continue;
        }
        let d = &record.distribution;
        ensure!(
            ["required", "optional"].contains(&d.client_side.as_str())
                && ["required", "optional"].contains(&d.server_side.as_str()),
            "O autor não definiu o ambiente do mod {}; confirma-o antes de exportar",
            record.title
        );
        let url = reqwest::Url::parse(&d.url)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("cdn.modrinth.com")
                && d.sha1.len() == 40
                && d.sha1.bytes().all(|c| c.is_ascii_hexdigit())
                && d.size > 0,
            "Faltam metadados de distribuição válidos para {}",
            record.title
        );
        files.push(json!({"path":format!("mods/{}",record.filename),"hashes":{"sha1":d.sha1,"sha512":record.sha512},"env":{"client":if required.contains(&record.project_id){"required"}else{d.client_side.as_str()},"server":d.server_side},"downloads":[d.url],"fileSize":d.size}));
    }
    let stamp = crate::backup_schedule::now()?.to_string();
    let index = json!({"formatVersion":1,"game":"minecraft","versionId":stamp,"name":name.chars().take(80).collect::<String>(),"summary":"Mods de cliente correspondentes ao servidor Nethergate; não inclui configurações privadas.","dependencies":{"minecraft":cfg.minecraft,"fabric-loader":fabric.loader},"files":files});
    let folder = secure_folder(root, &["hub-data", "exports", id])?;
    let path = folder.join(format!("nethergate-{stamp}.mrpack"));
    let bytes = pack_zip(&serde_json::to_vec_pretty(&index)?)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(
        json!({"path":path,"clientMods":files.len(),"serverOnlyExcluded":server_only,"minecraft":cfg.minecraft,"loader":fabric.loader}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_pack_excludes_server_only_and_requires_shared_dependencies() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let root = root();
        let mut cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
        cfg.software = Software::Fabric;
        crate::config::write_json(&root.join("nethergate.json"), &cfg).unwrap();
        fs::write(root.join(cfg.software.jar()), b"fixture launcher").unwrap();
        crate::config::write_json(
            &root.join("fabric.lock.json"),
            &crate::fabric::Installed {
                minecraft: cfg.minecraft.clone(),
                loader: "0.20.1".into(),
                installer: "1.1.2".into(),
                sha256: crate::paper::hash_file(&root.join(cfg.software.jar())).unwrap(),
            },
        )
        .unwrap();
        fs::create_dir(root.join("mods")).unwrap();
        let mut records = vec![];
        for (id, side) in [
            ("main0001", "required"),
            ("lib00001", "optional"),
            ("server01", "unsupported"),
        ] {
            let filename = format!("nethergate-{id}.jar");
            let file = root.join("mods").join(&filename);
            fs::write(&file, b"fixture mod").unwrap();
            records.push(Installed {
                project_id: id.into(),
                title: id.into(),
                version_id: format!("v{id}"),
                version_number: "1".into(),
                filename,
                sha512: digest(&file).unwrap(),
                minecraft: cfg.minecraft.clone(),
                distribution: Distribution {
                    url: format!("https://cdn.modrinth.com/data/{id}/file.jar"),
                    sha1: "a".repeat(40),
                    size: 11,
                    client_side: side.into(),
                    server_side: "required".into(),
                    required_projects: if id == "main0001" {
                        vec!["lib00001".into()]
                    } else {
                        vec![]
                    },
                },
            });
        }
        crate::config::write_json(&root.join("nethergate-mods.json"), &records).unwrap();
        let result = export_pack(&root, "fixture", &root, "Fixture").unwrap();
        assert_eq!(result["clientMods"], 2);
        assert_eq!(result["serverOnlyExcluded"], 1);
        let zip = fs::read(result["path"].as_str().unwrap()).unwrap();
        let len = u32::from_le_bytes(zip[18..22].try_into().unwrap()) as usize;
        let start = 30 + b"modrinth.index.json".len();
        let index: Value = serde_json::from_slice(&zip[start..start + len]).unwrap();
        assert!(index["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["env"]["client"] == "required"));
        fs::write(root.join("mods/unknown.jar"), b"external").unwrap();
        assert!(export_pack(&root, "fixture", &root, "Fixture").is_err());
    }
    fn root() -> PathBuf {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).unwrap();
        let name: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let path = std::env::temp_dir().join(format!("nethergate-plugins-{name}"));
        fs::create_dir(&path).unwrap();
        path
    }
    fn plan(hash: String) -> Plan {
        Plan {
            missing_projects: vec![],
            software: Software::Paper,
            distribution: Distribution::default(),
            project_id: "abc12345".into(),
            title: "Fixture".into(),
            version_id: "new12345".into(),
            version_number: "2".into(),
            minecraft: "1.21.11".into(),
            source_filename: "fixture.jar".into(),
            size: 8,
            sha512: hash,
            url: "https://cdn.modrinth.com/data/abc12345/versions/new12345/fixture.jar".into(),
            previous: None,
            blockers: vec![],
            fingerprint: String::new(),
        }
    }
    #[test]
    fn rejects_wrong_platform_paths_and_download_integrity() {
        let mut version:Version=serde_json::from_value(json!({"id":"new12345","project_id":"abc12345","version_number":"1","version_type":"release","game_versions":["1.21.11"],"loaders":["paper"],"files":[{"url":"https://cdn.modrinth.com/data/abc12345/versions/new12345/fixture.jar","filename":"fixture.jar","primary":true,"size":8,"hashes":{"sha512":"a".repeat(128)}}],"dependencies":[]})).unwrap();
        assert!(compatible(&version, "1.21.11"));
        assert!(!compatible(&version, "26.1"));
        assert!(primary(&version).is_ok());
        version.version_type = "beta".into();
        assert!(!compatible(&version, "1.21.11"));
        for url in [
            "http://cdn.modrinth.com/data/abc12345/file.jar",
            "https://example.org/data/abc12345/file.jar",
            "https://cdn.modrinth.com/data/other/file.jar",
        ] {
            version.files[0].url = url.into();
            assert!(primary(&version).is_err());
        }
        assert!(identifier("../path").is_err());
        let root = root();
        let file = root.join("plugin.jar");
        fs::write(&file, b"PK\x03\x04body").unwrap();
        let hash = digest(&file).unwrap();
        assert!(verify_download(&file, 8, &hash).is_ok());
        assert!(verify_download(&file, 9, &hash).is_err());
        assert!(verify_download(&file, 8, &"0".repeat(128)).is_err());
    }
    #[test]
    fn update_commit_preserves_previous_jar_and_recovers_registry_failure() {
        let root = root();
        let folder = root.join("plugins");
        fs::create_dir(&folder).unwrap();
        let target = folder.join("nethergate-abc12345.jar");
        fs::write(&target, b"old plugin").unwrap();
        let old = Installed {
            distribution: Distribution::default(),
            project_id: "abc12345".into(),
            title: "Fixture".into(),
            version_id: "old12345".into(),
            version_number: "1".into(),
            filename: "nethergate-abc12345.jar".into(),
            sha512: digest(&target).unwrap(),
            minecraft: "1.21.11".into(),
        };
        crate::config::write_json(&root.join(REGISTRY), &vec![old.clone()]).unwrap();
        let stage = root.join("snapshot");
        fs::create_dir(&stage).unwrap();
        let download = stage.join("new.part");
        fs::write(&download, b"new plugin").unwrap();
        let mut plan = plan(digest(&download).unwrap());
        plan.previous = Some(old.clone());
        // Force an atomic registry publication failure after the JAR has been replaced.
        fs::create_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        assert!(commit(&root, &download, &stage, &plan).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old plugin");
        assert_eq!(load(&root).unwrap()[0].version_id, old.version_id);
        assert!(!root.join(PENDING).exists());
        fs::remove_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        fs::write(&download, b"new plugin").unwrap();
        commit(&root, &download, &stage, &plan).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new plugin");
        assert_eq!(
            fs::read(stage.join("previous-live.jar")).unwrap(),
            b"old plugin"
        );
        assert!(intact(&root, &load(&root).unwrap()[0]));
    }
    #[test]
    fn refuses_unmanaged_target_and_interrupted_operation() {
        let root = root();
        fs::create_dir(root.join("plugins")).unwrap();
        let target = root.join("plugins/nethergate-abc12345.jar");
        fs::write(&target, b"manual").unwrap();
        let stage = root.join("snapshot");
        fs::create_dir(&stage).unwrap();
        let file = stage.join("new.part");
        fs::write(&file, b"new plugin").unwrap();
        assert!(commit(&root, &file, &stage, &plan(digest(&file).unwrap())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"manual");
        fs::write(root.join(PENDING), "interrupted").unwrap();
        assert!(ensure_no_pending(&root).is_err());
    }
    #[test]
    #[ignore = "Downloads a real Modrinth plugin into a separate fixture; opt-in network integration"]
    fn real_modrinth_install_preserves_world_and_plugin_settings() {
        let root = root();
        let server = root.join("server");
        fs::create_dir(&server).unwrap();
        crate::config::write_json(
            &server.join("nethergate.json"),
            &Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap(),
        )
        .unwrap();
        fs::write(server.join("server.properties"), "server-ip=127.0.0.1\n").unwrap();
        fs::create_dir(server.join("world")).unwrap();
        fs::write(server.join("world/level.dat"), "fixture-world").unwrap();
        fs::create_dir(server.join("plugins")).unwrap();
        fs::write(server.join("plugins/config.yml"), "preserve: true").unwrap();
        let results = search(&server, "Chunky").unwrap();
        assert!(!results["hits"].as_array().unwrap().is_empty());
        let preview = preview(&server, "chunky", None).unwrap();
        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
        let result = install(
            &root,
            "fixture",
            &server,
            &preview.project_id,
            &preview.version_id,
            &preview.fingerprint,
        )
        .unwrap();
        assert!(result["worldBackup"].is_object());
        assert_eq!(inventory(&server).unwrap()["installed"][0]["intact"], true);
        let snapshot = PathBuf::from(result["snapshot"].as_str().unwrap());
        assert_eq!(
            fs::read(snapshot.join("plugins/config.yml")).unwrap(),
            b"preserve: true"
        );
        assert_eq!(
            fs::read(server.join("world/level.dat")).unwrap(),
            b"fixture-world"
        );
        assert_eq!(
            backups::list(&root.join("hub-data/backups/fixture"), "fixture").unwrap()[0].reason,
            "before_plugins"
        );
        println!(
            "Verified Modrinth install + safety copies in {}",
            root.display()
        );
    }
}
