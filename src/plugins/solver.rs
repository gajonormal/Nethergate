//! Whole-set dependency resolution and a single reviewed directory swap.
use super::*;
use crate::config;
use crate::dependency_solver::{self, Release, Requirement};
use std::collections::{BTreeMap, HashSet};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bundle {
    pub projects: Vec<String>,
    pub minecraft: String,
    pub items: Vec<Plan>,
    pub warnings: Vec<String>,
    pub fingerprint: String,
    inventory: String,
}
struct Catalog {
    http: Client,
    minecraft: String,
    software: Software,
    installed: Vec<Installed>,
    requested: HashSet<String>,
    projects: BTreeMap<String, Value>,
    lists: BTreeMap<String, Vec<Version>>,
    versions: BTreeMap<String, Version>,
    calls: usize,
    deadline: std::time::Instant,
}
impl Catalog {
    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        ensure!(
            self.calls < 128 && std::time::Instant::now() < self.deadline,
            "O catálogo excedeu os limites da resolução; tenta um conjunto menor"
        );
        self.calls += 1;
        get(&self.http, path, query)
    }
    fn project(&mut self, id: &str) -> Result<Value> {
        identifier(id)?;
        if let Some(p) = self.projects.get(id) {
            return Ok(p.clone());
        }
        let value = self.get(&format!("project/{id}"), &[])?;
        let canonical = value["id"].as_str().context("Projeto sem ID")?;
        identifier(canonical)?;
        self.projects.insert(canonical.into(), value.clone());
        self.projects.insert(id.into(), value.clone());
        Ok(value)
    }
    fn version(&mut self, id: &str) -> Result<Version> {
        identifier(id)?;
        if let Some(v) = self.versions.get(id) {
            return Ok(v.clone());
        }
        let v: Version = serde_json::from_value(self.get(&format!("version/{id}"), &[])?)?;
        ensure!(v.id == id, "O catálogo devolveu outra versão");
        identifier(&v.project_id)?;
        self.versions.insert(v.id.clone(), v.clone());
        Ok(v)
    }
    fn graph(&mut self, v: &Version) -> Result<Option<Release>> {
        ensure!(
            v.dependencies.len() <= 64,
            "Demasiadas dependências declaradas"
        );
        let mut required = vec![];
        let mut incompatible = vec![];
        for dep in &v.dependencies {
            if !["required", "incompatible"].contains(&dep.dependency_type.as_str()) {
                continue;
            }
            let project = if let Some(id) = &dep.version_id {
                let exact = self.version(id)?;
                if let Some(project) = &dep.project_id {
                    ensure!(
                        project == &exact.project_id,
                        "Projeto e versão da dependência não correspondem"
                    );
                }
                exact.project_id
            } else if let Some(id) = &dep.project_id {
                identifier(id)?;
                id.clone()
            } else {
                return Ok(None);
            };
            let constraint = Requirement {
                project,
                version: dep.version_id.clone(),
            };
            if dep.dependency_type == "required" {
                required.push(constraint);
            } else {
                incompatible.push(constraint);
            }
        }
        Ok(Some(Release {
            project: v.project_id.clone(),
            version: v.id.clone(),
            required,
            incompatible,
        }))
    }
    fn candidates(&mut self, id: &str) -> Result<Vec<Release>> {
        let info = self.project(id)?;
        let kind = info["project_type"].as_str().unwrap_or("");
        if !(kind == "mod" || (kind == "plugin" && self.software == Software::Paper)) {
            return Ok(vec![]);
        }
        if self.software == Software::Fabric
            && !matches!(info["server_side"].as_str(), Some("required" | "optional"))
        {
            return Ok(vec![]);
        }
        if !self.lists.contains_key(id) {
            ensure!(self.lists.len() < 64, "O conjunto excede 64 projetos");
            let query = [
                ("loaders", json!([self.software.loader()]).to_string()),
                ("game_versions", json!([self.minecraft]).to_string()),
            ];
            let mut list: Vec<Version> =
                serde_json::from_value(self.get(&format!("project/{id}/version"), &query)?)?;
            list.retain(|v| {
                v.project_id == id && compatible_with(v, &self.minecraft, self.software.loader())
            });
            list.truncate(48);
            for v in &list {
                identifier(&v.id)?;
                self.versions.insert(v.id.clone(), v.clone());
            }
            if let Some(old) = self.installed.iter().find(|r| r.project_id == id).cloned() {
                let old = self.version(&old.version_id)?;
                if !list.iter().any(|v| v.id == old.id) {
                    list.push(old);
                }
            }
            self.lists.insert(id.into(), list);
        }
        let mut list = self.lists[id].clone();
        for v in self.versions.values().filter(|v| v.project_id == id) {
            if !list.iter().any(|x| x.id == v.id) {
                list.push(v.clone());
            }
        }
        ensure!(
            list.len() <= 64,
            "Demasiadas versões fixadas para o mesmo projeto"
        );
        if !self.requested.contains(id) {
            if let Some(old) = self.installed.iter().find(|r| r.project_id == id) {
                list.sort_by_key(|v| v.id != old.version_id);
            }
        }
        let mut out = vec![];
        for v in list {
            if compatible_with(&v, &self.minecraft, self.software.loader()) && primary(&v).is_ok() {
                if let Some(r) = self.graph(&v)? {
                    out.push(r);
                }
            }
        }
        Ok(out)
    }
}
fn state(directory: &Path) -> Result<String> {
    let inventory = inventory(directory)?;
    ensure!(
        !inventory["pending"].as_bool().unwrap_or(true),
        "Resolve a alteração interrompida antes de continuar"
    );
    ensure!(
        inventory["installed"]
            .as_array()
            .context("Inventário inválido")?
            .iter()
            .all(|v| v["intact"] == true),
        "Há um JAR gerido alterado ou ausente; resolve-o antes de atualizar o conjunto"
    );
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &json!({"inventory":inventory,"config":fs::read(directory.join("nethergate.json"))?})
        )?)
    ))
}
pub fn preview(directory: &Path, projects: &[String]) -> Result<Bundle> {
    preview_for(directory, projects, None)
}
pub fn preview_for(directory: &Path, projects: &[String], target: Option<&str>) -> Result<Bundle> {
    backups::ensure_no_pending(directory)?;
    ensure_no_pending(directory)?;
    ensure!(projects.len() <= 64, "Escolhe até 64 projetos");
    let inventory_fingerprint = state(directory)?;
    let installed = load(directory)?;
    let mut cfg = Config::load(directory)?;
    if let Some(target) = target {
        cfg.minecraft = target.into();
        if target
            .split('.')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| n >= 26)
        {
            cfg.java_major = cfg.java_major.max(25);
        }
        cfg.validate()?;
    }
    let mut catalog = Catalog {
        http: client()?,
        minecraft: cfg.minecraft.clone(),
        software: cfg.software,
        installed: installed.clone(),
        requested: HashSet::new(),
        projects: BTreeMap::new(),
        lists: BTreeMap::new(),
        versions: BTreeMap::new(),
        calls: 0,
        deadline: std::time::Instant::now() + Duration::from_secs(120),
    };
    let requested = if projects.is_empty() {
        installed
            .iter()
            .map(|r| r.project_id.clone())
            .collect::<Vec<_>>()
    } else {
        projects.to_vec()
    };
    let mut roots = vec![];
    for id in requested {
        let info = catalog.project(&id)?;
        let id = info["id"].as_str().context("Projeto inválido")?.to_string();
        catalog.requested.insert(id.clone());
        if !roots.contains(&id) {
            roots.push(id);
        }
    }
    let mut canonical = roots.clone();
    canonical.sort();
    roots = canonical.clone();
    let mut existing: Vec<_> = installed.iter().map(|r| r.project_id.clone()).collect();
    existing.sort();
    for id in existing {
        if !roots.contains(&id) {
            roots.push(id);
        }
    }
    ensure!(
        !roots.is_empty(),
        "Não existem projetos geridos para atualizar"
    );
    // New exact references discovered while searching can expand an earlier candidate list.
    let mut result = None;
    for _ in 0..8 {
        let before = catalog.versions.len();
        let found = dependency_solver::solve(&roots, |id| catalog.candidates(id));
        match found {
            Ok(v) => {
                result = Some(v);
                break;
            }
            Err(e) => {
                if catalog.versions.len() == before {
                    return Err(e);
                }
            }
        }
    }
    let selected = result.context(
        "Não foi possível resolver todas as versões fixadas dentro do limite de pesquisa",
    )?;
    let mut items = vec![];
    for (id, release) in &selected {
        let version = catalog
            .versions
            .get(&release.version)
            .context("Versão não encontrada")?
            .clone();
        let file = primary(&version)?;
        let info = catalog.project(id)?;
        let previous = installed.iter().find(|r| r.project_id == *id).cloned();
        if previous
            .as_ref()
            .is_some_and(|r| r.version_id == release.version && r.minecraft == cfg.minecraft)
        {
            continue;
        }
        let folder = plugin_dir(directory)?;
        ensure!(
            !(previous.is_none() && folder.join(format!("nethergate-{id}.jar")).exists()),
            "Destino ocupado por um JAR fora do gestor"
        );
        ensure!(
            !folder.join(&file.filename).exists()
                || previous
                    .as_ref()
                    .is_some_and(|r| r.filename == file.filename),
            "Existe um JAR manual com o nome do download; revê as cópias antes de instalar"
        );
        items.push(Plan {
            missing_projects: vec![],
            software: cfg.software,
            distribution: Distribution {
                required_projects: release.required.iter().map(|r| r.project.clone()).collect(),
                url: file.url.clone(),
                sha1: file.hashes["sha1"].as_str().unwrap_or("").into(),
                size: file.size,
                client_side: info["client_side"].as_str().unwrap_or("unknown").into(),
                server_side: info["server_side"].as_str().unwrap_or("unknown").into(),
            },
            project_id: id.clone(),
            title: info["title"]
                .as_str()
                .unwrap_or(id)
                .chars()
                .take(120)
                .collect(),
            version_id: version.id.clone(),
            version_number: version.version_number.clone(),
            minecraft: cfg.minecraft.clone(),
            source_filename: file.filename.clone(),
            size: file.size,
            sha512: file.hashes["sha512"]
                .as_str()
                .context("A versão selecionada não contém um hash SHA-512 válido")?
                .to_ascii_lowercase(),
            url: file.url.clone(),
            previous,
            blockers: vec![],
            fingerprint: String::new(),
        });
    }
    let mut warnings=vec!["Resolve as dependências e incompatibilidades declaradas no Modrinth. Não garante compatibilidade em execução. Configurações existentes são preservadas; revê alterações de formato nas notas dos autores.".into()];
    if !inventory(directory)?["external"]
        .as_array()
        .context("Inventário inválido")?
        .is_empty()
    {
        warnings.push("Existem JARs manuais: os seus conflitos não podem ser resolvidos pelo catálogo. Serão preservados.".into());
    }
    let mut plan = Bundle {
        projects: canonical,
        minecraft: cfg.minecraft,
        items,
        warnings,
        fingerprint: String::new(),
        inventory: inventory_fingerprint,
    };
    plan.fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&plan)?));
    Ok(plan)
}

fn commit_set(
    directory: &Path,
    prepared: &Path,
    previous: &Path,
    records: &[Installed],
    snapshot: &Path,
) -> Result<()> {
    ensure_no_pending(directory)?;
    let folder = plugin_dir(directory)?;
    let old_records = load(directory)?;
    let had_folder = folder.exists();
    config::write_json(
        &directory.join(PENDING),
        &json!({"operation":"addon_set","previous":previous,"prepared":prepared,"snapshot":snapshot}),
    )?;
    let mut moved = false;
    let mut placed = false;
    let result = (|| -> Result<()> {
        if had_folder {
            fs::rename(&folder, previous)?;
            moved = true;
        }
        fs::rename(prepared, &folder)?;
        placed = true;
        crate::hub::replace_json(&directory.join(registry(directory)?), &records)?;
        Ok(())
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            if placed {
                fs::rename(&folder, prepared)?;
            }
            if moved {
                fs::rename(previous, &folder)?;
            }
            ensure!(
                serde_json::to_vec(&load(directory)?)? == serde_json::to_vec(&old_records)?,
                "O registo mudou durante a recuperação"
            );
            fs::remove_file(directory.join(PENDING))?;
            Ok(())
        })();
        if let Err(rollback) = rollback {
            bail!("Alteração interrompida: {error:#}. Recuperação necessária: {rollback:#}. Cópias em {}",snapshot.display());
        }
        return Err(error);
    }
    fs::remove_file(directory.join(PENDING))
        .context("Conjunto instalado; falta concluir o diário")?;
    Ok(())
}
pub fn install(
    root: &Path,
    id: &str,
    directory: &Path,
    projects: &[String],
    expected: &str,
) -> Result<Value> {
    let plan = preview(directory, projects)?;
    ensure!(
        plan.fingerprint == expected,
        "O plano ou o inventário mudou. Revê novamente antes de instalar"
    );
    ensure!(!plan.items.is_empty(), "Não há alterações a aplicar");
    let key = crate::transfer::key()?;
    let snapshot = secure_folder(root, &["hub-data", "addon-changes", id, &key])?;
    config::write_json(&snapshot.join("plan.json"), &plan)?;
    config::write_json(&snapshot.join("previous-records.json"), &load(directory)?)?;
    let http = client()?;
    for item in &plan.items {
        let path = snapshot.join(format!("{}.jar", item.project_id));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let response = http.get(&item.url).send()?.error_for_status()?;
        let bytes = std::io::copy(&mut response.take(item.size + 1), &mut output)?;
        output.sync_all()?;
        drop(output);
        ensure!(bytes == item.size, "Download incompleto");
        verify_download(&path, item.size, &item.sha512)?;
    }
    ensure!(
        state(directory)? == plan.inventory,
        "O inventário mudou durante os downloads; nenhum ficheiro ativo foi alterado"
    );
    let cfg = Config::load(directory)?;
    let folder = plugin_dir(directory)?;
    let prepared = directory.join(format!(".nethergate-addons-{key}.partial"));
    let previous = directory.join(format!(".nethergate-addons-previous-{key}"));
    if folder.exists() {
        copy_tree(&folder, &snapshot.join(cfg.software.folder()), &mut 0)?;
        copy_tree(&folder, &prepared, &mut 0)?;
    } else {
        fs::create_dir(&prepared)?;
    }
    if cfg.software == Software::Fabric && directory.join("config").exists() {
        copy_tree(&directory.join("config"), &snapshot.join("config"), &mut 0)?;
    }
    let world = if backups::world_ready(directory)? {
        Some(backups::create(
            &crate::backup_destination::resolve(root, id)?,
            id,
            directory,
            "before_addon_set",
        )?)
    } else {
        ensure!(
            !directory.join(backups::primary_world(directory)?).exists(),
            "Mundo incompleto"
        );
        None
    };
    let mut records = load(directory)?;
    for item in &plan.items {
        let filename = format!("nethergate-{}.jar", item.project_id);
        let target = prepared.join(&filename);
        fs::copy(snapshot.join(format!("{}.jar", item.project_id)), &target)?;
        verify_download(&target, item.size, &item.sha512)?;
        records.retain(|r| r.project_id != item.project_id);
        records.push(Installed {
            distribution: item.distribution.clone(),
            project_id: item.project_id.clone(),
            title: item.title.clone(),
            version_id: item.version_id.clone(),
            version_number: item.version_number.clone(),
            filename,
            sha512: item.sha512.clone(),
            minecraft: item.minecraft.clone(),
        });
    }
    ensure!(
        state(directory)? == plan.inventory,
        "O inventário mudou antes de publicar; cópias preservadas"
    );
    commit_set(directory, &prepared, &previous, &records, &snapshot)?;
    Ok(
        json!({"changed":plan.items.len(),"snapshot":snapshot,"previousDirectory":previous,"worldBackup":world,"restartRequired":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ng-addon-set-{}", crate::transfer::key().unwrap()));
        fs::create_dir_all(root.join("plugins")).unwrap();
        config::write_json(
            &root.join("nethergate.json"),
            &Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap(),
        )
        .unwrap();
        fs::write(root.join("server.properties"), "level-name=world\n").unwrap();
        root
    }
    fn record(directory: &Path, version: &str) -> Installed {
        Installed {
            distribution: Distribution::default(),
            project_id: "project1".into(),
            title: "Fixture".into(),
            version_id: version.into(),
            version_number: version.into(),
            filename: "nethergate-project1.jar".into(),
            sha512: digest(&directory.join("nethergate-project1.jar")).unwrap(),
            minecraft: "1.21.11".into(),
        }
    }
    #[test]
    fn set_publication_rolls_back_on_registry_failure_and_preserves_previous_folder() {
        let root = fixture();
        let folder = root.join("plugins");
        fs::write(folder.join("nethergate-project1.jar"), b"old").unwrap();
        fs::write(folder.join("settings.yml"), b"private-config").unwrap();
        let old = record(&folder, "old1");
        config::write_json(&root.join("nethergate-plugins.json"), &vec![old.clone()]).unwrap();
        let staged = root.join("staged");
        copy_tree(&folder, &staged, &mut 0).unwrap();
        fs::write(staged.join("nethergate-project1.jar"), b"new").unwrap();
        let new = record(&staged, "new1");
        let snapshot = root.join("snapshot");
        fs::create_dir(&snapshot).unwrap();
        let previous = root.join("previous");
        fs::create_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        assert!(commit_set(
            &root,
            &staged,
            &previous,
            std::slice::from_ref(&new),
            &snapshot
        )
        .is_err());
        assert_eq!(
            fs::read(folder.join("nethergate-project1.jar")).unwrap(),
            b"old"
        );
        assert_eq!(load(&root).unwrap()[0].version_id, "old1");
        assert!(!root.join(PENDING).exists());
        fs::remove_dir(root.join("nethergate-plugins.json.tmp")).unwrap();
        commit_set(&root, &staged, &previous, &[new], &snapshot).unwrap();
        assert_eq!(
            fs::read(folder.join("nethergate-project1.jar")).unwrap(),
            b"new"
        );
        assert_eq!(
            fs::read(previous.join("nethergate-project1.jar")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::read(folder.join("settings.yml")).unwrap(),
            b"private-config"
        );
    }
    #[test]
    fn modified_managed_files_and_pending_transactions_block_planning() {
        let root = fixture();
        let folder = root.join("plugins");
        fs::write(folder.join("nethergate-project1.jar"), b"old").unwrap();
        let old = record(&folder, "old1");
        config::write_json(&root.join("nethergate-plugins.json"), &vec![old]).unwrap();
        let before = state(&root).unwrap();
        fs::write(folder.join("external.jar"), b"manual").unwrap();
        assert_ne!(state(&root).unwrap(), before);
        fs::write(folder.join("nethergate-project1.jar"), b"edited").unwrap();
        assert!(state(&root).is_err());
        fs::write(root.join(PENDING), "pending").unwrap();
        assert!(preview(&root, &["chunky".into()]).is_err());
    }
    #[test]
    #[ignore = "Downloads a real Modrinth release into an isolated synthetic world"]
    fn real_modrinth_set_downloads_verifies_and_preserves_world() {
        let root = fixture();
        fs::create_dir(root.join("world")).unwrap();
        fs::write(root.join("world/level.dat"), b"synthetic-world").unwrap();
        fs::write(root.join("plugins/settings.yml"), b"keep").unwrap();
        let plan = preview(&root, &["chunky".into()]).unwrap();
        assert!(!plan.items.is_empty());
        let result = install(&root, "fixture", &root, &plan.projects, &plan.fingerprint).unwrap();
        assert!(result["changed"].as_u64().unwrap() > 0);
        assert!(result["worldBackup"].is_object());
        assert_eq!(
            fs::read(root.join("world/level.dat")).unwrap(),
            b"synthetic-world"
        );
        assert_eq!(
            fs::read(root.join("plugins/settings.yml")).unwrap(),
            b"keep"
        );
        assert!(inventory(&root).unwrap()["installed"][0]["intact"] == true);
    }
}
