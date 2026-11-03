//! Immutable content-addressed blocks: reuse is verified from bytes, not metadata.
use super::*;
pub(crate) const BLOCK_SIZE: usize = 4 * 1024 * 1024;
pub(crate) const MANIFEST_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) trait Remote {
    fn read(&self, key: &str, limit: usize) -> Result<Option<Vec<u8>>>;
    /// Must not overwrite an existing, different object.
    fn write(&self, key: &str, bytes: Vec<u8>) -> Result<()>;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Block {
    sha256: String,
    bytes: usize,
    stored_bytes: usize,
    gzip: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Archive {
    format: u32,
    pub(crate) manifest: Value,
    blocks: BTreeMap<String, Vec<Block>>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    pub files: usize,
    pub total_files: usize,
    pub sent_blocks: usize,
    pub reused_blocks: usize,
    pub sent_bytes: u64,
}
fn block_key(hash: &str) -> String {
    format!("objects/sha256/{hash}")
}
pub(crate) fn snapshot_key(id: &str) -> String {
    format!("{id}/manifest.json")
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Archive> {
    ensure!(
        bytes.len() <= MANIFEST_LIMIT,
        "Manifesto cloud demasiado grande"
    );
    let archive: Archive = serde_json::from_slice(bytes)?;
    ensure!(
        archive.format == 3 && archive.manifest["format"] == 1,
        "Formato cloud não suportado"
    );
    let files = archive.manifest["files"]
        .as_array()
        .context("Manifesto sem ficheiros")?;
    ensure!(
        files.len() <= 200_000 && archive.blocks.len() == files.len(),
        "Índice de blocos inválido"
    );
    let mut seen = std::collections::HashSet::new();
    for file in files {
        let path = file["path"].as_str().context("Caminho inválido")?;
        for part in path.split('/') {
            backups::component(part)?;
        }
        ensure!(seen.insert(path.to_lowercase()), "Caminho duplicado");
        let blocks = archive
            .blocks
            .get(path)
            .context("Faltam blocos de um ficheiro")?;
        let mut total = 0u64;
        for block in blocks {
            ensure!(
                block.bytes > 0
                    && block.bytes <= BLOCK_SIZE
                    && block.stored_bytes > 0
                    && block.stored_bytes <= BLOCK_SIZE + 65536,
                "Tamanho de bloco inválido"
            );
            ensure!(
                block.sha256.len() == 64 && block.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "Hash de bloco inválido"
            );
            total = total
                .checked_add(block.bytes as u64)
                .context("Tamanho inválido")?;
        }
        ensure!(Some(total) == file["bytes"].as_u64(), "Blocos incompletos");
    }
    Ok(archive)
}

fn read_block(remote: &impl Remote, block: &Block) -> Result<Vec<u8>> {
    let stored = remote
        .read(&block_key(&block.sha256), block.stored_bytes)?
        .context("Bloco cloud em falta")?;
    ensure!(
        stored.len() == block.stored_bytes && hash(&stored) == block.sha256,
        "Bloco cloud corrompido"
    );
    if !block.gzip {
        ensure!(stored.len() == block.bytes, "Tamanho de bloco inválido");
        return Ok(stored);
    }
    let mut decoded = vec![];
    flate2::read::GzDecoder::new(stored.as_slice())
        .take(block.bytes as u64 + 1)
        .read_to_end(&mut decoded)?;
    ensure!(decoded.len() == block.bytes, "Bloco comprimido inválido");
    Ok(decoded)
}

pub(crate) fn verify(remote: &impl Remote, bytes: &[u8], cancel: &AtomicBool) -> Result<()> {
    let archive = parse(bytes)?;
    for file in archive.manifest["files"]
        .as_array()
        .context("O manifesto cloud não contém uma lista de ficheiros")?
    {
        let path = file["path"]
            .as_str()
            .context("O manifesto cloud contém um ficheiro sem caminho válido")?;
        let mut digest = Sha256::new();
        for block in archive
            .blocks
            .get(path)
            .context("Faltam blocos do ficheiro no manifesto cloud")?
        {
            ensure!(!cancel.load(Ordering::SeqCst), "Verificação cancelada");
            digest.update(read_block(remote, block)?);
        }
        ensure!(
            Some(format!("{:x}", digest.finalize()).as_str()) == file["sha256"].as_str(),
            "Ficheiro cloud corrompido"
        );
    }
    Ok(())
}

pub(crate) fn upload(
    remote: &impl Remote,
    store: &Path,
    server_id: &str,
    id: &str,
    compress: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&Progress) -> Result<()>,
) -> Result<()> {
    let (manifest, files) = backups::cloud_snapshot(store, server_id, id)?;
    let metadata: Value = serde_json::from_slice(&manifest)?;
    if let Some(existing) = remote.read(&snapshot_key(id), MANIFEST_LIMIT)? {
        let existing_archive = parse(&existing)?;
        let reused = existing_archive.blocks.values().map(Vec::len).sum();
        let mut old = existing_archive.manifest;
        old["serverId"] = metadata["serverId"].clone();
        old["info"]["reason"] = metadata["info"]["reason"].clone();
        ensure!(old == metadata, "Já existe outro backup com este ID");
        verify(remote, &existing, cancel)?;
        return progress(&Progress {
            files: files.len() + 1,
            total_files: files.len() + 1,
            reused_blocks: reused,
            ..Default::default()
        });
    }
    let mut state = Progress {
        total_files: files.len() + 1,
        ..Default::default()
    };
    progress(&state)?;
    let mut archive = Archive {
        format: 3,
        manifest: metadata,
        blocks: BTreeMap::new(),
    };
    for file in files {
        backups::ordinary(&file.path)?;
        let mut input = fs::File::open(&file.path)?;
        let mut remaining = file.bytes;
        let mut digest = Sha256::new();
        let mut blocks = vec![];
        while remaining > 0 {
            ensure!(
                !cancel.load(Ordering::SeqCst),
                "Envio interrompido; os blocos confirmados serão reutilizados"
            );
            let mut data = vec![0; remaining.min(BLOCK_SIZE as u64) as usize];
            input.read_exact(&mut data)?;
            remaining -= data.len() as u64;
            digest.update(&data);
            let bytes = data.len();
            let mut gzip = false;
            if compress {
                let mut encoder =
                    flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
                encoder.write_all(&data)?;
                let compressed = encoder.finish()?;
                if compressed.len() < data.len() {
                    data = compressed;
                    gzip = true;
                }
            }
            let sha256 = hash(&data);
            let key = block_key(&sha256);
            let stored_bytes = data.len();
            if let Some(existing) = remote.read(&key, stored_bytes)? {
                ensure!(
                    existing == data,
                    "Bloco existente corrompido; não foi substituído"
                );
                state.reused_blocks += 1;
            } else {
                remote.write(&key, data)?;
                let returned = remote
                    .read(&key, stored_bytes)?
                    .context("Bloco enviado não encontrado")?;
                ensure!(
                    returned.len() == stored_bytes && hash(&returned) == sha256,
                    "Verificação do bloco enviado falhou"
                );
                state.sent_blocks += 1;
                state.sent_bytes += stored_bytes as u64;
            }
            blocks.push(Block {
                sha256,
                bytes,
                stored_bytes,
                gzip,
            });
            progress(&state)?;
        }
        let mut extra = [0];
        ensure!(
            input.read(&mut extra)? == 0 && format!("{:x}", digest.finalize()) == file.sha256,
            "O backup mudou durante o envio"
        );
        archive.blocks.insert(
            file.key
                .strip_prefix("worlds/")
                .context("Caminho de snapshot inválido")?
                .into(),
            blocks,
        );
        state.files += 1;
        progress(&state)?;
    }
    ensure!(
        !cancel.load(Ordering::SeqCst),
        "Envio interrompido; podes retomar"
    );
    let bytes = serde_json::to_vec(&archive)?;
    parse(&bytes)?;
    remote.write(&snapshot_key(id), bytes.clone())?;
    ensure!(
        remote.read(&snapshot_key(id), MANIFEST_LIMIT)? == Some(bytes),
        "O manifesto publicado não corresponde ao backup"
    );
    state.files += 1;
    progress(&state)?;
    Ok(())
}

pub(crate) fn download(
    remote: &impl Remote,
    store: &Path,
    server_id: &str,
    id: &str,
    bytes: &[u8],
    cancel: &AtomicBool,
) -> Result<backups::BackupInfo> {
    let archive = parse(bytes)?;
    ensure!(
        archive.manifest["info"]["id"] == id,
        "Identificador cloud diferente"
    );
    backups::receive_cloud(
        store,
        server_id,
        id,
        &serde_json::to_vec(&archive.manifest)?,
        |file, to, size, expected| {
            let path = file.strip_prefix("worlds/").context("Caminho inválido")?;
            let mut output = OpenOptions::new().write(true).create_new(true).open(to)?;
            let mut digest = Sha256::new();
            let mut count = 0u64;
            for block in archive.blocks.get(path).context("Faltam blocos")? {
                ensure!(!cancel.load(Ordering::SeqCst), "Transferência cancelada");
                let data = read_block(remote, block)?;
                count += data.len() as u64;
                ensure!(count <= size, "Ficheiro demasiado grande");
                digest.update(&data);
                output.write_all(&data)?;
            }
            output.sync_all()?;
            ensure!(
                count == size && format!("{:x}", digest.finalize()) == expected,
                "Ficheiro recuperado corrompido"
            );
            Ok(())
        },
    )
}

impl Remote for S3 {
    fn read(&self, key: &str, limit: usize) -> Result<Option<Vec<u8>>> {
        let response = self.request(
            Method::GET,
            Some(&format!("{}/{key}", self.settings.prefix)),
            &[],
            None,
            &hash(&[]),
            BTreeMap::new(),
        )?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        ensure!(
            response.status().is_success(),
            "Leitura S3 recusada (HTTP {})",
            response.status().as_u16()
        );
        let mut bytes = vec![];
        response.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= limit, "Resposta cloud demasiado grande");
        Ok(Some(bytes))
    }
    fn write(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        let digest = hash(&bytes);
        self.put(
            &format!("{}/{key}", self.settings.prefix),
            Body::from(bytes.clone()),
            bytes.len() as u64,
            &digest,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Memory {
        objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
        writes: Arc<Mutex<Vec<String>>>,
    }
    impl Remote for Memory {
        fn read(&self, key: &str, limit: usize) -> Result<Option<Vec<u8>>> {
            let value = self.objects.lock().unwrap().get(key).cloned();
            ensure!(
                value.as_ref().is_none_or(|v| v.len() <= limit),
                "Resposta demasiado grande"
            );
            Ok(value)
        }
        fn write(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
            let mut objects = self.objects.lock().unwrap();
            if let Some(old) = objects.get(key) {
                ensure!(old == &bytes, "Conflito");
            } else {
                objects.insert(key.into(), bytes);
                self.writes.lock().unwrap().push(key.into());
            }
            Ok(())
        }
    }
    fn fixture() -> (PathBuf, PathBuf, PathBuf, backups::BackupInfo) {
        let root =
            std::env::temp_dir().join(format!("ng-chunks-{}", crate::transfer::key().unwrap()));
        let server = root.join("server");
        fs::create_dir_all(server.join("world/region")).unwrap();
        crate::config::write_json(
            &server.join("nethergate.json"),
            &crate::config::Config::new("1.21.11".into(), "https://example.org".into(), 21)
                .unwrap(),
        )
        .unwrap();
        fs::write(server.join("server.properties"), "level-name=world\n").unwrap();
        fs::write(server.join("world/level.dat"), b"world metadata").unwrap();
        let mut data = vec![1; BLOCK_SIZE];
        data.extend(vec![2; BLOCK_SIZE]);
        data.extend(vec![3; 256]);
        fs::write(server.join("world/region/r.0.0.mca"), data).unwrap();
        let store = root.join("backups");
        let backup = backups::create(&store, "source", &server, "automatic").unwrap();
        (root, store, server, backup)
    }
    #[test]
    fn restart_resumes_at_blocks_and_new_backup_deduplicates() {
        let (root, store, server, backup) = fixture();
        let remote = Memory::default();
        let cancel = AtomicBool::new(false);
        assert!(
            upload(&remote, &store, "source", &backup.id, false, &cancel, |p| {
                if p.sent_blocks == 2 {
                    cancel.store(true, Ordering::SeqCst)
                }
                Ok(())
            })
            .is_err()
        );
        assert!(!remote
            .objects
            .lock()
            .unwrap()
            .contains_key(&snapshot_key(&backup.id)));
        // A fresh client contains no session state; only verified remote blocks survive.
        let fresh = Memory {
            objects: remote.objects.clone(),
            writes: remote.writes.clone(),
        };
        let mut reused = 0;
        upload(
            &fresh,
            &store,
            "source",
            &backup.id,
            false,
            &AtomicBool::new(false),
            |p| {
                reused = p.reused_blocks;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(reused, 2);
        let before = fresh.writes.lock().unwrap().len();
        let next = backups::create(&store, "source", &server, "automatic").unwrap();
        upload(
            &fresh,
            &store,
            "source",
            &next.id,
            false,
            &AtomicBool::new(false),
            |p| {
                reused = p.reused_blocks;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(reused, 4);
        assert_eq!(fresh.writes.lock().unwrap().len(), before + 1);
        let bytes = fresh
            .read(&snapshot_key(&backup.id), MANIFEST_LIMIT)
            .unwrap()
            .unwrap();
        download(
            &fresh,
            &root.join("received"),
            "target",
            &backup.id,
            &bytes,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            fs::read(
                root.join("received")
                    .join(&backup.id)
                    .join("worlds/world/region/r.0.0.mca")
            )
            .unwrap(),
            fs::read(server.join("world/region/r.0.0.mca")).unwrap()
        );
    }
    #[test]
    fn compressed_blocks_reject_corruption_and_invalid_indexes() {
        let (root, store, _, backup) = fixture();
        let remote = Memory::default();
        let cancel = AtomicBool::new(false);
        upload(&remote, &store, "source", &backup.id, true, &cancel, |_| {
            Ok(())
        })
        .unwrap();
        let bytes = remote
            .read(&snapshot_key(&backup.id), MANIFEST_LIMIT)
            .unwrap()
            .unwrap();
        verify(&remote, &bytes, &cancel).unwrap();
        let mut malformed: Value = serde_json::from_slice(&bytes).unwrap();
        malformed["blocks"]["world/level.dat"][0]["bytes"] = json!(BLOCK_SIZE + 1);
        assert!(parse(&serde_json::to_vec(&malformed).unwrap()).is_err());
        let archive = parse(&bytes).unwrap();
        let key = block_key(&archive.blocks["world/level.dat"][0].sha256);
        remote.objects.lock().unwrap().get_mut(&key).unwrap()[0] ^= 1;
        assert!(verify(&remote, &bytes, &cancel).is_err());
        assert!(download(
            &remote,
            &root.join("corrupt"),
            "target",
            &backup.id,
            &bytes,
            &cancel
        )
        .is_err());
        assert!(!root.join("corrupt").join(&backup.id).exists());
    }
}
