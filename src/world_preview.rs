//! Read the original Minecraft world thumbnail when present; otherwise use the default cover.
#[cfg(test)]
mod tests;

use crate::backups::{component, ordinary, primary_world};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

// Version 1 contained generated isometric maps, which must never be reused here.
const FORMAT: u32 = 2;
const MAX_CACHE: u64 = 2 * 1024 * 1024;
const MAX_IMAGE: u64 = 1024 * 1024;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub revision: Option<String>,
    pub image: Option<String>,
    pub generating: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cache {
    format: u32,
    fingerprint: String,
    revision: String,
    image: String,
}
struct State {
    checked: Option<Instant>,
    preview: Preview,
}
#[derive(Default)]
pub struct Manager {
    states: Mutex<HashMap<String, State>>,
    busy: AtomicBool,
}

impl Manager {
    pub fn request(
        self: &Arc<Self>,
        store: PathBuf,
        id: &str,
        directory: PathBuf,
        known: Option<&str>,
    ) -> Result<Preview> {
        component(id)?;
        let mut states = self
            .states
            .lock()
            .map_err(|e| anyhow::anyhow!("Não foi possível ler as capas: {e}"))?;
        let state = states.entry(id.to_owned()).or_insert_with(|| State {
            checked: None,
            preview: Preview::default(),
        });
        let due = state
            .checked
            .is_none_or(|time| time.elapsed() >= Duration::from_secs(60));
        if due {
            state.preview.generating = true;
        }
        if due && !self.busy.swap(true, Ordering::AcqRel) {
            state.checked = Some(Instant::now());
            state.preview.generating = true;
            let manager = Arc::clone(self);
            let key = id.to_owned();
            let worker = std::thread::Builder::new()
                .name("world-preview".into())
                .spawn(move || {
                    struct Busy<'a>(&'a AtomicBool);
                    impl Drop for Busy<'_> {
                        fn drop(&mut self) {
                            self.0.store(false, Ordering::Release);
                        }
                    }
                    let _busy = Busy(&manager.busy);
                    let result = load_preview(&store, &key, &directory);
                    // A failed preview must never prevent normal server management.
                    let mut states = manager
                        .states
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(state) = states.get_mut(&key) {
                        // Start the cooldown after the check finishes.
                        state.checked = Some(Instant::now());
                        state.preview = result.unwrap_or_default();
                    }
                });
            if worker.is_err() {
                self.busy.store(false, Ordering::Release);
                state.preview.generating = false;
            }
        }
        let mut response = state.preview.clone();
        if known.is_some() && known == response.revision.as_deref() {
            response.image = None;
        }
        Ok(response)
    }
}

fn load_preview(store: &Path, id: &str, directory: &Path) -> Result<Preview> {
    component(id)?;
    let world = directory.join(primary_world(directory)?);
    ensure!(ordinary(&world)?.is_dir(), "O mundo ainda não existe");
    ensure!(
        !crate::backups::recovery_pending(directory)?,
        "Restauro pendente"
    );
    ensure!(
        ordinary(&world.join("level.dat"))?.is_file(),
        "O mundo ainda não foi guardado"
    );
    let icon = world.join("icon.png");
    let bytes = if icon.try_exists()? {
        let metadata = ordinary(&icon)?;
        let bytes = read_bounded(&icon, MAX_IMAGE)?;
        validate_png(&bytes)?;
        let current = ordinary(&icon)?;
        ensure!(
            metadata.len() == current.len() && metadata.modified()? == current.modified()?,
            "A miniatura está a ser atualizada"
        );
        bytes
    } else {
        return Ok(Preview::default());
    };
    let revision = format!("{:x}", Sha256::digest(&bytes));
    let signature = format!(
        "{:x}",
        Sha256::digest(format!("{FORMAT}:{:?}:{revision}", world.canonicalize()?))
    );
    if !store.exists() {
        fs::create_dir_all(store)?;
    }
    ensure!(ordinary(store)?.is_dir(), "Cache de capas inválida");
    let path = store.join(format!("{id}.json"));
    if path.exists() {
        ensure!(ordinary(&path)?.is_file(), "Cache de capa inválida");
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(MAX_CACHE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 <= MAX_CACHE {
            if let Ok(cache) = serde_json::from_slice::<Cache>(&bytes) {
                if cache.format == FORMAT
                    && cache.fingerprint == signature
                    && image_revision(&cache.image).as_deref() == Some(cache.revision.as_str())
                {
                    return Ok(Preview {
                        revision: Some(cache.revision),
                        image: Some(cache.image),
                        generating: false,
                    });
                }
            }
        }
    }
    ensure!(
        !crate::backups::recovery_pending(directory)?,
        "Restauro pendente"
    );
    let image = format!("data:image/png;base64,{}", STANDARD.encode(bytes));
    let cache = Cache {
        format: FORMAT,
        fingerprint: signature,
        revision: revision.clone(),
        image: image.clone(),
    };
    let temp = store.join(format!("{id}.json.tmp"));
    if temp.exists() {
        ensure!(ordinary(&temp)?.is_file(), "Cache temporária inválida");
    }
    crate::hub::replace_json(&path, &cache).context("Não foi possível guardar a capa")?;
    Ok(Preview {
        revision: Some(revision),
        image: Some(image),
        generating: false,
    })
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let meta = ordinary(path)?;
    ensure!(
        meta.is_file() && meta.len() <= limit,
        "Ficheiro de pré-visualização inválido ou demasiado grande"
    );
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Ficheiro de pré-visualização demasiado grande"
    );
    Ok(bytes)
}

fn image_revision(image: &str) -> Option<String> {
    let bytes = image
        .strip_prefix("data:image/png;base64,")
        .and_then(|data| STANDARD.decode(data).ok())?;
    if validate_png(&bytes).is_ok() {
        Some(format!("{:x}", Sha256::digest(bytes)))
    } else {
        None
    }
}

fn validate_png(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() >= 45 && bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "A miniatura não é PNG"
    );
    ensure!(
        bytes.get(8..16) == Some(&b"\0\0\0\rIHDR"[..]),
        "Cabeçalho PNG inválido"
    );
    let width = u32::from_be_bytes(bytes.get(16..20).context("PNG truncado")?.try_into()?);
    let height = u32::from_be_bytes(bytes.get(20..24).context("PNG truncado")?.try_into()?);
    ensure!(
        (1..=1024).contains(&width) && (1..=1024).contains(&height),
        "Dimensões da miniatura inválidas"
    );
    let mut offset = 8_usize;
    let mut has_data = false;
    while offset < bytes.len() {
        let size = u32::from_be_bytes(
            bytes
                .get(offset..offset + 4)
                .context("PNG truncado")?
                .try_into()?,
        ) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|v| v.checked_add(size))
            .context("Comprimento PNG inválido")?;
        let chunk = bytes.get(offset + 4..end).context("PNG truncado")?;
        let mut crc = !0_u32;
        for byte in &chunk[..chunk.len() - 4] {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320_u32 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        ensure!(
            u32::from_be_bytes(chunk[chunk.len() - 4..].try_into()?) == !crc,
            "Miniatura PNG corrompida"
        );
        if chunk.starts_with(b"IDAT") {
            has_data = true;
        }
        if chunk.starts_with(b"IEND") {
            ensure!(
                has_data && size == 0 && end == bytes.len(),
                "PNG incompleto"
            );
            return Ok(());
        }
        offset = end;
    }
    anyhow::bail!("Miniatura PNG incompleta")
}
