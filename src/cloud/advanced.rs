use super::*;

pub(super) fn decode_manifest(bytes: &[u8]) -> Result<(Vec<u8>, bool)> {
    let value: Value = serde_json::from_slice(bytes)?;
    if value["format"] == 3 {
        let archive = super::chunks::parse(bytes)?;
        return Ok((serde_json::to_vec(&archive.manifest)?, false));
    }
    if value["format"] == 2 {
        ensure!(
            value["compression"] == "gzip" && value["manifest"]["format"] == 1,
            "Formato cloud não suportado"
        );
        Ok((serde_json::to_vec(&value["manifest"])?, true))
    } else {
        ensure!(value["format"] == 1, "Formato cloud não suportado");
        Ok((bytes.into(), false))
    }
}

pub(super) struct CompressedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}
impl CompressedFile {
    pub fn new(
        source: &Path,
        expected_bytes: u64,
        expected_hash: &str,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("nethergate-cloud-{}.gz", crate::transfer::key()?));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut result = Self {
            path,
            bytes: 0,
            sha256: String::new(),
        };
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut input = fs::File::open(source)?;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            ensure!(!cancel.load(Ordering::SeqCst), "Compressão cancelada");
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            ensure!(
                count <= expected_bytes,
                "O backup mudou durante a compressão"
            );
            digest.update(&buffer[..n]);
            encoder.write_all(&buffer[..n])?;
        }
        ensure!(
            count == expected_bytes && format!("{:x}", digest.finalize()) == expected_hash,
            "O backup mudou durante a compressão"
        );
        encoder.finish()?.sync_all()?;
        result.bytes = fs::metadata(&result.path)?.len();
        result.sha256 = crate::paper::hash_file(&result.path)?;
        Ok(result)
    }
}
impl Drop for CompressedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPlan {
    pub fingerprint: String,
    pub remove: Vec<backups::BackupInfo>,
    pub keep: usize,
    #[serde(skip)]
    manifests: BTreeMap<String, String>,
}

fn archive_files(bytes: &[u8], id: &str) -> Result<Vec<backups::UploadFile>> {
    let (manifest, _) = decode_manifest(bytes)?;
    let value: Value = serde_json::from_slice(&manifest)?;
    let info: backups::BackupInfo = serde_json::from_value(value["info"].clone())?;
    ensure!(
        info.id == id && info.worlds.len() <= 256 && info.file_count <= 200_000,
        "Manifesto cloud inválido"
    );
    for world in &info.worlds {
        backups::component(world)?;
    }
    let rows = value["files"]
        .as_array()
        .context("Faltam ficheiros no manifesto")?;
    ensure!(
        rows.len() == info.file_count,
        "Contagem de ficheiros inválida"
    );
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    let mut files = vec![];
    for row in rows {
        let path = row["path"].as_str().context("Caminho inválido")?;
        for part in path.split('/') {
            backups::component(part)?;
        }
        ensure!(
            info.worlds
                .iter()
                .any(|w| path.starts_with(&format!("{w}/")))
                && seen.insert(path.to_lowercase()),
            "Caminho de mundo inválido ou duplicado"
        );
        let bytes = row["bytes"].as_u64().context("Tamanho inválido")?;
        total = total.checked_add(bytes).context("Tamanho inválido")?;
        let sha256 = row["sha256"].as_str().context("Hash inválido")?.to_string();
        ensure!(
            sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Hash inválido"
        );
        files.push(backups::UploadFile {
            path: PathBuf::new(),
            key: format!("worlds/{path}"),
            bytes,
            sha256,
        });
    }
    ensure!(
        total == info.size_bytes
            && info
                .worlds
                .iter()
                .all(|w| seen.contains(&format!("{w}/level.dat").to_lowercase())),
        "Conteúdo do manifesto inválido"
    );
    Ok(files)
}

pub fn retention_preview(root: &Path, id: &str, keep: usize) -> Result<Value> {
    let client = S3::new(load(root, id)?.context("Configura o destino cloud")?)?;
    Ok(serde_json::to_value(client.retention_plan(id, keep)?)?)
}
pub fn retention_apply(
    root: &Path,
    id: &str,
    keep: usize,
    fingerprint: &str,
    cancel: &AtomicBool,
) -> Result<Value> {
    let client = S3::new(load(root, id)?.context("Configura o destino cloud")?)?;
    let removed = client.apply_retention(id, fingerprint, keep, cancel)?;
    let mut current = state(root, id)?;
    current.retention_error = None;
    persist(root, id, &current)?;
    Ok(json!({"removed":removed}))
}

fn xml_text(bytes: &[u8], tag: &str) -> Result<String> {
    let doc = roxmltree::Document::parse(std::str::from_utf8(bytes)?)?;
    ensure!(
        !doc.descendants().any(|n| n.has_tag_name("Error")),
        "S3 devolveu um erro na resposta XML"
    );
    doc.descendants()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(str::to_string)
        .context("Resposta multipart inválida")
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn response_bytes(response: Response, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        response.status().is_success(),
        "Pedido S3 recusado (HTTP {})",
        response.status().as_u16()
    );
    let mut bytes = vec![];
    response.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "Resposta S3 demasiado grande");
    Ok(bytes)
}

impl S3 {
    pub(super) fn retention_plan(&self, server_id: &str, keep: usize) -> Result<RetentionPlan> {
        ensure!(
            (2..=50).contains(&keep),
            "Conserva entre 2 e 50 cópias automáticas"
        );
        let history = self.history_limit(1000, true)?;
        ensure!(
            history["limited"] == false,
            "Histórico demasiado grande para limpeza segura; usa o fornecedor"
        );
        let mut rows: Vec<backups::BackupInfo> =
            serde_json::from_value(history["backups"].clone())?;
        rows.retain(|b| b.reason == "automatic");
        rows.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        let mut digest = Sha256::new();
        digest.update(format!(
            "{}|{}|{}|{}",
            self.settings.endpoint, self.settings.bucket, self.settings.prefix, keep
        ));
        // Include every exact manifest, so changing the survivors invalidates the plan too.
        let mut manifests = BTreeMap::new();
        let mut remove = vec![];
        let mut active = 0;
        digest.update(server_id.as_bytes());
        for row in &rows {
            let bytes = self.bytes(
                &self.snapshot_key(&row.id, "manifest.json"),
                64 * 1024 * 1024,
            )?;
            let (decoded, _) = decode_manifest(&bytes)?;
            if serde_json::from_slice::<Value>(&decoded)?["serverId"] != server_id {
                continue;
            }
            if self.is_retired(&row.id)? {
                remove.push(row.clone());
            } else {
                active += 1;
                if active > keep {
                    remove.push(row.clone());
                }
            }
            archive_files(&bytes, &row.id)?;
            digest.update(&bytes);
            manifests.insert(row.id.clone(), hash(&bytes));
        }
        Ok(RetentionPlan {
            fingerprint: format!("{:x}", digest.finalize()),
            remove,
            keep,
            manifests,
        })
    }
    pub(super) fn apply_retention(
        &self,
        server_id: &str,
        expected: &str,
        keep: usize,
        cancel: &AtomicBool,
    ) -> Result<usize> {
        let plan = self.retention_plan(server_id, keep)?;
        ensure!(
            plan.fingerprint == expected,
            "O histórico mudou; revê a limpeza cloud"
        );
        // Verify retained automatic copies before removing any history.
        let history = self.history_limit(1000, true)?;
        let rows: Vec<backups::BackupInfo> = serde_json::from_value(history["backups"].clone())?;
        for row in rows
            .iter()
            .filter(|r| r.reason == "automatic" && !plan.remove.iter().any(|d| d.id == r.id))
        {
            ensure!(!cancel.load(Ordering::SeqCst), "Limpeza cancelada");
            let bytes = self.bytes(
                &self.snapshot_key(&row.id, "manifest.json"),
                64 * 1024 * 1024,
            )?;
            let (decoded, compressed) = decode_manifest(&bytes)?;
            if serde_json::from_slice::<Value>(&decoded)?["serverId"] != server_id {
                continue;
            }
            if serde_json::from_slice::<Value>(&bytes)?["format"] == 3 {
                super::chunks::verify(self, &bytes, cancel)?;
                continue;
            }
            for file in archive_files(&bytes, &row.id)? {
                self.verify_archive_file(&row.id, &file, compressed)?;
            }
        }
        let mut removed = 0;
        for row in &plan.remove {
            ensure!(!cancel.load(Ordering::SeqCst), "Limpeza cancelada");
            let key = self.snapshot_key(&row.id, "manifest.json");
            let response = self.get(&key)?;
            let etag = response
                .headers()
                .get("etag")
                .context("O fornecedor deve disponibilizar ETag para limpeza segura")?
                .to_str()?
                .to_string();
            let bytes = response_bytes(response, 64 * 1024 * 1024)?;
            ensure!(
                plan.manifests.get(&row.id) == Some(&hash(&bytes)),
                "O manifesto mudou durante a limpeza"
            );
            let files = archive_files(&bytes, &row.id)?;
            let (_, compressed) = decode_manifest(&bytes)?;
            // Tombstone first: interrupted deletion is hidden and can never be re-uploaded.
            let retired = serde_json::to_vec(
                &json!({"format":1,"id":row.id,"manifest":serde_json::from_slice::<Value>(&bytes)?}),
            )?;
            self.put(
                &self.snapshot_key(&row.id, "retired.json"),
                Body::from(retired.clone()),
                retired.len() as u64,
                &hash(&retired),
            )?;
            for file in files {
                // Shared blocks remain immutable; only the snapshot reference is retired.
                if serde_json::from_slice::<Value>(&bytes)?["format"] == 3 {
                    break;
                }
                ensure!(
                    !cancel.load(Ordering::SeqCst),
                    "Limpeza interrompida; dados restantes podem ser limpos no fornecedor"
                );
                self.delete_object(
                    &self.snapshot_key(
                        &row.id,
                        &if compressed {
                            format!("{}.gz", file.key)
                        } else {
                            file.key
                        },
                    ),
                    None,
                )?;
            }
            self.delete_object(&key, Some(etag))?;
            removed += 1;
        }
        Ok(removed)
    }
    fn delete_object(&self, key: &str, etag: Option<String>) -> Result<()> {
        let headers = etag
            .map(|v| BTreeMap::from([("if-match".into(), v)]))
            .unwrap_or_default();
        let response = self.request(Method::DELETE, Some(key), &[], None, &hash(&[]), headers)?;
        ensure!(
            response.status().is_success() || response.status().as_u16() == 404,
            "Limpeza S3 recusada (HTTP {})",
            response.status().as_u16()
        );
        Ok(())
    }
    pub(super) fn verify_archive_file(
        &self,
        id: &str,
        file: &backups::UploadFile,
        compressed: bool,
    ) -> Result<()> {
        let key = self.snapshot_key(
            id,
            &if compressed {
                format!("{}.gz", file.key)
            } else {
                file.key.clone()
            },
        );
        if !compressed {
            return self.verify(&key, file.bytes, &file.sha256);
        }
        let mut reader =
            flate2::read::GzDecoder::new(self.get(&key)?).take(file.bytes.saturating_add(1));
        let mut digest = Sha256::new();
        let mut count = 0;
        let mut buffer = [0u8; 65536];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            digest.update(&buffer[..n]);
        }
        ensure!(
            count == file.bytes && format!("{:x}", digest.finalize()) == file.sha256,
            "Ficheiro comprimido corrompido"
        );
        Ok(())
    }
    pub(super) fn put_file(
        &self,
        key: &str,
        path: &Path,
        size: u64,
        expected: &str,
        cancel: &AtomicBool,
    ) -> Result<()> {
        if size < 16 * 1024 * 1024 {
            return self.put(
                key,
                Body::sized(fs::File::open(path)?.take(size), size),
                size,
                expected,
            );
        }
        self.multipart(key, path, size, expected, cancel, 8 * 1024 * 1024)
    }
    pub(super) fn multipart(
        &self,
        key: &str,
        path: &Path,
        size: u64,
        expected: &str,
        cancel: &AtomicBool,
        part_size: usize,
    ) -> Result<()> {
        ensure!(
            size.div_ceil(part_size as u64) <= 10000,
            "O ficheiro excede o limite de partes multipart"
        );
        let existing = self.request(
            Method::GET,
            Some(key),
            &[],
            None,
            &hash(&[]),
            BTreeMap::new(),
        )?;
        if existing.status().is_success() {
            return Self::verify_response(existing, size, expected);
        }
        ensure!(
            existing.status().as_u16() == 404,
            "Não foi possível verificar o objeto antes do multipart"
        );
        ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
        let started = response_bytes(
            self.request(
                Method::POST,
                Some(key),
                &[("uploads".into(), "".into())],
                None,
                &hash(&[]),
                BTreeMap::from([("x-amz-meta-nethergate-sha256".into(), expected.into())]),
            )?,
            65536,
        )?;
        let upload = xml_text(&started, "UploadId")?;
        ensure!(
            upload.len() <= 4096 && !upload.is_empty(),
            "UploadId inválido"
        );
        let result = (|| -> Result<()> {
            let mut input = fs::File::open(path)?;
            let mut remaining = size;
            let mut parts = String::new();
            let mut number = 1;
            while remaining > 0 {
                ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
                let mut bytes = vec![0; remaining.min(part_size as u64) as usize];
                input.read_exact(&mut bytes)?;
                remaining -= bytes.len() as u64;
                let payload = hash(&bytes);
                let response = self.request(
                    Method::PUT,
                    Some(key),
                    &[
                        ("partNumber".into(), number.to_string()),
                        ("uploadId".into(), upload.clone()),
                    ],
                    Some(Body::from(bytes)),
                    &payload,
                    BTreeMap::new(),
                )?;
                ensure!(
                    response.status().is_success(),
                    "Falha ao enviar parte {number}"
                );
                let etag = response
                    .headers()
                    .get("etag")
                    .context("Parte S3 sem ETag")?
                    .to_str()?;
                ensure!(etag.len() <= 256, "ETag inválido");
                parts.push_str(&format!(
                    "<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>",
                    escape(etag)
                ));
                number += 1;
            }
            ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
            let body =
                format!("<CompleteMultipartUpload>{parts}</CompleteMultipartUpload>").into_bytes();
            let digest = hash(&body);
            let response = self.request(
                Method::POST,
                Some(key),
                &[("uploadId".into(), upload.clone())],
                Some(Body::from(body)),
                &digest,
                BTreeMap::from([
                    ("if-none-match".into(), "*".into()),
                    ("content-type".into(), "application/xml".into()),
                ]),
            )?;
            if response.status().as_u16() == 412 {
                self.verify(key, size, expected)?;
                let cleanup = self.request(
                    Method::DELETE,
                    Some(key),
                    &[("uploadId".into(), upload.clone())],
                    None,
                    &hash(&[]),
                    BTreeMap::new(),
                )?;
                ensure!(
                    cleanup.status().is_success() || cleanup.status().as_u16() == 404,
                    "O objeto foi confirmado, mas ficaram partes multipart por limpar"
                );
                return Ok(());
            }
            let bytes = response_bytes(response, 65536)?;
            xml_text(&bytes, "ETag")?; // HTTP 200 can still contain an embedded S3 Error.
            self.verify(key, size, expected)
        })();
        if let Err(error) = result {
            let cleanup = self.request(
                Method::DELETE,
                Some(key),
                &[("uploadId".into(), upload)],
                None,
                &hash(&[]),
                BTreeMap::new(),
            );
            if !cleanup.is_ok_and(|r| r.status().is_success() || r.status().as_u16() == 404) {
                return Err(error.context("Não foi possível cancelar o multipart remoto; revê uploads incompletos no fornecedor"));
            }
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn is_retired(&self, id: &str) -> Result<bool> {
        let response = self.request(
            Method::GET,
            Some(&self.snapshot_key(id, "retired.json")),
            &[],
            None,
            &hash(&[]),
            BTreeMap::new(),
        )?;
        ensure!(
            response.status().is_success() || response.status().as_u16() == 404,
            "Não foi possível verificar o estado de retenção"
        );
        Ok(response.status().is_success())
    }
    pub(super) fn ensure_not_retired(&self, id: &str) -> Result<()> {
        ensure!(
            !self.is_retired(id)?,
            "Esta cópia foi retirada pela retenção cloud; cria um novo backup para enviar"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compression_refuses_modified_input_and_cancellation() {
        let path = std::env::temp_dir().join(format!(
            "ng-compression-{}",
            crate::transfer::key().unwrap()
        ));
        fs::write(&path, b"changed snapshot").unwrap();
        assert!(CompressedFile::new(
            &path,
            16,
            &hash(b"original content"),
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(CompressedFile::new(
            &path,
            16,
            &hash(b"changed snapshot"),
            &AtomicBool::new(true)
        )
        .is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn multipart_http_success_with_embedded_error_is_not_a_commit() {
        assert!(xml_text(
            b"<Error><Code>InternalError</Code><ETag>not-a-commit</ETag></Error>",
            "ETag"
        )
        .is_err());
        assert!(xml_text(b"<CompleteMultipartUploadResult/>", "ETag").is_err());
    }
}
