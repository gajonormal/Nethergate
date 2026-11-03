//! S3-compatible, signed, read-back verified snapshots. Credentials remain in DPAPI.
pub mod accounts;
mod advanced;
pub(crate) mod chunks;
pub mod services;
use crate::{backup_schedule, backups};
pub use advanced::{retention_apply, retention_preview};
use anyhow::{ensure, Context, Result};
use hmac::{Hmac, Mac};
use reqwest::{
    blocking::{Body, Client, Response},
    Method,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
    #[serde(default)]
    pub session_token: String,
    #[serde(default)]
    pub automatic: bool,
    #[serde(default)]
    pub compression: bool,
    #[serde(default)]
    pub deduplication: bool,
    #[serde(default)]
    pub retention_keep: usize,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    #[serde(default)]
    pub pending_backup: Option<String>,
    #[serde(default)]
    pub uploaded: Vec<String>,
    pub last_success: Option<u64>,
    pub last_backup_id: Option<String>,
    pub error: Option<String>,
    pub retry_at: Option<u64>,
    pub completed_files: usize,
    pub total_files: usize,
    #[serde(default)]
    pub retention_error: Option<String>,
    #[serde(default)]
    pub sent_blocks: usize,
    #[serde(default)]
    pub reused_blocks: usize,
}
fn paths(root: &Path, id: &str) -> Result<(PathBuf, PathBuf)> {
    backups::component(id)?;
    let folder = root.join("hub-data/cloud").join(id);
    Ok((folder.join("settings.protected"), folder.join("state.json")))
}
fn validate(value: &Settings) -> Result<()> {
    ensure!(
        value.retention_keep == 0 || (2..=50).contains(&value.retention_keep),
        "Retenção cloud: escolhe 2 a 50 cópias, ou 0 para desligar"
    );
    let url = reqwest::Url::parse(&value.endpoint)?;
    ensure!(
        (url.scheme() == "https"
            || (cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "O endpoint S3 deve ser uma origem HTTPS sem caminho, credenciais ou parâmetros"
    );
    ensure!(
        !value.region.is_empty()
            && value.region.len() <= 64
            && value
                .region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "Região S3 inválida"
    );
    ensure!(
        (3..=63).contains(&value.bucket.len())
            && value
                .bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
            && !value.bucket.contains("..")
            && value
                .bucket
                .starts_with(|c: char| c.is_ascii_alphanumeric())
            && value.bucket.ends_with(|c: char| c.is_ascii_alphanumeric()),
        "Nome de bucket inválido"
    );
    ensure!(
        !value.prefix.is_empty() && value.prefix.len() <= 256,
        "Escolhe um prefixo exclusivo para este servidor"
    );
    for part in value.prefix.split('/') {
        backups::component(part)?;
    }
    ensure!(
        !value.access_key.is_empty()
            && value.access_key.len() <= 256
            && value
                .access_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "Access key inválida"
    );
    ensure!(
        !value.secret_key.is_empty()
            && value.secret_key.len() <= 4096
            && !value.secret_key.chars().any(char::is_control)
            && value.session_token.len() <= 8192
            && value.session_token.bytes().all(|b| (33..=126).contains(&b)),
        "Credenciais S3 inválidas"
    );
    Ok(())
}
pub fn load(root: &Path, id: &str) -> Result<Option<Settings>> {
    let (path, _) = paths(root, id)?;
    if !path.exists() {
        return Ok(None);
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 32768,
        "Configuração cloud inválida"
    );
    let value: Settings =
        serde_json::from_slice(&crate::secrets::protect(&fs::read(path)?, true)?)?;
    validate(&value)?;
    Ok(Some(value))
}
pub fn state(root: &Path, id: &str) -> Result<State> {
    let (_, path) = paths(root, id)?;
    if !path.exists() {
        return Ok(State::default());
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 4 * 1024 * 1024,
        "Estado cloud inválido"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn persist(root: &Path, id: &str, value: &State) -> Result<()> {
    let (_, path) = paths(root, id)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração cloud não tem pasta pai")?,
    )?;
    crate::hub::replace_json(&path, value)
}
pub fn status(root: &Path, id: &str) -> Result<Value> {
    let settings = load(root, id)?;
    let current = state(root, id)?;
    Ok(
        json!({"configured":settings.is_some(),"settings":settings.map(|v|json!({"endpoint":v.endpoint,"region":v.region,"bucket":v.bucket,"prefix":v.prefix,"automatic":v.automatic,"compression":v.compression,"deduplication":v.deduplication,"retentionKeep":v.retention_keep})),"state":{"uploadedCount":current.uploaded.len(),"lastSuccess":current.last_success,"lastBackupId":current.last_backup_id,"error":current.error,"retryAt":current.retry_at,"completedFiles":current.completed_files,"totalFiles":current.total_files,"retentionError":current.retention_error,"sentBlocks":current.sent_blocks,"reusedBlocks":current.reused_blocks,"pendingBackup":current.pending_backup}}),
    )
}
pub fn save(root: &Path, id: &str, mut value: Settings) -> Result<Value> {
    let previous = load(root, id)?;
    value.endpoint = value.endpoint.trim_end_matches('/').into();
    value.prefix = value.prefix.trim_matches('/').into();
    if value.access_key.is_empty() && value.secret_key.is_empty() {
        let old = previous.as_ref().context("Introduz as credenciais S3")?;
        ensure!(
            old.endpoint == value.endpoint,
            "Volta a introduzir as credenciais ao mudar de endpoint"
        );
        value.access_key = old.access_key.clone();
        value.secret_key = old.secret_key.clone();
        value.session_token = old.session_token.clone();
    }
    validate(&value)?;
    let bytes = crate::secrets::protect(&serde_json::to_vec(&value)?, false)?;
    let (path, _) = paths(root, id)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração cloud não tem pasta pai")?,
    )?;
    if previous.is_none_or(|p| {
        p.endpoint != value.endpoint || p.bucket != value.bucket || p.prefix != value.prefix
    }) {
        persist(root, id, &State::default())?;
    }
    let staging = path.with_extension("protected.tmp");
    let mut file = fs::File::create(&staging)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(staging, path)?;
    status(root, id)
}
pub fn pause(root: &Path, id: &str) -> Result<Value> {
    let mut value = load(root, id)?.context("Configura primeiro o destino cloud")?;
    value.automatic = false;
    save(root, id, value)
}
fn encoded(text: &str, slash: bool) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    // Normalize the key to the SHA-256 block size, then use the infallible
    // fixed-size constructor (RFC 2104), preserving arbitrary key support.
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut mac = <Hmac<Sha256> as hmac::digest::KeyInit>::new(&block.into());
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}
fn signature(
    settings: &Settings,
    method: &str,
    uri: &str,
    query: &str,
    date: &str,
    payload: &str,
    headers: &mut BTreeMap<String, String>,
) -> Result<String> {
    ensure!(date.len() == 16, "Data inválida");
    headers.insert("x-amz-date".into(), date.into());
    headers.insert("x-amz-content-sha256".into(), payload.into());
    if !settings.session_token.is_empty() {
        headers.insert(
            "x-amz-security-token".into(),
            settings.session_token.clone(),
        );
    }
    let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical = headers
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}:{}\n",
                v.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        })
        .collect::<String>();
    let request = format!("{method}\n{uri}\n{query}\n{canonical}\n{signed}\n{payload}");
    let scope = format!("{}/{}/s3/aws4_request", &date[..8], settings.region);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hash(request.as_bytes())
    );
    let key = hmac(
        format!("AWS4{}", settings.secret_key).as_bytes(),
        &date.as_bytes()[..8],
    );
    let key = hmac(&key, settings.region.as_bytes());
    let key = hmac(&key, b"s3");
    let key = hmac(&key, b"aws4_request");
    let signature = hmac(&key, to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok(format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
        settings.access_key
    ))
}
struct S3 {
    settings: Settings,
    http: Client,
}
impl S3 {
    fn new(settings: Settings) -> Result<Self> {
        validate(&settings)?;
        Ok(Self {
            settings,
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(180))
                .build()?,
        })
    }
    fn request(
        &self,
        method: Method,
        key: Option<&str>,
        query: &[(String, String)],
        body: Option<Body>,
        payload: &str,
        mut headers: BTreeMap<String, String>,
    ) -> Result<Response> {
        let uri = format!(
            "/{}/{}",
            encoded(&self.settings.bucket, false),
            key.map(|s| encoded(s, true)).unwrap_or_default()
        );
        let mut query = query
            .iter()
            .map(|(k, v)| (encoded(k, false), encoded(v, false)))
            .collect::<Vec<_>>();
        query.sort();
        let query = query
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let url = reqwest::Url::parse(&format!(
            "{}{}{}{}",
            self.settings.endpoint.trim_end_matches('/'),
            uri,
            if query.is_empty() { "" } else { "?" },
            query
        ))?;
        let host = match url.port() {
            Some(port) => format!(
                "{}:{port}",
                url.host_str()
                    .context("O endpoint S3 não contém um hostname válido")?
            ),
            None => url
                .host_str()
                .context("O endpoint S3 não contém um hostname válido")?
                .into(),
        };
        headers.insert("host".into(), host);
        let now = time::OffsetDateTime::now_utc();
        let date = format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        );
        let auth = signature(
            &self.settings,
            method.as_str(),
            &uri,
            &query,
            &date,
            payload,
            &mut headers,
        )?;
        let mut request = self.http.request(method, url).header("Authorization", auth);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        request
            .send()
            .context("Não foi possível contactar o armazenamento S3")
    }
    fn get(&self, key: &str) -> Result<Response> {
        let response = self.request(
            Method::GET,
            Some(key),
            &[],
            None,
            &hash(&[]),
            BTreeMap::new(),
        )?;
        ensure!(
            response.status().is_success(),
            "Leitura S3 recusada (HTTP {})",
            response.status().as_u16()
        );
        Ok(response)
    }
    fn bytes(&self, key: &str, limit: u64) -> Result<Vec<u8>> {
        let mut bytes = vec![];
        self.get(key)?.take(limit + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= limit, "Resposta S3 demasiado grande");
        Ok(bytes)
    }
    fn verify(&self, key: &str, size: u64, expected: &str) -> Result<()> {
        Self::verify_response(self.get(key)?, size, expected)
    }
    fn verify_response(response: Response, size: u64, expected: &str) -> Result<()> {
        let mut response = response.take(size.saturating_add(1));
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            digest.update(&buffer[..n]);
        }
        ensure!(
            bytes == size && format!("{:x}", digest.finalize()) == expected,
            "A verificação do objeto cloud falhou"
        );
        Ok(())
    }
    fn put(&self, key: &str, body: Body, size: u64, expected: &str) -> Result<()> {
        ensure!(
            size <= 5 * 1024 * 1024 * 1024,
            "Um ficheiro excede o limite de 5 GiB deste fornecedor de upload"
        );
        // Resume at file boundaries. Never trust metadata alone or overwrite an
        // existing object: verify its actual bytes before reusing it.
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
            "Não foi possível verificar o objeto S3 antes do envio (HTTP {})",
            existing.status().as_u16()
        );
        let headers = BTreeMap::from([
            ("if-none-match".into(), "*".into()),
            ("x-amz-meta-nethergate-sha256".into(), expected.into()),
        ]);
        let response = self.request(Method::PUT, Some(key), &[], Some(body), expected, headers)?;
        ensure!(
            response.status().is_success() || response.status().as_u16() == 412,
            "Upload S3 recusado (HTTP {})",
            response.status().as_u16()
        );
        self.verify(key, size, expected)
    }
    fn snapshot_key(&self, id: &str, file: &str) -> String {
        format!("{}/{id}/{file}", self.settings.prefix)
    }
    fn upload_progress(
        &self,
        store: &Path,
        server_id: &str,
        id: &str,
        cancel: &AtomicBool,
        mut progress: impl FnMut(&chunks::Progress) -> Result<()>,
    ) -> Result<()> {
        ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
        let (manifest, files) = backups::cloud_snapshot(store, server_id, id)?;
        self.ensure_not_retired(id)?;
        let key = self.snapshot_key(id, "manifest.json");
        let existing = self.request(
            Method::GET,
            Some(&key),
            &[],
            None,
            &hash(&[]),
            BTreeMap::new(),
        )?;
        if existing.status().is_success() {
            let mut bytes = vec![];
            existing
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if serde_json::from_slice::<Value>(&bytes)?["format"] == 3 {
                let mut remote = chunks::parse(&bytes)?.manifest;
                let local: Value = serde_json::from_slice(&manifest)?;
                remote["serverId"] = local["serverId"].clone();
                remote["info"]["reason"] = local["info"]["reason"].clone();
                ensure!(remote == local, "Já existe outro backup com este ID");
                return chunks::upload(
                    self,
                    store,
                    server_id,
                    id,
                    self.settings.compression,
                    cancel,
                    |p| progress(p),
                );
            }
            let (remote_bytes, compressed) = advanced::decode_manifest(&bytes)?;
            let mut remote: Value = serde_json::from_slice(&remote_bytes)?;
            let local: Value = serde_json::from_slice(&manifest)?;
            remote["serverId"] = local["serverId"].clone();
            remote["info"]["reason"] = local["info"]["reason"].clone();
            ensure!(
                remote == local,
                "Já existe um arquivo cloud diferente com este ID"
            );
            for file in &files {
                ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
                self.verify_archive_file(id, file, compressed)?;
            }
            return Ok(());
        }
        ensure!(
            existing.status().as_u16() == 404,
            "Não foi possível verificar o destino S3 (HTTP {})",
            existing.status().as_u16()
        );
        if self.settings.deduplication {
            return chunks::upload(
                self,
                store,
                server_id,
                id,
                self.settings.compression,
                cancel,
                |state| progress(state),
            );
        }
        progress(&chunks::Progress {
            files: 0,
            total_files: files.len() + 1,
            ..Default::default()
        })?;
        for (i, file) in files.iter().enumerate() {
            ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
            backups::ordinary(&file.path)?;
            if self.settings.compression {
                let compressed =
                    advanced::CompressedFile::new(&file.path, file.bytes, &file.sha256, cancel)?;
                self.put_file(
                    &self.snapshot_key(id, &format!("{}.gz", file.key)),
                    &compressed.path,
                    compressed.bytes,
                    &compressed.sha256,
                    cancel,
                )?;
            } else {
                self.put_file(
                    &self.snapshot_key(id, &file.key),
                    &file.path,
                    file.bytes,
                    &file.sha256,
                    cancel,
                )?;
            }
            progress(&chunks::Progress {
                files: i + 1,
                total_files: files.len() + 1,
                ..Default::default()
            })?;
        }
        ensure!(!cancel.load(Ordering::SeqCst), "Envio cancelado");
        let manifest = if self.settings.compression {
            serde_json::to_vec(
                &json!({"format":2,"compression":"gzip","manifest":serde_json::from_slice::<Value>(&manifest)?}),
            )?
        } else {
            manifest
        };
        self.put(
            &key,
            Body::from(manifest.clone()),
            manifest.len() as u64,
            &hash(&manifest),
        )?;
        progress(&chunks::Progress {
            files: files.len() + 1,
            total_files: files.len() + 1,
            ..Default::default()
        })?;
        Ok(())
    }
    #[cfg(test)]
    fn upload(
        &self,
        store: &Path,
        server_id: &str,
        id: &str,
        cancel: &AtomicBool,
        mut progress: impl FnMut(usize, usize) -> Result<()>,
    ) -> Result<()> {
        self.upload_progress(store, server_id, id, cancel, |p| {
            progress(p.files, p.total_files)
        })
    }
    fn history(&self) -> Result<Value> {
        self.history_limit(20, false)
    }
    fn history_limit(&self, maximum: usize, include_retired: bool) -> Result<Value> {
        let prefix = format!("{}/", self.settings.prefix);
        let mut ids = vec![];
        let mut token = None;
        let mut limited = false;
        for page in 0..10 {
            let mut query = vec![
                ("list-type".into(), "2".into()),
                ("delimiter".into(), "/".into()),
                ("prefix".into(), prefix.clone()),
                ("max-keys".into(), "1000".into()),
            ];
            if let Some(value) = token.take() {
                query.push(("continuation-token".into(), value));
            }
            let response =
                self.request(Method::GET, None, &query, None, &hash(&[]), BTreeMap::new())?;
            ensure!(
                response.status().is_success(),
                "Listagem S3 recusada (HTTP {})",
                response.status().as_u16()
            );
            let mut bytes = vec![];
            response.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 2 * 1024 * 1024, "Listagem demasiado grande");
            let xml = std::str::from_utf8(&bytes)?;
            let doc = roxmltree::Document::parse(xml)?;
            for node in doc
                .descendants()
                .filter(|n| n.has_tag_name("CommonPrefixes"))
            {
                if let Some(value) = node
                    .children()
                    .find(|n| n.has_tag_name("Prefix"))
                    .and_then(|n| n.text())
                {
                    if let Some(id) = value
                        .strip_prefix(&prefix)
                        .and_then(|s| s.strip_suffix('/'))
                    {
                        if backups::valid_id(id).is_ok() {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
            let truncated = doc
                .descendants()
                .any(|n| n.has_tag_name("IsTruncated") && n.text() == Some("true"));
            if !truncated {
                break;
            }
            token = doc
                .descendants()
                .find(|n| n.has_tag_name("NextContinuationToken"))
                .and_then(|n| n.text())
                .map(str::to_string);
            ensure!(
                token.as_ref().is_some_and(|s| s.len() <= 8192),
                "Paginação S3 inválida"
            );
            if page == 9 {
                limited = true;
            }
        }
        ids.sort();
        ids.dedup();
        ids.reverse();
        let mut rows = vec![];
        limited |= ids.len() > maximum;
        for id in ids.into_iter().take(maximum) {
            if !include_retired && self.is_retired(&id)? {
                continue;
            }
            let response = self.request(
                Method::GET,
                Some(&self.snapshot_key(&id, "manifest.json")),
                &[],
                None,
                &hash(&[]),
                BTreeMap::new(),
            )?;
            if response.status().as_u16() == 404 {
                continue;
            }
            ensure!(
                response.status().is_success(),
                "Não foi possível ler um manifesto cloud"
            );
            let mut bytes = vec![];
            response
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 64 * 1024 * 1024,
                "Manifesto demasiado grande"
            );
            let (bytes, _) = advanced::decode_manifest(&bytes)?;
            let metadata: Value = serde_json::from_slice(&bytes)?;
            let info: backups::BackupInfo = serde_json::from_value(metadata["info"].clone())?;
            ensure!(
                info.id == id && info.worlds.len() <= 256 && info.file_count <= 200_000,
                "Manifesto cloud inválido"
            );
            rows.push(info);
        }
        rows.sort_by_key(|v| std::cmp::Reverse(v.created_at));
        Ok(json!({"backups":rows,"limited":limited}))
    }
    fn download(
        &self,
        store: &Path,
        server_id: &str,
        id: &str,
        cancel: &AtomicBool,
    ) -> Result<backups::BackupInfo> {
        backups::valid_id(id)?;
        self.ensure_not_retired(id)?;
        let archive = self.bytes(&self.snapshot_key(id, "manifest.json"), 64 * 1024 * 1024)?;
        if serde_json::from_slice::<Value>(&archive)?["format"] == 3 {
            return chunks::download(self, store, server_id, id, &archive, cancel);
        }
        let (manifest, compressed) = advanced::decode_manifest(&archive)?;
        backups::receive_cloud(
            store,
            server_id,
            id,
            &manifest,
            |file, to, bytes, expected| {
                ensure!(!cancel.load(Ordering::SeqCst), "Transferência cancelada");
                let mut output = OpenOptions::new().write(true).create_new(true).open(to)?;
                let response = self.get(&self.snapshot_key(
                    id,
                    &if compressed {
                        format!("{file}.gz")
                    } else {
                        file.into()
                    },
                ))?;
                let mut reader: Box<dyn Read> = if compressed {
                    Box::new(flate2::read::GzDecoder::new(response))
                } else {
                    Box::new(response)
                };
                let copied = std::io::copy(
                    &mut reader.by_ref().take(bytes.saturating_add(1)),
                    &mut output,
                )?;
                output.sync_all()?;
                drop(output);
                ensure!(
                    copied == bytes && crate::paper::hash_file(to)? == expected,
                    "Ficheiro cloud incompleto ou corrompido"
                );
                Ok(())
            },
        )
    }
}
pub fn history(root: &Path, id: &str) -> Result<Value> {
    S3::new(load(root, id)?.context("Configura primeiro o destino cloud")?)?.history()
}
pub fn upload(root: &Path, id: &str, backup_id: &str, cancel: &AtomicBool) -> Result<()> {
    let settings = load(root, id)?.context("Configura primeiro o destino cloud")?;
    let client = S3::new(settings)?;
    let mut current = state(root, id)?;
    let result = (|| -> Result<()> {
        let store = crate::backup_destination::resolve(root, id)?;
        current.pending_backup = Some(backup_id.into());
        persist(root, id, &current)?;
        client.upload_progress(&store, id, backup_id, cancel, |progress| {
            current.completed_files = progress.files;
            current.total_files = progress.total_files;
            current.sent_blocks = progress.sent_blocks;
            current.reused_blocks = progress.reused_blocks;
            current.error = None;
            persist(root, id, &current)
        })
    })();
    match &result {
        Ok(()) => {
            current.pending_backup = None;
            if !current.uploaded.iter().any(|s| s == backup_id) {
                ensure!(
                    current.uploaded.len() < 50_000,
                    "O índice cloud atingiu o limite"
                );
                current.uploaded.push(backup_id.into());
            }
            current.last_success = Some(backup_schedule::now()?);
            current.last_backup_id = Some(backup_id.into());
            current.error = None;
            current.retry_at = None;
            current.retention_error = None;
            if client.settings.retention_keep > 0 {
                let cleanup = client
                    .retention_plan(id, client.settings.retention_keep)
                    .and_then(|plan| {
                        ensure!(!plan.remove.iter().any(|b|b.id==backup_id), "O relógio coloca o novo backup entre os antigos; revê a retenção manualmente");
                        client.apply_retention(
                            id,
                            &plan.fingerprint,
                            client.settings.retention_keep,
                            cancel,
                        )
                    });
                if let Err(e) = cleanup {
                    current.retention_error =
                        Some(format!("Cópia enviada; limpeza cloud pendente: {e:#}"));
                }
            }
        }
        Err(e) => {
            current.error = Some(format!("{e:#}"));
            current.retry_at = Some(backup_schedule::now()?.saturating_add(300_000));
        }
    }
    persist(root, id, &current)?;
    result
}
pub fn download(
    root: &Path,
    id: &str,
    backup_id: &str,
    cancel: &AtomicBool,
) -> Result<backups::BackupInfo> {
    let store = crate::backup_destination::resolve(root, id)?;
    S3::new(load(root, id)?.context("Configura primeiro o destino cloud")?)?
        .download(&store, id, backup_id, cancel)
}
pub fn record_error(root: &Path, id: &str, error: &str) -> Result<()> {
    let mut current = state(root, id)?;
    current.error = Some(error.into());
    current.retry_at = Some(backup_schedule::now()?.saturating_add(300_000));
    persist(root, id, &current)
}
pub fn begin(root: &Path, id: &str) -> Result<()> {
    let mut value = state(root, id)?;
    value.completed_files = 0;
    value.total_files = 0;
    value.sent_blocks = 0;
    value.reused_blocks = 0;
    value.error = None;
    persist(root, id, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        thread,
    };
    fn settings(endpoint: &str) -> Settings {
        Settings {
            endpoint: endpoint.into(),
            region: "us-east-1".into(),
            bucket: "bucket".into(),
            prefix: "snapshots/profile".into(),
            access_key: "TESTKEY".into(),
            secret_key: "TESTSECRET".into(),
            session_token: String::new(),
            automatic: false,
            compression: false,
            deduplication: false,
            retention_keep: 0,
        }
    }
    struct Mock {
        endpoint: String,
        objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        puts: Arc<Mutex<Vec<String>>>,
        calls: Arc<Mutex<Vec<String>>>,
        fail_part: Arc<AtomicBool>,
        fail_delete: Arc<AtomicBool>,
        corrupt: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }
    impl Mock {
        fn new() -> Self {
            let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
            let endpoint = format!(
                "http://127.0.0.1:{}",
                server.server_addr().to_ip().unwrap().port()
            );
            let credentials = settings(&endpoint);
            let objects = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
            let puts = Arc::new(Mutex::new(Vec::new()));
            let put_log = puts.clone();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let call_log = calls.clone();
            let fail_part = Arc::new(AtomicBool::new(false));
            let part_error = fail_part.clone();
            let fail_delete = Arc::new(AtomicBool::new(false));
            let delete_error = fail_delete.clone();
            let corrupt = Arc::new(AtomicBool::new(false));
            let stop = Arc::new(AtomicBool::new(false));
            let (store, broken, done) = (objects.clone(), corrupt.clone(), stop.clone());
            let join = thread::spawn(move || {
                let mut uploads = BTreeMap::<String, BTreeMap<u32, Vec<u8>>>::new();
                while !done.load(Ordering::SeqCst) {
                    let Some(mut request) = server.recv_timeout(Duration::from_millis(50)).unwrap()
                    else {
                        continue;
                    };
                    let url = request.url().to_string();
                    let (uri, query) = url.split_once('?').unwrap_or((&url, ""));
                    let header = |name: &str| {
                        request
                            .headers()
                            .iter()
                            .find(|h| h.field.to_string().eq_ignore_ascii_case(name))
                            .map(|h| h.value.as_str().to_string())
                    };
                    let auth = header("Authorization").unwrap_or_default();
                    let date = header("x-amz-date").unwrap_or_default();
                    let payload = header("x-amz-content-sha256").unwrap_or_default();
                    let signed = auth
                        .split("SignedHeaders=")
                        .nth(1)
                        .and_then(|s| s.split(',').next())
                        .unwrap_or("");
                    let mut headers = BTreeMap::new();
                    for name in signed.split(';') {
                        if let Some(value) = header(name) {
                            headers.insert(name.into(), value);
                        }
                    }
                    let valid = signature(
                        &credentials,
                        request.method().as_str(),
                        uri,
                        query,
                        &date,
                        &payload,
                        &mut headers,
                    )
                    .is_ok_and(|v| v == auth);
                    if !valid {
                        let _ = request.respond(tiny_http::Response::empty(403));
                        continue;
                    }
                    let mut body = vec![];
                    request
                        .as_reader()
                        .take(32 * 1024 * 1024)
                        .read_to_end(&mut body)
                        .unwrap();
                    if hash(&body) != payload {
                        let _ = request.respond(tiny_http::Response::empty(400));
                        continue;
                    }
                    call_log
                        .lock()
                        .unwrap()
                        .push(format!("{} {}", request.method(), url));
                    let parsed = reqwest::Url::parse(&format!("http://localhost{url}")).unwrap();
                    let params: BTreeMap<String, String> = parsed
                        .query_pairs()
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    if request.method() == &tiny_http::Method::Post
                        && params.contains_key("uploads")
                    {
                        uploads.insert(uri.into(), BTreeMap::new());
                        let _ = request.respond(tiny_http::Response::from_string("<InitiateMultipartUploadResult><UploadId>mock+/upload==1</UploadId></InitiateMultipartUploadResult>"));
                        continue;
                    }
                    if params.contains_key("uploadId") {
                        if request.method() == &tiny_http::Method::Delete {
                            uploads.remove(uri);
                            let _ = request.respond(tiny_http::Response::empty(204));
                        } else if request.method() == &tiny_http::Method::Put {
                            if part_error.load(Ordering::SeqCst) {
                                let _ = request.respond(tiny_http::Response::empty(500));
                                continue;
                            }
                            let etag = format!("\"{}\"", hash(&body));
                            uploads
                                .get_mut(uri)
                                .unwrap()
                                .insert(params["partNumber"].parse().unwrap(), body);
                            let _ =
                                request.respond(tiny_http::Response::empty(200).with_header(
                                    tiny_http::Header::from_bytes("ETag", etag).unwrap(),
                                ));
                        } else {
                            let mut objects = store.lock().unwrap();
                            if objects.contains_key(uri) {
                                let _ = request.respond(tiny_http::Response::empty(412));
                                continue;
                            }
                            assert!(
                                request
                                    .headers()
                                    .iter()
                                    .any(|h| h.field.equiv("if-none-match")
                                        && h.value.as_str() == "*")
                            );
                            let parts = uploads.remove(uri).unwrap();
                            let value = parts.into_values().flatten().collect::<Vec<_>>();
                            objects.insert(uri.into(), value);
                            let _=request.respond(tiny_http::Response::from_string("<CompleteMultipartUploadResult><ETag>complete</ETag></CompleteMultipartUploadResult>"));
                        }
                        continue;
                    }
                    if request.method() == &tiny_http::Method::Delete {
                        if delete_error.swap(false, Ordering::SeqCst) {
                            let _ = request.respond(tiny_http::Response::empty(500));
                            continue;
                        }
                        let mut objects = store.lock().unwrap();
                        if let Some(etag) =
                            request.headers().iter().find(|h| h.field.equiv("if-match"))
                        {
                            if objects
                                .get(uri)
                                .is_some_and(|v| etag.value.as_str() != format!("\"{}\"", hash(v)))
                            {
                                let _ = request.respond(tiny_http::Response::empty(412));
                                continue;
                            }
                        }
                        objects.remove(uri);
                        let _ = request.respond(tiny_http::Response::empty(204));
                        continue;
                    }
                    if request.method() == &tiny_http::Method::Put {
                        put_log.lock().unwrap().push(uri.into());
                        let mut objects = store.lock().unwrap();
                        if objects.contains_key(uri) {
                            let _ = request.respond(tiny_http::Response::empty(412));
                        } else {
                            objects.insert(uri.into(), body);
                            let _ = request.respond(tiny_http::Response::empty(200));
                        }
                    } else if !query.is_empty() {
                        let objects = store.lock().unwrap();
                        let prefix = "/bucket/snapshots/profile/";
                        let mut ids = objects
                            .keys()
                            .filter_map(|k| {
                                k.strip_prefix(prefix)
                                    .and_then(|k| k.split('/').next())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>();
                        ids.sort();
                        ids.dedup();
                        let xml=format!("<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><IsTruncated>false</IsTruncated>{}</ListBucketResult>",ids.iter().map(|id|format!("<CommonPrefixes><Prefix>snapshots/profile/{id}/</Prefix></CommonPrefixes>")).collect::<String>());
                        let _ = request.respond(tiny_http::Response::from_string(xml));
                    } else {
                        let value = store.lock().unwrap().get(uri).cloned();
                        match value {
                            Some(mut bytes) => {
                                if broken.load(Ordering::SeqCst) && uri.contains("/worlds/") {
                                    bytes = b"corruption".to_vec();
                                }
                                let etag = format!("\"{}\"", hash(&bytes));
                                let _ = request.respond(
                                    tiny_http::Response::from_data(bytes).with_header(
                                        tiny_http::Header::from_bytes("ETag", etag).unwrap(),
                                    ),
                                );
                            }
                            None => {
                                let _ = request.respond(tiny_http::Response::empty(404));
                            }
                        }
                    }
                }
            });
            Self {
                endpoint,
                objects,
                puts,
                calls,
                fail_part,
                fail_delete,
                corrupt,
                stop,
                join: Some(join),
            }
        }
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(join) = self.join.take() {
                join.join().unwrap();
            }
        }
    }
    fn fixture() -> (PathBuf, PathBuf, backups::BackupInfo) {
        let root =
            std::env::temp_dir().join(format!("ng-cloud-{}", crate::transfer::key().unwrap()));
        let server = root.join("server");
        fs::create_dir_all(server.join("world")).unwrap();
        crate::config::write_json(
            &server.join("nethergate.json"),
            &crate::config::Config::new("1.21.11".into(), "https://example.org".into(), 21)
                .unwrap(),
        )
        .unwrap();
        fs::write(server.join("server.properties"), "level-name=world\n").unwrap();
        fs::write(server.join("world/level.dat"), b"original-world").unwrap();
        let store = root.join("backups");
        let copy = backups::create(&store, "source", &server, "manual").unwrap();
        (root, store, copy)
    }
    #[test]
    fn fixed_size_hmac_constructor_preserves_all_key_lengths() {
        for length in [0, 1, 32, 63, 64, 65, 128, 1024] {
            let key = vec![0x5a; length];
            let message = b"signature regression fixture";
            let mut previous = Hmac::<Sha256>::new_from_slice(&key).unwrap();
            previous.update(message);
            assert_eq!(
                hmac(&key, message),
                previous.finalize().into_bytes().to_vec()
            );
        }
    }

    #[test]
    fn deduplicated_s3_retention_preserves_blocks_used_by_survivors() {
        let mock = Mock::new();
        let mut settings = settings(&mock.endpoint);
        settings.deduplication = true;
        let mut client = S3::new(settings).unwrap();
        let (_, store, _) = fixture();
        let server = store.with_file_name("server");
        let cancel = AtomicBool::new(false);
        let mut copies = vec![];
        for _ in 0..3 {
            let copy = backups::create(&store, "source", &server, "automatic").unwrap();
            client
                .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
                .unwrap();
            copies.push(copy);
        }
        assert_eq!(
            mock.puts
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.contains("/objects/sha256/"))
                .count(),
            1
        );
        let plan = client.retention_plan("source", 2).unwrap();
        assert_eq!(
            client
                .apply_retention("source", &plan.fingerprint, 2, &cancel)
                .unwrap(),
            1
        );
        assert!(!mock
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("DELETE") && c.contains("/objects/sha256/")));
        let survivor = copies
            .iter()
            .find(|c| !plan.remove.iter().any(|r| r.id == c.id))
            .unwrap();
        client.settings.deduplication = false;
        client
            .upload(&store, "source", &survivor.id, &cancel, |_, _| Ok(()))
            .unwrap();
        client
            .download(
                &store.with_file_name("dedup-recovered"),
                "target",
                &survivor.id,
                &cancel,
            )
            .unwrap();
        assert_eq!(
            client.history().unwrap()["backups"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn gzip_roundtrip_reuses_archive_even_after_compression_setting_changes() {
        let mock = Mock::new();
        let (_root, store, copy) = fixture();
        let mut config = settings(&mock.endpoint);
        config.compression = true;
        let mut client = S3::new(config).unwrap();
        let cancel = AtomicBool::new(false);
        client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
        assert!(mock
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with(".gz")));
        let put_count = mock.puts.lock().unwrap().len();
        client.settings.compression = false;
        client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
        assert_eq!(mock.puts.lock().unwrap().len(), put_count);
        let target = store.with_file_name("downloaded-gzip");
        let recovered = client
            .download(&target, "target", &copy.id, &cancel)
            .unwrap();
        assert_eq!(recovered.size_bytes, copy.size_bytes);
        assert_eq!(
            fs::read(target.join(&copy.id).join("worlds/world/level.dat")).unwrap(),
            b"original-world"
        );
        mock.corrupt.store(true, Ordering::SeqCst);
        assert!(client
            .download(
                &store.with_file_name("corrupt-gzip"),
                "target",
                &copy.id,
                &cancel
            )
            .is_err());
    }
    #[test]
    fn multipart_commits_conditionally_and_aborts_failed_parts() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (root, _, _) = fixture();
        let path = root.join("large.bin");
        let bytes = vec![42u8; 6 * 1024 * 1024];
        fs::write(&path, &bytes).unwrap();
        let cancel = AtomicBool::new(false);
        client
            .multipart(
                "large",
                &path,
                bytes.len() as u64,
                &hash(&bytes),
                &cancel,
                5 * 1024 * 1024,
            )
            .unwrap();
        assert_eq!(
            mock.objects.lock().unwrap().get("/bucket/large").unwrap(),
            &bytes
        );
        mock.fail_part.store(true, Ordering::SeqCst);
        assert!(client
            .multipart(
                "broken",
                &path,
                bytes.len() as u64,
                &hash(&bytes),
                &cancel,
                5 * 1024 * 1024
            )
            .is_err());
        assert!(!mock.objects.lock().unwrap().contains_key("/bucket/broken"));
        assert!(mock
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.starts_with("DELETE /bucket/broken?uploadId=")));
    }
    #[test]
    fn retention_preserves_manual_copies_refuses_stale_plan_and_resumes_cleanup() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (_root, store, manual) = fixture();
        let server = store.with_file_name("server");
        let cancel = AtomicBool::new(false);
        client
            .upload(&store, "source", &manual.id, &cancel, |_, _| Ok(()))
            .unwrap();
        let mut ids = vec![];
        for _ in 0..4 {
            let copy = backups::create(&store, "source", &server, "automatic").unwrap();
            client
                .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
                .unwrap();
            ids.push(copy.id);
        }
        // An accidentally shared prefix must not let this profile retire another profile's data.
        let foreign = backups::create(&store, "other-profile", &server, "automatic").unwrap();
        client
            .upload(&store, "other-profile", &foreign.id, &cancel, |_, _| Ok(()))
            .unwrap();
        let plan = client.retention_plan("source", 2).unwrap();
        assert_eq!(plan.remove.len(), 2);
        assert!(client
            .apply_retention("source", "stale", 2, &cancel)
            .is_err());
        mock.fail_delete.store(true, Ordering::SeqCst);
        assert!(client
            .apply_retention("source", &plan.fingerprint, 2, &cancel)
            .is_err());
        let retry = client.retention_plan("source", 2).unwrap();
        assert_eq!(
            client
                .apply_retention("source", &retry.fingerprint, 2, &cancel)
                .unwrap(),
            2
        );
        let history = client.history().unwrap();
        assert_eq!(history["backups"].as_array().unwrap().len(), 4);
        assert!(mock.objects.lock().unwrap().contains_key(&format!(
            "/bucket/{}",
            client.snapshot_key(&foreign.id, "manifest.json")
        )));
        assert!(mock.objects.lock().unwrap().contains_key(&format!(
            "/bucket/{}",
            client.snapshot_key(&manual.id, "manifest.json")
        )));
        for removed in &plan.remove {
            assert!(client
                .upload(&store, "source", &removed.id, &cancel, |_, _| Ok(()))
                .is_err());
        }
    }
    #[test]
    fn corrupt_retention_survivor_prevents_remote_deletion() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (_, store, _) = fixture();
        let server = store.with_file_name("server");
        let cancel = AtomicBool::new(false);
        for _ in 0..3 {
            let copy = backups::create(&store, "source", &server, "automatic").unwrap();
            client
                .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
                .unwrap();
        }
        let plan = client.retention_plan("source", 2).unwrap();
        mock.corrupt.store(true, Ordering::SeqCst);
        assert!(client
            .apply_retention("source", &plan.fingerprint, 2, &cancel)
            .is_err());
        assert!(!mock
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.starts_with("DELETE")));
    }
    #[test]
    fn aws_published_signature_vector_and_uri_encoding() {
        let mut value = settings("https://s3.amazonaws.com");
        value.access_key = "AKIAIOSFODNN7EXAMPLE".into();
        value.secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into();
        let mut headers = BTreeMap::from([
            ("host".into(), "examplebucket.s3.amazonaws.com".into()),
            ("range".into(), "bytes=0-9".into()),
        ]);
        let auth = signature(
            &value,
            "GET",
            "/test.txt",
            "",
            "20130524T000000Z",
            &hash(&[]),
            &mut headers,
        )
        .unwrap();
        assert!(
            auth.ends_with(
                "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
            ),
            "{auth}"
        );
        assert_eq!(encoded("a b/é+", true), "a%20b/%C3%A9%2B");
        value.endpoint = "http://example.org".into();
        assert!(validate(&value).is_err());
    }
    #[test]
    fn signed_upload_readback_history_and_recovery_into_new_profile() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (root, store, copy) = fixture();
        let cancel = AtomicBool::new(false);
        client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
        assert!(mock
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("/manifest.json")));
        let history = client.history().unwrap();
        assert_eq!(history["backups"][0]["id"], copy.id);
        let recovered = root.join("recovered");
        client
            .download(&recovered, "new-profile", &copy.id, &cancel)
            .unwrap();
        assert_eq!(backups::list(&recovered, "new-profile").unwrap().len(), 1);
        client
            .upload(&recovered, "new-profile", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
        let server = root.join("server");
        fs::write(server.join("world/level.dat"), b"changed-world").unwrap();
        backups::restore(&recovered, "new-profile", &server, &copy.id).unwrap();
        assert_eq!(
            fs::read(server.join("world/level.dat")).unwrap(),
            b"original-world"
        );
        client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
    }
    #[test]
    fn interrupted_upload_reuses_verified_files_without_sending_them_again() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (_, store, copy) = fixture();
        let cancel = AtomicBool::new(false);
        assert!(client
            .upload(&store, "source", &copy.id, &cancel, |done, _| {
                if done == 1 {
                    cancel.store(true, Ordering::SeqCst);
                }
                Ok(())
            })
            .is_err());
        let sent = mock.puts.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert!(!sent[0].ends_with("/manifest.json"));
        cancel.store(false, Ordering::SeqCst);
        client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .unwrap();
        let requests = mock.puts.lock().unwrap();
        assert_eq!(requests.iter().filter(|key| **key == sent[0]).count(), 1);
        assert!(requests.last().unwrap().ends_with("/manifest.json"));
        assert_eq!(client.history().unwrap()["backups"][0]["id"], copy.id);
    }
    #[test]
    fn resume_rejects_corrupt_remote_files_without_overwriting_or_committing() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (_, store, copy) = fixture();
        let cancel = AtomicBool::new(false);
        assert!(client
            .upload(&store, "source", &copy.id, &cancel, |done, _| {
                if done == 1 {
                    cancel.store(true, Ordering::SeqCst);
                }
                Ok(())
            })
            .is_err());
        let first = mock.puts.lock().unwrap()[0].clone();
        mock.objects
            .lock()
            .unwrap()
            .insert(first.clone(), b"corrupt".to_vec());
        cancel.store(false, Ordering::SeqCst);
        assert!(client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .is_err());
        assert_eq!(*mock.puts.lock().unwrap(), vec![first.clone()]);
        let objects = mock.objects.lock().unwrap();
        assert_eq!(objects[&first], b"corrupt");
        assert!(!objects.keys().any(|key| key.ends_with("/manifest.json")));
    }
    #[test]
    fn corruption_and_cancellation_never_publish_a_complete_snapshot() {
        let mock = Mock::new();
        let client = S3::new(settings(&mock.endpoint)).unwrap();
        let (_, store, copy) = fixture();
        let cancel = AtomicBool::new(true);
        assert!(client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .is_err());
        assert!(mock.objects.lock().unwrap().is_empty());
        cancel.store(false, Ordering::SeqCst);
        mock.corrupt.store(true, Ordering::SeqCst);
        assert!(client
            .upload(&store, "source", &copy.id, &cancel, |_, _| Ok(()))
            .is_err());
        assert!(!mock
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("/manifest.json")));
    }
    #[cfg(windows)]
    #[test]
    fn credentials_are_protected_and_never_returned_to_the_panel() {
        let root = std::env::temp_dir().join(format!(
            "ng-cloud-secret-{}",
            crate::transfer::key().unwrap()
        ));
        let value = settings("https://storage.example.org");
        save(&root, "profile", value.clone()).unwrap();
        let bytes = fs::read(paths(&root, "profile").unwrap().0).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("TESTSECRET"));
        let status = serde_json::to_string(&status(&root, "profile").unwrap()).unwrap();
        assert!(!status.contains("TESTSECRET") && !status.contains("TESTKEY"));
        let mut changed = value;
        changed.endpoint = "https://other.example.org".into();
        changed.access_key.clear();
        changed.secret_key.clear();
        assert!(save(&root, "profile", changed).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn automatic_upload_works_without_a_panel() {
        let mock = Mock::new();
        let root = std::env::temp_dir().join(format!(
            "ng-cloud-background-{}",
            crate::transfer::key().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let hub = crate::hub::Hub::new(root.clone()).unwrap();
        let input:crate::hub::CreateServer=serde_json::from_value(json!({"software":"paper","name":"Cloud fixture","version":"1.21.11","memoryMb":1024,"port":25590,"contact":"https://example.org","javaMajor":21,"javaPath":"auto","eulaAccepted":true})).unwrap();
        let id = hub.create(input).unwrap();
        let server = root.join("servers").join(&id);
        fs::create_dir(server.join("world")).unwrap();
        fs::write(server.join("world/level.dat"), b"background-world").unwrap();
        let backup = hub.create_backup(&id).unwrap();
        let mut config = settings(&mock.endpoint);
        config.automatic = true;
        save(&root, &id, config).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let value = state(&root, &id).unwrap();
            if value.last_backup_id.as_deref() == Some(&backup.id) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{:?}", value.error);
            thread::sleep(Duration::from_millis(100));
        }
        hub.stop_all();
        assert!(mock
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("manifest.json")));
    }
}
