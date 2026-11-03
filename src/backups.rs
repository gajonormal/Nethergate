//! Full, offline world snapshots. Server binaries, credentials and hub settings are excluded.
use crate::{config::Config, paper::hash_file};
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const PENDING: &str = ".nethergate-restore-pending.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupInfo {
    pub id: String,
    pub created_at: u64,
    pub size_bytes: u64,
    pub file_count: usize,
    pub worlds: Vec<String>,
    pub minecraft_version: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: u32,
    server_id: String,
    primary_world: String,
    info: BackupInfo,
    files: Vec<Record>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Record {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub safety_backup: BackupInfo,
    pub restored_backup: String,
    pub cleanup_warning: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    source_backup: String,
    safety_backup: String,
    transaction: String,
    old_worlds: Vec<String>,
    new_worlds: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPlan {
    pub fingerprint: String,
    pub safety_backup: BackupInfo,
    pub source_backup: String,
    pub worlds: Vec<String>,
}

/// Only the verified safety snapshot is trusted; never follow paths in a journal.
pub fn recovery_plan(
    store: &Path,
    server_id: &str,
    directory: &Path,
) -> Result<Option<RecoveryPlan>> {
    use sha2::{Digest, Sha256};
    let marker = directory.join(PENDING);
    if !recovery_pending(directory)? {
        return Ok(None);
    }
    ensure!(
        ordinary(&marker)?.is_file() && fs::metadata(&marker)?.len() <= 65536,
        "Diário de restauro inválido"
    );
    let bytes = fs::read(&marker)?;
    let journal: Journal = serde_json::from_slice(&bytes)
        .context("O diário está incompleto; preserva-o para recuperação manual")?;
    valid_id(&journal.safety_backup)?;
    valid_id(&journal.source_backup)?;
    ensure!(
        journal.old_worlds.len() <= 256 && journal.new_worlds.len() <= 256,
        "Diário demasiado grande"
    );
    let (folder, metadata) = manifest(store, server_id, &journal.safety_backup)?;
    ensure!(
        metadata.info.reason == "before_restore",
        "A cópia indicada não é uma cópia anterior ao restauro"
    );
    ensure!(
        metadata.info.minecraft_version == Config::load(directory)?.minecraft
            && metadata.primary_world == primary_world(directory)?,
        "A configuração do servidor mudou; recupera primeiro a versão e o nome do mundo originais"
    );
    verify(&folder, &metadata)?;
    let mut worlds = journal.old_worlds;
    worlds.extend(journal.new_worlds);
    worlds.extend(metadata.info.worlds.clone());
    worlds.sort();
    worlds.dedup();
    let mut names = HashSet::new();
    for world in &worlds {
        component(world)?;
        ensure!(
            names.insert(world.to_lowercase()),
            "Mundos ambíguos no diário"
        );
        if directory.join(world).try_exists()? {
            ordinary_tree(&directory.join(world))?;
        }
    }
    let mut digest = Sha256::new();
    digest.update(&bytes);
    digest.update(fs::read(folder.join("manifest.json"))?);
    Ok(Some(RecoveryPlan {
        fingerprint: format!("{:x}", digest.finalize()),
        safety_backup: metadata.info,
        source_backup: journal.source_backup,
        worlds,
    }))
}

/// A failed recovery can be repeated: every attempt starts from the immutable safety
/// snapshot and preserves the partial state in a fresh transaction, including after a crash.
pub fn recover(store: &Path, server_id: &str, directory: &Path, expected: &str) -> Result<String> {
    let plan =
        recovery_plan(store, server_id, directory)?.context("Não existe restauro pendente")?;
    ensure!(
        plan.fingerprint == expected,
        "O plano mudou; revê a recuperação novamente"
    );
    let (folder, metadata) = manifest(store, server_id, &plan.safety_backup.id)?;
    for world in &plan.worlds {
        if !directory.join(world).exists() {
            fs::create_dir(directory.join(world))?;
        }
    }
    let guards = world_locks(directory, &plan.worlds)?;
    // Renames must remain on the server volume, even with backups on an external disk.
    let transaction = directory.join(format!(".nethergate-restore-recovery-{}", identifier()?));
    fs::create_dir(&transaction)?;
    let staged = transaction.join("staged");
    copy_records(&folder.join("worlds"), &staged, &metadata.files)?;
    let previous = transaction.join("interrupted");
    fs::create_dir(&previous)?;
    for world in &plan.worlds {
        fs::create_dir(previous.join(world))?;
        for entry in fs::read_dir(directory.join(world))? {
            let entry = entry?;
            if entry.file_name() != "session.lock" {
                fs::rename(entry.path(), previous.join(world).join(entry.file_name()))?;
            }
        }
    }
    for world in &metadata.info.worlds {
        for entry in fs::read_dir(staged.join(world))? {
            let entry = entry?;
            fs::rename(entry.path(), directory.join(world).join(entry.file_name()))?;
        }
    }
    // Verify the published worlds before unblocking start. Any failure keeps the marker.
    let mut actual = Vec::new();
    for world in &metadata.info.worlds {
        collect(directory, Path::new(world), &mut actual)?;
    }
    actual.sort_by(|a, b| a.path.cmp(&b.path));
    ensure!(
        actual == metadata.files,
        "Recuperação incompleta; mantém-se o bloqueio e podes tentar novamente"
    );
    drop(guards);
    for world in &plan.worlds {
        if !metadata.info.worlds.contains(world) {
            fs::rename(
                directory.join(world),
                transaction.join(format!("unused-{world}")),
            )?;
        }
    }
    fs::remove_file(directory.join(PENDING))?;
    Ok(format!(
        "Estado anterior reposto. Ficheiros da tentativa interrompida preservados em {}",
        previous.display()
    ))
}

fn identifier() -> Result<String> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let mut random = [0u8; 8];
    getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("Falha ao gerar ID: {e}"))?;
    Ok(format!(
        "{stamp}-{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}

pub(crate) fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 80 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'),
        "Identificador de backup inválido"
    );
    Ok(())
}

pub(crate) fn component(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name
                .chars()
                .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c)),
        "Nome de ficheiro/pasta não suportado: {name}"
    );
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9')),
        "Nome reservado do Windows: {name}"
    );
    Ok(())
}

fn safe_relative(name: &str) -> Result<PathBuf> {
    ensure!(!name.contains('\\'), "Caminho inválido no manifesto");
    let path = PathBuf::from(name);
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Caminho fora do backup"
    );
    for part in name.split('/') {
        component(part)?;
    }
    Ok(path)
}

pub(crate) fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Não foi possível ler {}", path.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Links simbólicos não são incluídos: {}",
        path.display()
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Junction/reparse point não suportado: {}",
            path.display()
        );
    }
    ensure!(
        metadata.is_file() || metadata.is_dir(),
        "Tipo de ficheiro não suportado"
    );
    Ok(metadata)
}

fn store_path(store: &Path) -> Result<PathBuf> {
    if store.exists() {
        ensure!(ordinary(store)?.is_dir(), "A pasta de backups não é válida");
    } else {
        fs::create_dir_all(store)?;
    }
    Ok(store.canonicalize()?)
}

pub(crate) fn primary_world(directory: &Path) -> Result<String> {
    let properties = fs::read_to_string(directory.join("server.properties"))?;
    let world = properties
        .lines()
        .filter_map(|line| line.trim().strip_prefix("level-name="))
        .next_back()
        .unwrap_or("world")
        .trim();
    component(world)?;
    Ok(world.into())
}

pub(crate) fn world_names(directory: &Path) -> Result<Vec<String>> {
    let primary = primary_world(directory)?;
    let primary_path = directory.join(&primary);
    ensure!(
        primary_path.join("level.dat").is_file(),
        "O mundo ainda não existe. Inicia e para o servidor antes do primeiro backup."
    );
    let expected = [
        primary.clone(),
        format!("{primary}_nether"),
        format!("{primary}_the_end"),
    ];
    let mut worlds = Vec::new();
    for item in fs::read_dir(directory)? {
        let item = item?;
        let Some(name) = item.file_name().to_str().map(String::from) else {
            bail!("Nome de pasta não UTF-8");
        };
        if item.path().join("level.dat").is_file() || expected.contains(&name) {
            if !item.path().exists() {
                continue;
            }
            component(&name)?;
            ensure!(
                ordinary(&item.path())?.is_dir(),
                "O mundo não é uma pasta normal"
            );
            ensure!(
                ordinary(&item.path().join("level.dat"))?.is_file(),
                "Mundo sem level.dat: {name}"
            );
            worlds.push(name);
        }
    }
    worlds.sort();
    ensure!(worlds.contains(&primary), "Mundo principal não encontrado");
    Ok(worlds)
}

pub(crate) fn world_locks(directory: &Path, worlds: &[String]) -> Result<Vec<File>> {
    let mut guards = Vec::new();
    for world in worlds {
        let lock = directory.join(world).join("session.lock");
        if lock.exists() {
            ordinary(&lock)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock)?;
        file.try_lock_exclusive().context("O mundo está a ser usado por Java ou outro programa. Para o servidor antes de continuar.")?;
        guards.push(file);
    }
    Ok(guards)
}

fn collect(base: &Path, relative: &Path, records: &mut Vec<Record>) -> Result<()> {
    let path = base.join(relative);
    let metadata = ordinary(&path)?;
    if metadata.is_dir() {
        for item in fs::read_dir(path)? {
            let item = item?;
            let name = item.file_name();
            let name = name.to_str().context("Nome de ficheiro não UTF-8")?;
            component(name)?;
            if name == "session.lock" {
                continue;
            }
            collect(base, &relative.join(name), records)?;
        }
    } else {
        let key = relative
            .to_str()
            .context("Caminho não UTF-8")?
            .replace('\\', "/");
        safe_relative(&key)?;
        records.push(Record {
            path: key,
            bytes: metadata.len(),
            sha256: hash_file(&path)?,
        });
    }
    Ok(())
}

fn copy_records(source: &Path, destination: &Path, records: &[Record]) -> Result<()> {
    for record in records {
        let relative = safe_relative(&record.path)?;
        let from = source.join(&relative);
        ensure!(
            ordinary(&from)?.is_file(),
            "O ficheiro do mundo foi alterado"
        );
        let to = destination.join(relative);
        fs::create_dir_all(to.parent().context("Caminho inválido")?)?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(&to)?;
        let mut input = File::open(&from)?;
        let written = std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        ensure!(
            written == record.bytes && hash_file(&to)? == record.sha256,
            "Falha de integridade ao copiar {}",
            record.path
        );
    }
    Ok(())
}

pub fn ensure_no_pending(directory: &Path) -> Result<()> {
    ensure!(!recovery_pending(directory)?, "Há um restauro interrompido. Abre Mundo → Recuperação do mundo para verificar e repor a cópia anterior. Mantém o diário .nethergate-restore-pending.json e os ficheiros preservados.");
    Ok(())
}

pub fn recovery_pending(directory: &Path) -> Result<bool> {
    Ok(fs::symlink_metadata(directory.join(PENDING))
        .map(|_| true)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(e)
            }
        })?)
}

pub fn create(store: &Path, server_id: &str, directory: &Path, reason: &str) -> Result<BackupInfo> {
    ensure_no_pending(directory)?;
    let worlds = world_names(directory)?;
    let _world_guards = world_locks(directory, &worlds)?;
    create_locked(store, server_id, directory, worlds, reason)
}

fn create_locked(
    store: &Path,
    server_id: &str,
    directory: &Path,
    worlds: Vec<String>,
    reason: &str,
) -> Result<BackupInfo> {
    let store = store_path(store)?;
    let mut files = Vec::new();
    for world in &worlds {
        collect(directory, Path::new(world), &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let info = BackupInfo {
        id: identifier()?,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .try_into()?,
        size_bytes: files.iter().map(|f| f.bytes).sum(),
        file_count: files.len(),
        worlds,
        minecraft_version: Config::load(directory)?.minecraft,
        reason: reason.into(),
    };
    let partial = store.join(format!("{}.partial", info.id));
    fs::create_dir(&partial)?;
    copy_records(directory, &partial.join("worlds"), &files)?;
    let manifest = Manifest {
        format: 1,
        server_id: server_id.into(),
        primary_world: primary_world(directory)?,
        info: info.clone(),
        files,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(partial.join("manifest.json"))?;
    file.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(partial, store.join(&info.id))?;
    Ok(info)
}

fn manifest(store: &Path, server_id: &str, id: &str) -> Result<(PathBuf, Manifest)> {
    valid_id(id)?;
    let folder = store_path(store)?.join(id);
    ensure!(ordinary(&folder)?.is_dir(), "Backup não encontrado");
    let metadata = ordinary(&folder.join("manifest.json"))?;
    ensure!(
        metadata.is_file() && metadata.len() <= 64 * 1024 * 1024,
        "Manifesto inválido"
    );
    let manifest: Manifest = serde_json::from_slice(&fs::read(folder.join("manifest.json"))?)?;
    ensure!(
        manifest.format == 1 && manifest.server_id == server_id && manifest.info.id == id,
        "O backup não pertence a este servidor ou usa um formato incompatível"
    );
    component(&manifest.primary_world)?;
    let mut worlds = HashSet::new();
    for world in &manifest.info.worlds {
        component(world)?;
        ensure!(
            worlds.insert(world.to_lowercase()),
            "Mundos duplicados no manifesto"
        );
    }
    ensure!(
        manifest.info.worlds.contains(&manifest.primary_world),
        "O backup não inclui o mundo principal"
    );
    let mut paths = HashSet::new();
    let mut bytes = 0u64;
    for file in &manifest.files {
        let path = safe_relative(&file.path)?;
        let root = path
            .components()
            .next()
            .context("Caminho inválido")?
            .as_os_str()
            .to_str()
            .context("Nome inválido")?;
        ensure!(
            manifest.info.worlds.iter().any(|w| w == root),
            "Ficheiro fora dos mundos indicados"
        );
        ensure!(
            paths.insert(file.path.to_lowercase()),
            "Ficheiros duplicados no manifesto"
        );
        ensure!(
            file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Checksum inválido"
        );
        bytes = bytes.checked_add(file.bytes).context("Tamanho inválido")?;
    }
    ensure!(
        bytes == manifest.info.size_bytes && manifest.files.len() == manifest.info.file_count,
        "Contagem de ficheiros/tamanho inconsistente"
    );
    for world in &manifest.info.worlds {
        ensure!(
            paths.contains(&format!("{world}/level.dat").to_lowercase()),
            "Falta level.dat no mundo {world}"
        );
    }
    Ok((folder, manifest))
}

pub fn list(store: &Path, server_id: &str) -> Result<Vec<BackupInfo>> {
    if !store.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for item in fs::read_dir(store_path(store)?)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".nethergate-store.json"
            || name.ends_with(".partial")
            || name.ends_with(".retired")
            || name.starts_with("restore-")
        {
            continue;
        }
        valid_id(&name)?;
        result.push(manifest(store, server_id, &name)?.1.info);
    }
    result.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(result)
}

pub fn world_ready(directory: &Path) -> Result<bool> {
    Ok(directory
        .join(primary_world(directory)?)
        .join("level.dat")
        .is_file())
}

/// Do not replace useful history with hourly copies of the same offline world.
pub fn unchanged_since(store: &Path, server_id: &str, directory: &Path, id: &str) -> Result<bool> {
    let Ok((folder, metadata)) = manifest(store, server_id, id) else {
        return Ok(false);
    };
    if metadata.info.reason != "automatic"
        || metadata.info.minecraft_version != Config::load(directory)?.minecraft
        || verify(&folder, &metadata).is_err()
    {
        return Ok(false);
    }
    let worlds = world_names(directory)?;
    if worlds != metadata.info.worlds || primary_world(directory)? != metadata.primary_world {
        return Ok(false);
    }
    let _guards = world_locks(directory, &worlds)?;
    let mut records = Vec::new();
    for world in worlds {
        collect(directory, Path::new(&world), &mut records)?;
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(records == metadata.files)
}

fn ordinary_tree(path: &Path) -> Result<()> {
    if ordinary(path)?.is_dir() {
        for entry in fs::read_dir(path)? {
            ordinary_tree(&entry?.path())?;
        }
    }
    Ok(())
}

/// Expire only recognized automatic copies, after checking all survivors.
pub fn retain_automatic(store: &Path, server_id: &str, newest: &str, keep: usize) -> Result<usize> {
    ensure!((2..=50).contains(&keep), "Retenção automática inválida");
    let intended = std::path::absolute(store)?;
    let store = store_path(store)?;
    let expected = crate::firewall::ps_path(&intended);
    let resolved = crate::firewall::ps_path(&store);
    ensure!(
        if cfg!(windows) {
            expected.eq_ignore_ascii_case(&resolved)
        } else {
            expected == resolved
        },
        "A pasta de retenção atravessa um link ou junction; nada foi eliminado"
    );
    let mut copies: Vec<_> = list(&store, server_id)?
        .into_iter()
        .filter(|b| b.reason == "automatic")
        .collect();
    copies.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    // Always retain the just-created copy, including if the system clock moved backwards.
    let index = copies
        .iter()
        .position(|b| b.id == newest)
        .context("A nova cópia automática não foi encontrada; nada foi eliminado")?;
    let fresh = copies.remove(index);
    copies.insert(0, fresh);
    for copy in copies.iter().take(keep) {
        let (folder, metadata) = manifest(&store, server_id, &copy.id)?;
        verify(&folder, &metadata)?;
    }
    let mut candidates = Vec::new();
    for copy in copies.iter().skip(keep) {
        let (folder, metadata) = manifest(&store, server_id, &copy.id)?;
        ensure!(
            metadata.info.reason == "automatic",
            "A cópia deixou de ser automática"
        );
        let resolved = folder.canonicalize()?;
        let retired = store.join(format!("{}.retired", copy.id));
        ensure!(
            resolved.parent() == Some(store.as_path())
                && resolved.starts_with(&store)
                && retired.parent() == Some(store.as_path())
                && !retired.exists(),
            "Destino de retenção inválido"
        );
        ordinary_tree(&resolved)?;
        candidates.push((resolved, retired));
    }
    let mut removed = 0;
    for (folder, retired) in candidates {
        fs::rename(&folder, &retired)?;
        let resolved = retired.canonicalize()?;
        ensure!(
            resolved.parent() == Some(store.as_path()) && resolved.starts_with(&store),
            "Destino de limpeza inválido"
        );
        fs::remove_dir_all(&resolved).context("A nova cópia está guardada, mas ficaram dados antigos .retired por limpar na pasta de backups")?;
        removed += 1;
    }
    Ok(removed)
}

fn verify(folder: &Path, manifest: &Manifest) -> Result<()> {
    let source = folder.join("worlds");
    ordinary(&source)?;
    for world in &manifest.info.worlds {
        ordinary(&source.join(world))?;
    }
    for record in &manifest.files {
        let relative = safe_relative(&record.path)?;
        let mut path = source.clone();
        for part in relative.components() {
            path.push(part);
            ordinary(&path)?;
        }
        ensure!(
            fs::metadata(&path)?.len() == record.bytes && hash_file(&path)? == record.sha256,
            "Backup corrompido ou alterado: {}. O mundo atual não foi substituído.",
            record.path
        );
    }
    // Reject extra files and directory links, not just the files named by the manifest.
    let mut actual = Vec::new();
    for world in &manifest.info.worlds {
        collect(&source, Path::new(world), &mut actual)?;
    }
    let expected: HashSet<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    ensure!(
        actual.len() == expected.len() && actual.iter().all(|f| expected.contains(f.path.as_str())),
        "O conteúdo do backup não corresponde ao manifesto"
    );
    Ok(())
}
#[derive(Clone)]
pub(crate) struct UploadFile {
    pub path: PathBuf,
    pub key: String,
    pub bytes: u64,
    pub sha256: String,
}
pub(crate) fn cloud_snapshot(
    store: &Path,
    server_id: &str,
    id: &str,
) -> Result<(Vec<u8>, Vec<UploadFile>)> {
    let (folder, metadata) = manifest(store, server_id, id)?;
    verify(&folder, &metadata)?;
    let files = metadata
        .files
        .iter()
        .map(|r| UploadFile {
            path: folder.join("worlds").join(&r.path),
            key: format!("worlds/{}", r.path),
            bytes: r.bytes,
            sha256: r.sha256.clone(),
        })
        .collect();
    Ok((fs::read(folder.join("manifest.json"))?, files))
}
pub(crate) fn receive_cloud(
    store: &Path,
    server_id: &str,
    id: &str,
    bytes: &[u8],
    mut fetch: impl FnMut(&str, &Path, u64, &str) -> Result<()>,
) -> Result<BackupInfo> {
    valid_id(id)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "Manifesto cloud demasiado grande"
    );
    let mut metadata: Manifest = serde_json::from_slice(bytes)?;
    ensure!(
        metadata.info.id == id,
        "O arquivo cloud tem outro identificador"
    );
    metadata.server_id = server_id.into();
    let store = store_path(store)?;
    if store.join(id).exists() {
        let (folder, current) = manifest(&store, server_id, id)?;
        verify(&folder, &current)?;
        ensure!(
            current.files == metadata.files
                && current.info.minecraft_version == metadata.info.minecraft_version,
            "Existe um backup local diferente com o mesmo ID"
        );
        return Ok(current.info);
    }
    let staging = store.join(format!("cloud-{}.partial", identifier()?));
    fs::create_dir(&staging)?;
    let folder = staging.join(id);
    fs::create_dir(&folder)?;
    metadata.info.reason = "cloud".into();
    crate::config::write_json(&folder.join("manifest.json"), &metadata)?;
    // Reuse every structural validation before trusting remote paths or making download requests.
    let (_, validated) = manifest(&staging, server_id, id)?;
    ensure!(
        validated.files.len() <= 200_000,
        "Demasiados ficheiros no backup cloud"
    );
    for record in &validated.files {
        let target = folder.join("worlds").join(safe_relative(&record.path)?);
        fs::create_dir_all(target.parent().context("Caminho inválido")?)?;
        fetch(
            &format!("worlds/{}", record.path),
            &target,
            record.bytes,
            &record.sha256,
        )?;
    }
    verify(&folder, &validated)?;
    ensure!(!store.join(id).exists(), "O destino foi ocupado");
    fs::rename(&folder, store.join(id))?;
    fs::remove_dir(&staging)?;
    Ok(validated.info)
}

pub fn restore(store: &Path, server_id: &str, directory: &Path, id: &str) -> Result<RestoreResult> {
    ensure_no_pending(directory)?;
    let (folder, manifest) = manifest(store, server_id, id)?;
    ensure!(
        Config::load(directory)?.minecraft == manifest.info.minecraft_version,
        "O backup usa outra versão de Minecraft. Restaura num perfil da mesma versão; para atualizar, usa a migração para um perfil novo."
    );
    ensure!(
        primary_world(directory)? == manifest.primary_world,
        "O nome do mundo principal mudou desde este backup"
    );
    verify(&folder, &manifest)?;
    let current = world_names(directory)?;
    for world in &manifest.info.worlds {
        ensure!(
            current.contains(world) || !directory.join(world).exists(),
            "Existe uma pasta de destino que não foi reconhecida como mundo: {world}"
        );
    }
    let mut world_guards = world_locks(directory, &current)?;
    let safety = create_locked(
        store,
        server_id,
        directory,
        current.clone(),
        "before_restore",
    )?;
    let transaction = directory.join(format!(".nethergate-restore-{}", identifier()?));
    fs::create_dir(&transaction)?;
    let staged = transaction.join("staged");
    let previous = transaction.join("previous");
    fs::create_dir(&previous)?;
    copy_records(&folder.join("worlds"), &staged, &manifest.files)?;
    // All copies and the safety backup have been verified before any world is moved.
    let marker = directory.join(PENDING);
    let journal = Journal {
        source_backup: id.into(),
        safety_backup: safety.id.clone(),
        transaction: transaction.display().to_string(),
        old_worlds: current.clone(),
        new_worlds: manifest.info.worlds.clone(),
    };
    // Publish a complete, flushed journal before the first destructive rename.
    let staged_journal = transaction.join("journal.json");
    crate::config::write_json(&staged_journal, &journal)?;
    ensure!(
        !recovery_pending(directory)?,
        "Foi encontrado outro restauro pendente"
    );
    fs::rename(staged_journal, &marker)?;
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let outcome = (|| -> Result<()> {
        for world in &manifest.info.worlds {
            if !current.contains(world) {
                fs::create_dir(directory.join(world))?;
                world_guards.extend(world_locks(directory, std::slice::from_ref(world))?);
            }
        }
        // Retain session.lock in each world directory: Windows does not allow moving
        // its parent while it is locked. Move data entries, keeping the Java lock held.
        for world in &current {
            fs::create_dir(previous.join(world))?;
            for item in fs::read_dir(directory.join(world))? {
                let item = item?;
                let name = item.file_name();
                if name == "session.lock" {
                    continue;
                }
                fs::rename(item.path(), previous.join(world).join(&name))
                    .with_context(|| format!("Falha ao preservar o mundo {world}"))?;
                moved.push((world.clone(), name));
            }
        }
        for world in &manifest.info.worlds {
            for item in fs::read_dir(staged.join(world))? {
                let item = item?;
                let name = item.file_name();
                ensure!(
                    !directory.join(world).join(&name).exists(),
                    "Já existe um ficheiro de destino"
                );
                fs::rename(item.path(), directory.join(world).join(&name))?;
                installed.push((world.clone(), name));
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        let mut rollback_errors = Vec::new();
        for (world, name) in installed.iter().rev() {
            if let Err(e) = fs::rename(
                directory.join(world).join(name),
                staged.join(world).join(name),
            ) {
                rollback_errors.push(format!("{world}: {e}"));
            }
        }
        for (world, name) in moved.iter().rev() {
            if let Err(e) = fs::rename(
                previous.join(world).join(name),
                directory.join(world).join(name),
            ) {
                rollback_errors.push(format!("{world}: {e}"));
            }
        }
        if rollback_errors.is_empty() {
            fs::remove_file(&marker)?;
            bail!("O restauro falhou e o estado anterior foi reposto. Cópia de segurança: {}. Causa: {error:#}", safety.id);
        }
        bail!("Restauro interrompido; o arranque foi bloqueado. Os dados estão preservados em {} e no backup {}. Recuperação necessária: {}. Causa: {error:#}", transaction.display(), safety.id, rollback_errors.join("; "));
    }
    fs::remove_file(&marker)?;
    // Keep the previous directories as a second recovery copy. No world data is deleted.
    Ok(RestoreResult {
        safety_backup: safety,
        restored_backup: id.into(),
        cleanup_warning: Some(format!(
            "Ficheiros anteriores preservados em {}",
            previous.display()
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("ng-backup-{}", identifier().unwrap()));
        let server = root.join("server");
        fs::create_dir_all(&server).unwrap();
        let cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
        crate::config::write_json(&server.join("nethergate.json"), &cfg).unwrap();
        fs::write(server.join("server.properties"), "level-name=world\n").unwrap();
        for name in ["world", "world_nether", "world_the_end"] {
            fs::create_dir_all(server.join(name).join("region")).unwrap();
            fs::write(
                server.join(name).join("level.dat"),
                format!("original-{name}"),
            )
            .unwrap();
            fs::write(
                server.join(name).join("region/r.0.0.mca"),
                format!("chunks-{name}"),
            )
            .unwrap();
        }
        (root.join("backups"), server)
    }
    #[test]
    fn recovery_restores_verified_safety_and_preserves_partial_data() {
        let (store, server) = fixture();
        let safety = create(&store, "test", &server, "before_restore").unwrap();
        let journal = Journal {
            source_backup: safety.id.clone(),
            safety_backup: safety.id.clone(),
            transaction: "untrusted/path/never/followed".into(),
            old_worlds: safety.worlds.clone(),
            new_worlds: vec!["world_extra".into()],
        };
        crate::config::write_json(&server.join(PENDING), &journal).unwrap();
        fs::write(server.join("world/level.dat"), "partial").unwrap();
        fs::create_dir(server.join("world_extra")).unwrap();
        fs::write(server.join("world_extra/partial.dat"), "partial-extra").unwrap();
        let plan = recovery_plan(&store, "test", &server).unwrap().unwrap();
        assert!(recover(&store, "test", &server, "stale").is_err());
        let locks = world_locks(&server, &safety.worlds).unwrap();
        assert!(recover(&store, "test", &server, &plan.fingerprint).is_err());
        assert!(recovery_pending(&server).unwrap());
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "partial"
        );
        drop(locks);
        // Simulate a crash in a previous recovery, after moving some data away.
        fs::remove_file(server.join("world/region/r.0.0.mca")).unwrap();
        let result = recover(&store, "test", &server, &plan.fingerprint).unwrap();
        assert!(result.contains("preservados"));
        assert!(!recovery_pending(&server).unwrap());
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "original-world"
        );
        assert_eq!(
            fs::read_to_string(server.join("world/region/r.0.0.mca")).unwrap(),
            "chunks-world"
        );
        let recovery = fs::read_dir(&server)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".nethergate-restore-recovery-")
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(recovery.join("interrupted/world/level.dat")).unwrap(),
            "partial"
        );
        assert_eq!(
            fs::read_to_string(recovery.join("interrupted/world_extra/partial.dat")).unwrap(),
            "partial-extra"
        );
    }
    #[test]
    fn corrupt_recovery_snapshot_or_journal_never_unblocks_start() {
        let (store, server) = fixture();
        let safety = create(&store, "test", &server, "before_restore").unwrap();
        let journal = Journal {
            source_backup: safety.id.clone(),
            safety_backup: safety.id.clone(),
            transaction: "ignored".into(),
            old_worlds: safety.worlds.clone(),
            new_worlds: vec!["../escape".into()],
        };
        crate::config::write_json(&server.join(PENDING), &journal).unwrap();
        assert!(recovery_plan(&store, "test", &server).is_err());
        fs::write(
            store.join(&safety.id).join("worlds/world/level.dat"),
            "corrupt",
        )
        .unwrap();
        assert!(recovery_plan(&store, "test", &server).is_err());
        assert!(ensure_no_pending(&server).is_err());
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "original-world"
        );
    }
    #[test]
    fn retention_preserves_manual_and_restore_copies_and_keeps_newest() {
        let (store, server) = fixture();
        let manual = create(&store, "test", &server, "manual").unwrap();
        let safety = create(&store, "test", &server, "before_restore").unwrap();
        let mut copies = Vec::new();
        for _ in 0..4 {
            copies.push(create(&store, "test", &server, "automatic").unwrap());
        }
        let newest = &copies.last().unwrap().id;
        assert_eq!(retain_automatic(&store, "test", newest, 2).unwrap(), 2);
        let list = list(&store, "test").unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list.iter().filter(|b| b.reason == "automatic").count(), 2);
        for id in [&manual.id, &safety.id, newest] {
            assert!(store.join(id).is_dir());
        }
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "original-world"
        );
        let restored = restore(&store, "test", &server, newest).unwrap();
        assert_eq!(restored.restored_backup, *newest);
    }
    #[test]
    fn corrupt_survivor_prevents_any_retention_deletion() {
        let (store, server) = fixture();
        let mut copies = Vec::new();
        for _ in 0..4 {
            copies.push(create(&store, "test", &server, "automatic").unwrap());
        }
        let newest = &copies.last().unwrap().id;
        fs::write(store.join(newest).join("worlds/world/level.dat"), "corrupt").unwrap();
        assert!(retain_automatic(&store, "test", newest, 2).is_err());
        for copy in &copies {
            assert!(store.join(&copy.id).join("manifest.json").is_file());
        }
        assert!(retain_automatic(&store, "other-server", newest, 2).is_err());
    }
    #[test]
    fn unchanged_world_is_skipped_only_with_a_verified_existing_copy() {
        let (store, server) = fixture();
        let copy = create(&store, "test", &server, "automatic").unwrap();
        assert!(unchanged_since(&store, "test", &server, &copy.id).unwrap());
        fs::write(
            store.join(&copy.id).join("worlds/world/level.dat"),
            "damaged",
        )
        .unwrap();
        assert!(!unchanged_since(&store, "test", &server, &copy.id).unwrap());
        let copy = create(&store, "test", &server, "automatic").unwrap();
        fs::write(server.join("world/level.dat"), "new progress").unwrap();
        assert!(!unchanged_since(&store, "test", &server, &copy.id).unwrap());
    }
    #[test]
    fn restores_all_dimensions_and_saves_the_current_world() {
        let (store, server) = fixture();
        let snapshot = create(&store, "test", &server, "manual").unwrap();
        assert_eq!(snapshot.worlds.len(), 3);
        assert_eq!(snapshot.file_count, 6);
        fs::write(server.join("world/level.dat"), "new progress").unwrap();
        fs::write(server.join("world/region/new.mca"), "new chunk").unwrap();
        let restored = restore(&store, "test", &server, &snapshot.id).unwrap();
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "original-world"
        );
        assert!(!server.join("world/region/new.mca").exists());
        assert_eq!(
            fs::read_to_string(
                store
                    .join(restored.safety_backup.id)
                    .join("worlds/world/level.dat")
            )
            .unwrap(),
            "new progress"
        );
        assert_eq!(list(&store, "test").unwrap().len(), 2);
        assert!(!server.join(PENDING).exists());
    }
    #[test]
    fn rejects_corruption_before_changing_current_world() {
        let (store, server) = fixture();
        let snapshot = create(&store, "test", &server, "manual").unwrap();
        fs::write(
            store.join(&snapshot.id).join("worlds/world/level.dat"),
            "corrupted",
        )
        .unwrap();
        fs::write(server.join("world/level.dat"), "keep this").unwrap();
        assert!(restore(&store, "test", &server, &snapshot.id).is_err());
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "keep this"
        );
    }
    #[test]
    fn rejects_traversal_wrong_server_and_wrong_version() {
        let (store, server) = fixture();
        let snapshot = create(&store, "test", &server, "manual").unwrap();
        assert!(restore(&store, "test", &server, "../outside").is_err());
        assert!(restore(&store, "other", &server, &snapshot.id).is_err());
        let mut cfg = Config::load(&server).unwrap();
        cfg.minecraft = "26.3".into();
        fs::write(
            server.join("nethergate.json"),
            serde_json::to_vec(&cfg).unwrap(),
        )
        .unwrap();
        assert!(restore(&store, "test", &server, &snapshot.id).is_err());
        assert!(safe_relative("world/../../outside").is_err());
        assert!(safe_relative("world/C:/outside").is_err());
    }
    #[test]
    fn refuses_locked_worlds_and_interrupted_restores() {
        let (store, server) = fixture();
        let _guard = world_locks(&server, &["world".into()]).unwrap();
        assert!(create(&store, "test", &server, "manual").is_err());
        fs::write(server.join(PENDING), "{}").unwrap();
        assert!(ensure_no_pending(&server).is_err());
        let error =
            crate::process::prepare(&server, &Config::load(&server).unwrap(), false).unwrap_err();
        assert!(error.to_string().contains("restauro interrompido"));
    }

    #[cfg(windows)]
    #[test]
    fn rolls_back_partial_moves_when_windows_refuses_a_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let (store, server) = fixture();
        let snapshot = create(&store, "test", &server, "manual").unwrap();
        fs::write(server.join("world/level.dat"), "keep current progress").unwrap();
        // Permit reads (so the safety copy succeeds), but deny renaming/deletion.
        let _busy_file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(server.join("world_nether/region/r.0.0.mca"))
            .unwrap();
        let error = restore(&store, "test", &server, &snapshot.id).unwrap_err();
        assert!(
            error.to_string().contains("estado anterior foi reposto"),
            "{error:#}"
        );
        assert_eq!(
            fs::read_to_string(server.join("world/level.dat")).unwrap(),
            "keep current progress"
        );
        assert!(server.join("world_nether/region/r.0.0.mca").exists());
        assert!(!server.join(PENDING).exists());
        assert_eq!(list(&store, "test").unwrap().len(), 2);
    }
}
