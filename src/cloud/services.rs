//! Drive/Dropbox adapters for the same verified, resumable block archive used by S3.
use super::{
    accounts::{self, Provider},
    chunks::{self, Remote},
    *,
};
use std::sync::Mutex;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub namespace: String,
    #[serde(default)]
    pub compression: bool,
    #[serde(default)]
    pub automatic: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferState {
    #[serde(default)]
    pending_backup: Option<String>,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    uploaded: Vec<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    last_success: Option<u64>,
    #[serde(default)]
    retry_at: Option<u64>,
    #[serde(default)]
    files: usize,
    #[serde(default)]
    total_files: usize,
    #[serde(default)]
    sent_blocks: usize,
    #[serde(default)]
    reused_blocks: usize,
}
fn directory(root: &Path, id: &str, provider: Provider) -> Result<PathBuf> {
    backups::component(id)?;
    Ok(root
        .join("hub-data/cloud-services")
        .join(id)
        .join(provider.key()))
}
fn settings(root: &Path, id: &str, provider: Provider) -> Result<Settings> {
    let path = directory(root, id, provider)?.join("settings.json");
    if !path.try_exists()? {
        return Ok(Settings {
            namespace: id.into(),
            compression: true,
            automatic: false,
        });
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 4096,
        "Configuração cloud inválida"
    );
    let value: Settings = serde_json::from_slice(&fs::read(path)?)?;
    validate(&value)?;
    Ok(value)
}
fn validate(value: &Settings) -> Result<()> {
    ensure!(
        !value.namespace.is_empty()
            && value.namespace.len() <= 128
            && value
                .namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "Identificador cloud: usa letras, números, hífen ou underscore"
    );
    Ok(())
}
fn state(root: &Path, id: &str, provider: Provider) -> Result<TransferState> {
    let path = directory(root, id, provider)?.join("state.json");
    if !path.try_exists()? {
        return Ok(TransferState::default());
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 4 * 1024 * 1024,
        "Estado cloud demasiado grande"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn persist(root: &Path, id: &str, provider: Provider, state: &TransferState) -> Result<()> {
    let dir = directory(root, id, provider)?;
    fs::create_dir_all(&dir)?;
    crate::hub::replace_json(&dir.join("state.json"), state)
}
pub fn status(root: &Path, id: &str, provider: Provider) -> Result<Value> {
    let settings = settings(root, id, provider)?;
    let state = state(root, id, provider)?;
    let account = accounts::status(root, provider)?;
    Ok(
        json!({"account":account,"settings":settings,"state":{"error":state.error,"lastSuccess":state.last_success,"files":state.files,"totalFiles":state.total_files,"sentBlocks":state.sent_blocks,"reusedBlocks":state.reused_blocks,"pendingBackup":state.pending_backup}}),
    )
}

pub fn record_error(root: &Path, id: &str, provider: Provider, error: &str) -> Result<()> {
    let mut state = state(root, id, provider)?;
    state.error = Some(error.into());
    state.retry_at = Some(backup_schedule::now()?.saturating_add(300_000));
    persist(root, id, provider, &state)
}
pub fn save(root: &Path, id: &str, provider: Provider, value: Settings) -> Result<Value> {
    validate(&value)?;
    let old = settings(root, id, provider)?;
    let dir = directory(root, id, provider)?;
    fs::create_dir_all(&dir)?;
    crate::hub::replace_json(&dir.join("settings.json"), &value)?;
    if old.namespace != value.namespace {
        persist(root, id, provider, &TransferState::default())?;
    }
    status(root, id, provider)
}
pub(crate) fn automatic_candidate(
    root: &Path,
    id: &str,
    provider: Provider,
) -> Result<Option<String>> {
    if !settings(root, id, provider)?.automatic
        || accounts::status(root, provider)?["connected"] != true
    {
        return Ok(None);
    }
    let state = state(root, id, provider)?;
    if state
        .retry_at
        .is_some_and(|v| v > backup_schedule::now().unwrap_or(0))
    {
        return Ok(None);
    }
    let generation = accounts::generation(root, provider)?;
    let store = crate::backup_destination::resolve(root, id)?;
    let backups = backups::list(&store, id)?;
    if state.generation == generation {
        if let Some(pending) = state
            .pending_backup
            .as_ref()
            .filter(|p| backups.iter().any(|b| &b.id == *p))
        {
            return Ok(Some(pending.clone()));
        }
    }
    Ok(backups
        .into_iter()
        .rev()
        .find(|b| state.generation != generation || !state.uploaded.contains(&b.id))
        .map(|b| b.id))
}

pub fn upload(
    root: &Path,
    id: &str,
    provider: Provider,
    backup: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    let config = settings(root, id, provider)?;
    let remote = ServiceRemote::new(root, provider, &config.namespace)?;
    let mut current = state(root, id, provider)?;
    if current.generation != remote.generation {
        current = TransferState {
            generation: remote.generation.clone(),
            ..Default::default()
        };
    }
    current.error = None;
    current.pending_backup = Some(backup.into());
    current.files = 0;
    current.total_files = 0;
    current.sent_blocks = 0;
    current.reused_blocks = 0;
    persist(root, id, provider, &current)?;
    let store = crate::backup_destination::resolve(root, id)?;
    let result = chunks::upload(
        &remote,
        &store,
        id,
        backup,
        config.compression,
        cancel,
        |p| {
            current.files = p.files;
            current.total_files = p.total_files;
            current.sent_blocks = p.sent_blocks;
            current.reused_blocks = p.reused_blocks;
            persist(root, id, provider, &current)
        },
    );
    match &result {
        Ok(()) => {
            current.pending_backup = None;
            ensure!(current.uploaded.len() < 50_000, "Índice cloud cheio");
            if !current.uploaded.iter().any(|b| b == backup) {
                current.uploaded.push(backup.into());
            }
            current.last_success = Some(backup_schedule::now()?);
            current.retry_at = None;
        }
        Err(e) => {
            current.error = Some(format!("{e:#}"));
            current.retry_at = Some(backup_schedule::now()?.saturating_add(300_000));
        }
    }
    persist(root, id, provider, &current)?;
    result
}
pub fn download(
    root: &Path,
    id: &str,
    provider: Provider,
    backup: &str,
    cancel: &AtomicBool,
) -> Result<backups::BackupInfo> {
    backups::valid_id(backup)?;
    let remote = ServiceRemote::new(root, provider, &settings(root, id, provider)?.namespace)?;
    let bytes = remote
        .read(&chunks::snapshot_key(backup), chunks::MANIFEST_LIMIT)?
        .context("Backup cloud não encontrado")?;
    let result = chunks::download(
        &remote,
        &crate::backup_destination::resolve(root, id)?,
        id,
        backup,
        &bytes,
        cancel,
    );
    if let Err(e) = &result {
        let mut s = state(root, id, provider)?;
        s.error = Some(format!("{e:#}"));
        persist(root, id, provider, &s)?;
    }
    result
}
pub fn history(root: &Path, id: &str, provider: Provider) -> Result<Value> {
    let remote = ServiceRemote::new(root, provider, &settings(root, id, provider)?.namespace)?;
    let mut ids = remote.snapshot_ids()?;
    ids.sort();
    ids.dedup();
    ids.reverse();
    let limited = ids.len() > 20;
    let mut rows = vec![];
    for id in ids.into_iter().take(20) {
        if let Some(bytes) = remote.read(&chunks::snapshot_key(&id), chunks::MANIFEST_LIMIT)? {
            let archive = chunks::parse(&bytes)?;
            let info: backups::BackupInfo =
                serde_json::from_value(archive.manifest["info"].clone())?;
            ensure!(info.id == id, "ID cloud inválido");
            rows.push(info);
        }
    }
    rows.sort_by_key(|v| std::cmp::Reverse(v.created_at));
    Ok(json!({"backups":rows,"limited":limited}))
}

struct ServiceRemote {
    #[cfg(test)]
    test_endpoint: Option<String>,
    root: PathBuf,
    provider: Provider,
    generation: String,
    namespace: String,
    http: Client,
    folders: Mutex<std::collections::HashSet<String>>,
}
impl ServiceRemote {
    fn new(root: &Path, provider: Provider, namespace: &str) -> Result<Self> {
        Ok(Self {
            #[cfg(test)]
            test_endpoint: None,
            root: root.into(),
            provider,
            generation: accounts::generation(root, provider)?,
            namespace: hash(namespace.as_bytes()),
            http: accounts::http()?,
            folders: Mutex::new(Default::default()),
        })
    }
    fn authorized(&self, method: Method, url: &str) -> Result<reqwest::blocking::RequestBuilder> {
        let url = reqwest::Url::parse(url)?;
        let host = url.host_str().unwrap_or("");
        let allowed = match self.provider {
            Provider::GoogleDrive => host == "www.googleapis.com",
            Provider::Dropbox => host == "api.dropboxapi.com" || host == "content.dropboxapi.com",
        };
        ensure!(
            url.scheme() == "https"
                && allowed
                && url.port_or_known_default() == Some(443)
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            "Destino cloud recusado"
        );
        #[cfg(test)]
        if let Some(endpoint) = &self.test_endpoint {
            let local = format!(
                "{}{}{}",
                endpoint,
                url.path(),
                url.query().map(|q| format!("?{q}")).unwrap_or_default()
            );
            return Ok(self.http.request(method, local).bearer_auth("test-token"));
        }
        Ok(self
            .http
            .request(method, url)
            .bearer_auth(accounts::access_token(
                &self.root,
                self.provider,
                &self.generation,
            )?))
    }
    fn json(&self, url: &str, body: &Value) -> Result<Value> {
        json_response(
            self.authorized(Method::POST, url)?
                .json(body)
                .send()
                .map_err(|_| anyhow::anyhow!("Não foi possível contactar o fornecedor cloud"))?,
        )
    }
    fn drive_ids(&self, key: &str) -> Result<Vec<String>> {
        safe_key(key)?;
        let query=format!("trashed = false and appProperties has {{ key='ngNamespace' and value='{}' }} and appProperties has {{ key='ngKey' and value='{key}' }}",self.namespace);
        let response = self
            .authorized(Method::GET, "https://www.googleapis.com/drive/v3/files")?
            .query(&[
                ("spaces", "appDataFolder"),
                ("q", &query),
                ("fields", "files(id),nextPageToken"),
                ("pageSize", "100"),
            ])
            .send()
            .map_err(|_| anyhow::anyhow!("Falha ao procurar ficheiro Drive"))?;
        let value = json_response(response)?;
        ensure!(
            value["nextPageToken"].is_null(),
            "Demasiados ficheiros Drive com a mesma chave"
        );
        let ids = value["files"]
            .as_array()
            .context("Listagem Drive inválida")?
            .iter()
            .map(|v| {
                v["id"]
                    .as_str()
                    .map(str::to_string)
                    .context("ID Drive inválido")
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            ids.len() <= 8 && ids.iter().all(|id| valid_drive_id(id)),
            "IDs Drive inválidos ou duplicados em excesso"
        );
        Ok(ids)
    }
    fn dropbox_path(&self, key: &str) -> Result<String> {
        safe_key(key)?;
        Ok(format!("/nethergate/{}/{key}", self.namespace))
    }
    fn ensure_dropbox_folders(&self, path: &str) -> Result<()> {
        let parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
        let mut current = String::new();
        for part in &parts[..parts.len() - 1] {
            current.push('/');
            current.push_str(part);
            if self
                .folders
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .contains(&current)
            {
                continue;
            }
            let response = self
                .authorized(
                    Method::POST,
                    "https://api.dropboxapi.com/2/files/create_folder_v2",
                )?
                .json(&json!({"path":current,"autorename":false}))
                .send()
                .map_err(|_| anyhow::anyhow!("Falha ao preparar pasta Dropbox"))?;
            if response.status().as_u16() == 409 {
                let error = bounded(response, 65536)?;
                let value: Value = serde_json::from_slice(&error)?;
                ensure!(
                    value["error_summary"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("path/conflict/folder/")),
                    "Conflito ao preparar a pasta Dropbox"
                );
            } else {
                json_response(response)?;
            }
            self.folders
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .insert(current.clone());
        }
        Ok(())
    }
    fn snapshot_ids(&self) -> Result<Vec<String>> {
        let mut ids = vec![];
        let mut cursor = None;
        for page in 0..100 {
            let value = match self.provider {
                Provider::GoogleDrive => {
                    let query=format!("trashed = false and appProperties has {{ key='ngNamespace' and value='{}' }} and appProperties has {{ key='ngKind' and value='snapshot' }}",self.namespace);
                    let mut params = vec![
                        ("spaces", "appDataFolder".to_string()),
                        ("q", query),
                        ("fields", "files(appProperties),nextPageToken".into()),
                        ("pageSize", "1000".into()),
                    ];
                    if let Some(token) = cursor.take() {
                        params.push(("pageToken", token));
                    }
                    json_response(
                        self.authorized(Method::GET, "https://www.googleapis.com/drive/v3/files")?
                            .query(&params)
                            .send()
                            .map_err(|_| anyhow::anyhow!("Falha ao listar Drive"))?,
                    )?
                }
                Provider::Dropbox => {
                    if let Some(token) = cursor.take() {
                        self.json(
                            "https://api.dropboxapi.com/2/files/list_folder/continue",
                            &json!({"cursor":token}),
                        )?
                    } else {
                        let response=self.authorized(Method::POST,"https://api.dropboxapi.com/2/files/list_folder")?.json(&json!({"path":format!("/nethergate/{}",self.namespace),"recursive":false,"limit":1000})).send().map_err(|_|anyhow::anyhow!("Falha ao listar Dropbox"))?;
                        if response.status().as_u16() == 409 {
                            let value: Value = serde_json::from_slice(&bounded(response, 65536)?)?;
                            ensure!(is_not_found(&value), "Listagem Dropbox recusada");
                            return Ok(vec![]);
                        }
                        json_response(response)?
                    }
                }
            };
            match self.provider {
                Provider::GoogleDrive => {
                    for row in value["files"]
                        .as_array()
                        .context("Listagem Drive inválida")?
                    {
                        if let Some(id) = row["appProperties"]["ngKey"]
                            .as_str()
                            .and_then(|s| s.strip_suffix("/manifest.json"))
                        {
                            backups::valid_id(id)?;
                            ids.push(id.into());
                        }
                    }
                    cursor = value["nextPageToken"].as_str().map(str::to_string);
                }
                Provider::Dropbox => {
                    for row in value["entries"]
                        .as_array()
                        .context("Listagem Dropbox inválida")?
                    {
                        if row[".tag"] == "folder" {
                            if let Some(id) = row["name"]
                                .as_str()
                                .filter(|id| backups::valid_id(id).is_ok())
                            {
                                ids.push(id.into());
                            }
                        }
                    }
                    if value["has_more"] == true {
                        cursor = Some(
                            value["cursor"]
                                .as_str()
                                .context("Cursor Dropbox inválido")?
                                .into(),
                        );
                    } else {
                        cursor = None;
                    }
                }
            }
            if cursor.is_none() {
                return Ok(ids);
            }
            ensure!(
                page < 99,
                "Histórico demasiado grande; não foi apresentada uma listagem parcial"
            );
        }
        Ok(ids)
    }
}
fn valid_drive_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn safe_key(key: &str) -> Result<()> {
    ensure!(
        key.len() <= 124
            && key.split('/').all(|p| !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
        "Chave cloud inválida"
    );
    Ok(())
}
fn bounded(response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    response.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Resposta cloud demasiado grande");
    Ok(bytes)
}
fn json_response(response: Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "Pedido cloud recusado (HTTP {}); verifica conta, permissões e espaço disponível",
        response.status().as_u16()
    );
    Ok(serde_json::from_slice(&bounded(
        response,
        4 * 1024 * 1024,
    )?)?)
}
fn is_not_found(value: &Value) -> bool {
    value["error_summary"]
        .as_str()
        .is_some_and(|s| s.starts_with("path/not_found/"))
}

impl Remote for ServiceRemote {
    fn read(&self, key: &str, limit: usize) -> Result<Option<Vec<u8>>> {
        match self.provider {
            Provider::GoogleDrive => {
                let ids = self.drive_ids(key)?;
                let mut result = None;
                for id in ids {
                    let response = self
                        .authorized(
                            Method::GET,
                            &format!("https://www.googleapis.com/drive/v3/files/{id}"),
                        )?
                        .query(&[("alt", "media")])
                        .send()
                        .map_err(|_| anyhow::anyhow!("Falha ao ler Drive"))?;
                    ensure!(
                        response.status().is_success(),
                        "Ficheiro Drive indisponível (HTTP {})",
                        response.status().as_u16()
                    );
                    let bytes = bounded(response, limit)?;
                    if let Some(old) = &result {
                        ensure!(old==&bytes,"Existem ficheiros Drive diferentes para a mesma chave; nada foi substituído");
                    } else {
                        result = Some(bytes);
                    }
                }
                Ok(result)
            }
            Provider::Dropbox => {
                let arg = serde_json::to_string(&json!({"path":self.dropbox_path(key)?}))?;
                let response = self
                    .authorized(
                        Method::POST,
                        "https://content.dropboxapi.com/2/files/download",
                    )?
                    .header("Dropbox-API-Arg", arg)
                    .send()
                    .map_err(|_| anyhow::anyhow!("Falha ao ler Dropbox"))?;
                if response.status().as_u16() == 409 {
                    let value: Value = serde_json::from_slice(&bounded(response, 65536)?)?;
                    ensure!(is_not_found(&value), "Leitura Dropbox recusada");
                    return Ok(None);
                }
                ensure!(
                    response.status().is_success(),
                    "Leitura Dropbox recusada (HTTP {})",
                    response.status().as_u16()
                );
                Ok(Some(bounded(response, limit)?))
            }
        }
    }
    fn write(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        if let Some(existing) = self.read(key, bytes.len())? {
            ensure!(
                existing == bytes,
                "Objeto cloud diferente; não foi substituído"
            );
            return Ok(());
        }
        match self.provider {
            Provider::GoogleDrive => {
                let metadata = json!({"name":format!("ng-{}-{}",self.namespace,hash(key.as_bytes())),"parents":["appDataFolder"],"appProperties":{"ngNamespace":self.namespace,"ngKey":key,"ngKind":if key.ends_with("/manifest.json"){"snapshot"}else{"block"}}});
                let response = self
                    .authorized(
                        Method::POST,
                        "https://www.googleapis.com/upload/drive/v3/files",
                    )?
                    .query(&[("uploadType", "resumable"), ("fields", "id")])
                    .header("X-Upload-Content-Type", "application/octet-stream")
                    .header("X-Upload-Content-Length", bytes.len())
                    .json(&metadata)
                    .send()
                    .map_err(|_| anyhow::anyhow!("Falha ao preparar envio Drive"))?;
                ensure!(
                    response.status().is_success(),
                    "Envio Drive recusado (HTTP {})",
                    response.status().as_u16()
                );
                let location = response
                    .headers()
                    .get("location")
                    .context("Drive não devolveu sessão de upload")?
                    .to_str()?
                    .to_string();
                let url = reqwest::Url::parse(&location)?;
                ensure!(
                    url.path() == "/upload/drive/v3/files"
                        && url.query_pairs().any(|(k, _)| k == "upload_id"),
                    "Sessão Drive inválida"
                );
                json_response(
                    self.authorized(Method::PUT, &location)?
                        .header("Content-Type", "application/octet-stream")
                        .body(bytes.clone())
                        .send()
                        .map_err(|_| {
                            anyhow::anyhow!("Envio Drive interrompido; volta a enviar para retomar")
                        })?,
                )?;
            }
            Provider::Dropbox => {
                let path = self.dropbox_path(key)?;
                self.ensure_dropbox_folders(&path)?;
                let arg = serde_json::to_string(
                    &json!({"path":path,"mode":"add","autorename":false,"mute":true,"strict_conflict":true}),
                )?;
                let response = self
                    .authorized(
                        Method::POST,
                        "https://content.dropboxapi.com/2/files/upload",
                    )?
                    .header("Content-Type", "application/octet-stream")
                    .header("Dropbox-API-Arg", arg)
                    .body(bytes.clone())
                    .send()
                    .map_err(|_| {
                        anyhow::anyhow!("Envio Dropbox interrompido; volta a enviar para retomar")
                    })?;
                if response.status().as_u16() != 409 {
                    json_response(response)?;
                }
            }
        }
        ensure!(
            self.read(key, bytes.len())? == Some(bytes),
            "O objeto cloud não foi confirmado após o envio"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    #[test]
    fn pending_upload_survives_reload_and_namespace_change_resets_it() {
        let root = std::env::temp_dir().join(format!(
            "ng-service-state-{}",
            crate::transfer::key().unwrap()
        ));
        let provider = Provider::GoogleDrive;
        save(
            &root,
            "profile",
            provider,
            Settings {
                namespace: "world-one".into(),
                compression: true,
                automatic: false,
            },
        )
        .unwrap();
        persist(
            &root,
            "profile",
            provider,
            &TransferState {
                pending_backup: Some("123-ab".into()),
                sent_blocks: 3,
                reused_blocks: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let reloaded = state(&root, "profile", provider).unwrap();
        assert_eq!(reloaded.pending_backup.as_deref(), Some("123-ab"));
        assert_eq!(reloaded.sent_blocks, 3);
        save(
            &root,
            "profile",
            provider,
            Settings {
                namespace: "world-two".into(),
                compression: true,
                automatic: false,
            },
        )
        .unwrap();
        assert!(state(&root, "profile", provider)
            .unwrap()
            .pending_backup
            .is_none());
    }
    struct Mock {
        endpoint: String,
        stop: Arc<AtomicBool>,
        join: Option<std::thread::JoinHandle<()>>,
    }
    impl Mock {
        fn new() -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", server.server_addr());
            let stop = Arc::new(AtomicBool::new(false));
            let done = stop.clone();
            let join = std::thread::spawn(move || {
                let mut drive = BTreeMap::<String, (Value, Vec<u8>)>::new();
                let mut pending = BTreeMap::<String, Value>::new();
                let mut dropbox = BTreeMap::<String, Vec<u8>>::new();
                let mut seq = 0;
                while !done.load(Ordering::SeqCst) {
                    let Some(mut request) = server.recv_timeout(Duration::from_millis(50)).unwrap()
                    else {
                        continue;
                    };
                    let header = |name: &str| {
                        request
                            .headers()
                            .iter()
                            .find(|h| h.field.to_string().eq_ignore_ascii_case(name))
                            .map(|h| h.value.as_str().to_string())
                    };
                    if header("Authorization").as_deref() != Some("Bearer test-token") {
                        request.respond(tiny_http::Response::empty(403)).unwrap();
                        continue;
                    }
                    let arg = header("Dropbox-API-Arg")
                        .map(|v| serde_json::from_str::<Value>(&v).unwrap());
                    let url =
                        reqwest::Url::parse(&format!("http://localhost{}", request.url())).unwrap();
                    let query = url.query_pairs().collect::<HashMap<_, _>>();
                    let path = url.path().to_string();
                    let mut body = vec![];
                    request
                        .as_reader()
                        .take(10 * 1024 * 1024)
                        .read_to_end(&mut body)
                        .unwrap();
                    let result: Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> = (|| {
                        let json_reply = |value: Value| {
                            tiny_http::Response::from_data(serde_json::to_vec(&value).unwrap())
                        };
                        if path == "/upload/drive/v3/files" {
                            if request.method() == &tiny_http::Method::Post {
                                seq += 1;
                                let upload = format!("upload-{seq}");
                                pending.insert(upload.clone(), serde_json::from_slice(&body)?);
                                return Ok(json_reply(json!({})).with_header(tiny_http::Header::from_bytes("Location",format!("https://www.googleapis.com/upload/drive/v3/files?upload_id={upload}")).unwrap()));
                            }
                            let upload = query.get("upload_id").context("upload id")?.to_string();
                            let metadata = pending.remove(&upload).context("session")?;
                            let id = format!("file-{seq}");
                            drive.insert(id.clone(), (metadata, body));
                            return Ok(json_reply(json!({"id":id})));
                        }
                        if path == "/drive/v3/files" {
                            let q = query.get("q").context("query")?;
                            let files=drive.iter().filter(|(_, (meta,_))|{
                                let p=&meta["appProperties"];
                                (!q.contains("ngKey") || q.contains(&format!("value='{}'",p["ngKey"].as_str().unwrap()))) && (!q.contains("ngKind") || p["ngKind"]=="snapshot")
                            }).map(|(id,(meta,_))|json!({"id":id,"appProperties":meta["appProperties"]})).collect::<Vec<_>>();
                            return Ok(json_reply(json!({"files":files})));
                        }
                        if let Some(id) = path.strip_prefix("/drive/v3/files/") {
                            return Ok(tiny_http::Response::from_data(
                                drive.get(id).context("file")?.1.clone(),
                            ));
                        }
                        if path == "/2/files/create_folder_v2" {
                            return Ok(json_reply(json!({"metadata":{}})));
                        }
                        if path == "/2/files/download" {
                            let key = arg.as_ref().unwrap()["path"].as_str().unwrap();
                            return Ok(match dropbox.get(key) {
                                Some(bytes) => tiny_http::Response::from_data(bytes.clone()),
                                None => json_reply(json!({"error_summary":"path/not_found/test"}))
                                    .with_status_code(409),
                            });
                        }
                        if path == "/2/files/upload" {
                            let key = arg.as_ref().unwrap()["path"].as_str().unwrap();
                            assert_eq!(arg.as_ref().unwrap()["autorename"], false);
                            assert_eq!(arg.as_ref().unwrap()["mode"], "add");
                            if dropbox.contains_key(key) {
                                return Ok(json_reply(
                                    json!({"error_summary":"path/conflict/file"}),
                                )
                                .with_status_code(409));
                            }
                            dropbox.insert(key.into(), body);
                            return Ok(json_reply(json!({"id":"id:test"})));
                        }
                        if path == "/2/files/list_folder" {
                            let value: Value = serde_json::from_slice(&body)?;
                            let prefix = format!("{}/", value["path"].as_str().unwrap());
                            let mut folders = std::collections::BTreeSet::new();
                            for key in dropbox.keys() {
                                if let Some(name) =
                                    key.strip_prefix(&prefix).and_then(|s| s.split('/').next())
                                {
                                    folders.insert(name);
                                }
                            }
                            return Ok(json_reply(
                                json!({"entries":folders.into_iter().map(|name|json!({".tag":"folder","name":name})).collect::<Vec<_>>(),"has_more":false}),
                            ));
                        }
                        anyhow::bail!("Unexpected endpoint {path}")
                    })(
                    );
                    request
                        .respond(result.unwrap_or_else(|_| {
                            tiny_http::Response::from_data(b"error".to_vec()).with_status_code(500)
                        }))
                        .unwrap();
                }
            });
            Self {
                endpoint,
                stop,
                join: Some(join),
            }
        }
        fn remote(&self, provider: Provider) -> ServiceRemote {
            ServiceRemote {
                test_endpoint: Some(self.endpoint.clone()),
                root: PathBuf::new(),
                provider,
                generation: "test".into(),
                namespace: hash(b"test-namespace"),
                http: accounts::http().unwrap(),
                folders: Mutex::new(Default::default()),
            }
        }
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.join.take().unwrap().join().unwrap();
        }
    }
    #[test]
    fn drive_and_dropbox_protocol_roundtrip_and_resume() {
        for provider in [Provider::GoogleDrive, Provider::Dropbox] {
            let mock = Mock::new();
            let remote = mock.remote(provider);
            let root = std::env::temp_dir()
                .join(format!("ng-provider-{}", crate::transfer::key().unwrap()));
            let server = root.join("server");
            fs::create_dir_all(server.join("world")).unwrap();
            crate::config::write_json(
                &server.join("nethergate.json"),
                &crate::config::Config::new("1.21.11".into(), "https://example.org".into(), 21)
                    .unwrap(),
            )
            .unwrap();
            fs::write(server.join("server.properties"), "level-name=world\n").unwrap();
            fs::write(
                server.join("world/level.dat"),
                vec![42u8; chunks::BLOCK_SIZE + 64],
            )
            .unwrap();
            let store = root.join("backups");
            let copy = backups::create(&store, "source", &server, "manual").unwrap();
            let cancel = AtomicBool::new(false);
            assert!(
                chunks::upload(&remote, &store, "source", &copy.id, true, &cancel, |p| {
                    if p.sent_blocks == 1 {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                })
                .is_err()
            );
            let fresh = mock.remote(provider);
            let mut reused = 0;
            chunks::upload(
                &fresh,
                &store,
                "source",
                &copy.id,
                true,
                &AtomicBool::new(false),
                |p| {
                    reused = p.reused_blocks;
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(reused, 1);
            assert_eq!(fresh.snapshot_ids().unwrap(), vec![copy.id.clone()]);
            let bytes = fresh
                .read(&chunks::snapshot_key(&copy.id), chunks::MANIFEST_LIMIT)
                .unwrap()
                .unwrap();
            chunks::download(
                &fresh,
                &root.join("received"),
                "target",
                &copy.id,
                &bytes,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(
                fs::metadata(
                    root.join("received")
                        .join(&copy.id)
                        .join("worlds/world/level.dat")
                )
                .unwrap()
                .len(),
                (chunks::BLOCK_SIZE + 64) as u64
            );
            assert!(fresh
                .authorized(Method::GET, "https://www.googleapis.com.evil.test/steal")
                .is_err());
        }
    }
}
