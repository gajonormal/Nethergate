//! Desktop OAuth with PKCE, loopback callback and user-bound encrypted tokens.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{
    collections::HashMap,
    net::TcpListener,
    sync::{Mutex, OnceLock},
    time::Instant,
};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    GoogleDrive,
    Dropbox,
}

pub fn allowed_browser_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    matches!(
        (url.host_str().unwrap_or(""), url.path()),
        ("accounts.google.com", "/o/oauth2/v2/auth")
            | ("www.dropbox.com", "/oauth2/authorize")
            | ("console.cloud.google.com", "/apis/credentials")
            | ("www.dropbox.com", "/developers/apps")
    )
}
impl Provider {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "google_drive" => Ok(Self::GoogleDrive),
            "dropbox" => Ok(Self::Dropbox),
            _ => anyhow::bail!("Fornecedor cloud inválido"),
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::GoogleDrive => "google_drive",
            Self::Dropbox => "dropbox",
        }
    }
    fn token_url(self) -> &'static str {
        match self {
            Self::GoogleDrive => "https://oauth2.googleapis.com/token",
            Self::Dropbox => "https://api.dropboxapi.com/oauth2/token",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientConfig {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    config: ClientConfig,
    generation: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}
#[derive(Clone, Default)]
struct Flow {
    id: String,
    pending: bool,
    error: Option<String>,
}
static FLOWS: OnceLock<Mutex<HashMap<PathBuf, Flow>>> = OnceLock::new();
static ACCESS: OnceLock<Mutex<()>> = OnceLock::new();
fn access_guard() -> std::sync::MutexGuard<'static, ()> {
    ACCESS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn flows() -> &'static Mutex<HashMap<PathBuf, Flow>> {
    FLOWS.get_or_init(Default::default)
}
fn folder(root: &Path, provider: Provider) -> PathBuf {
    root.join("hub-data/cloud-accounts").join(provider.key())
}
fn account_path(root: &Path, provider: Provider) -> PathBuf {
    folder(root, provider).join("account.protected")
}
pub(crate) fn http() -> Result<Client> {
    Ok(Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()?)
}
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Não foi possível gerar a autorização"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn load(root: &Path, provider: Provider) -> Result<Option<Account>> {
    let path = account_path(root, provider);
    if !path.try_exists()? {
        return Ok(None);
    }
    ensure!(
        backups::ordinary(&path)?.len() <= 32768,
        "Conta cloud inválida"
    );
    Ok(Some(serde_json::from_slice(&crate::secrets::protect(
        &fs::read(path)?,
        true,
    )?)?))
}
fn save(root: &Path, provider: Provider, account: &Account) -> Result<()> {
    let path = account_path(root, provider);
    fs::create_dir_all(
        path.parent()
            .context("O ficheiro de credenciais cloud não tem pasta pai")?,
    )?;
    let encrypted = crate::secrets::protect(&serde_json::to_vec(account)?, false)?;
    let temporary = path.with_extension(format!("{}.tmp", crate::transfer::key()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&encrypted)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, &path)?;
    Ok(())
}
pub fn status(root: &Path, provider: Provider) -> Result<Value> {
    let account = load(root, provider)?;
    let flow = flows()
        .lock()
        .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
        .get(&account_path(root, provider))
        .cloned()
        .unwrap_or_default();
    Ok(
        json!({"connected":account.is_some(),"clientId":account.map(|a|a.config.client_id),"pending":flow.pending,"error":flow.error}),
    )
}
fn valid_config(config: &ClientConfig) -> Result<()> {
    ensure!(
        !config.client_id.is_empty()
            && config.client_id.len() <= 512
            && config
                .client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)),
        "Client ID/App Key inválido"
    );
    ensure!(
        config.client_secret.len() <= 2048
            && config
                .client_secret
                .bytes()
                .all(|b| (33..=126).contains(&b)),
        "Client secret inválido"
    );
    Ok(())
}
fn auth_url(
    provider: Provider,
    config: &ClientConfig,
    redirect: &str,
    state: &str,
    verifier: &str,
) -> Result<String> {
    let mut url = reqwest::Url::parse(match provider {
        Provider::GoogleDrive => "https://accounts.google.com/o/oauth2/v2/auth",
        Provider::Dropbox => "https://www.dropbox.com/oauth2/authorize",
    })?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect)
            .append_pair("state", state)
            .append_pair("code_challenge_method", "S256")
            .append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            );
        match provider {
            Provider::GoogleDrive => {
                q.append_pair("scope", "https://www.googleapis.com/auth/drive.appdata")
                    .append_pair("access_type", "offline")
                    .append_pair("prompt", "consent");
            }
            Provider::Dropbox => {
                q.append_pair(
                    "scope",
                    "files.content.read files.content.write files.metadata.read",
                )
                .append_pair("token_access_type", "offline");
            }
        }
    }
    Ok(url.into())
}
fn token_response(response: Response) -> Result<Value> {
    ensure!(response.status().is_success(),"Autorização recusada pelo fornecedor (HTTP {}); revê o registo OAuth ou volta a ligar a conta",response.status().as_u16());
    let mut bytes = vec![];
    response.take(32769).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 32768, "Resposta OAuth demasiado grande");
    Ok(serde_json::from_slice(&bytes)?)
}
fn update_token(account: &mut Account, value: &Value, require_refresh: bool) -> Result<()> {
    let access = value["access_token"]
        .as_str()
        .context("O fornecedor não devolveu um token")?;
    ensure!(
        !access.is_empty()
            && access.len() <= 4096
            && access.bytes().all(|b| (33..=126).contains(&b)),
        "Token inválido"
    );
    let seconds = value["expires_in"]
        .as_u64()
        .context("Falta a validade do token")?;
    ensure!(
        (1..=86400 * 30).contains(&seconds),
        "Validade OAuth inválida"
    );
    account.access_token = access.into();
    account.expires_at = backup_schedule::now()?.saturating_add(seconds * 1000);
    if let Some(refresh) = value["refresh_token"].as_str() {
        ensure!(
            !refresh.is_empty() && refresh.len() <= 4096 && !refresh.chars().any(char::is_control),
            "Refresh token inválido"
        );
        account.refresh_token = refresh.into();
    }
    ensure!(
        !require_refresh || !account.refresh_token.is_empty(),
        "O fornecedor não concedeu acesso offline; revê a autorização"
    );
    Ok(())
}
pub fn begin(root: &Path, provider: Provider, config: ClientConfig) -> Result<Value> {
    valid_config(&config)?;
    ensure!(
        cfg!(windows),
        "As contas protegidas estão disponíveis no Windows nesta versão"
    );
    let path = account_path(root, provider);
    let mut running = flows()
        .lock()
        .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
    ensure!(
        !running.get(&path).is_some_and(|f| f.pending),
        "Já existe uma autorização em curso"
    );
    let listener = TcpListener::bind(match provider {
        Provider::GoogleDrive => "127.0.0.1:0",
        Provider::Dropbox => "127.0.0.1:53682",
    })
    .context("Não foi possível abrir a ligação OAuth local; fecha outra autorização em curso")?;
    let address = listener.local_addr()?;
    let redirect = format!("http://{address}/oauth/callback");
    let state = random()?;
    let verifier = random()?;
    let url = auth_url(provider, &config, &redirect, &state, &verifier)?;
    let flow_id = crate::transfer::key()?;
    running.insert(
        path.clone(),
        Flow {
            id: flow_id.clone(),
            pending: true,
            error: None,
        },
    );
    drop(running);
    let root = root.to_path_buf();
    let reply = json!({"url":url,"redirectUri":redirect});
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let server = tiny_http::Server::from_listener(listener, None)
                .map_err(|_| anyhow::anyhow!("Não foi possível receber a autorização"))?;
            let deadline = Instant::now() + Duration::from_secs(600);
            let mut requests = 0;
            while Instant::now() < deadline && requests < 100 {
                if !flows()
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                    .get(&path)
                    .is_some_and(|f| f.pending && f.id == flow_id)
                {
                    anyhow::bail!("Autorização cancelada");
                }
                let Some(request) = server.recv_timeout(Duration::from_millis(250))? else {
                    continue;
                };
                requests += 1;
                let code = callback_code(&request, &address.to_string(), &state);
                let code = match code {
                    Ok(Some(code)) => code,
                    Ok(None) => {
                        let _ = request.respond(
                            tiny_http::Response::from_string("Pedido OAuth recusado")
                                .with_status_code(403),
                        );
                        continue;
                    }
                    Err(e) => {
                        let _ = request.respond(
                            tiny_http::Response::from_string(
                                "Autorizacao recusada. Volta ao Nethergate.",
                            )
                            .with_status_code(400),
                        );
                        return Err(e);
                    }
                };
                let mut params = vec![
                    ("client_id", config.client_id.clone()),
                    ("grant_type", "authorization_code".into()),
                    ("code", code),
                    ("code_verifier", verifier.clone()),
                    ("redirect_uri", redirect.clone()),
                ];
                if !config.client_secret.is_empty() {
                    params.push(("client_secret", config.client_secret.clone()));
                }
                let value = token_response(
                    http()?
                        .post(provider.token_url())
                        .form(&params)
                        .send()
                        .map_err(|_| anyhow::anyhow!("Falha ao trocar a autorização OAuth"))?,
                )?;
                let mut account = Account {
                    config: config.clone(),
                    generation: crate::transfer::key()?,
                    access_token: String::new(),
                    refresh_token: String::new(),
                    expires_at: 0,
                };
                update_token(&mut account, &value, true)?;
                let _access = access_guard();
                let folder = folder(&root, provider);
                fs::create_dir_all(&folder)?;
                let _guard = crate::config::lock(&folder)?;
                let mut flows = flows()
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
                ensure!(
                    flows
                        .get(&path)
                        .is_some_and(|f| f.pending && f.id == flow_id),
                    "Autorização cancelada"
                );
                save(&root, provider, &account)?;
                flows
                    .get_mut(&path)
                    .context("A autorização OAuth deixou de estar disponível")?
                    .pending = false;
                drop(flows);
                let _ = request.respond(tiny_http::Response::from_string(
                    "Conta ligada. Podes fechar esta pagina e voltar ao Nethergate.",
                ));
                return Ok(());
            }
            anyhow::bail!("A autorização expirou; tenta novamente")
        })();
        if let Err(error) = result {
            let mut flows = flows()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(flow) = flows
                .get_mut(&path)
                .filter(|f| f.id == flow_id && f.pending)
            {
                flow.pending = false;
                flow.error = Some(format!("{error:#}"));
            }
        }
    });
    Ok(reply)
}
fn callback_code(request: &tiny_http::Request, host: &str, state: &str) -> Result<Option<String>> {
    if request.method() != &tiny_http::Method::Get
        || request.url().len() > 8192
        || !request.remote_addr().is_some_and(|a| a.ip().is_loopback())
    {
        return Ok(None);
    }
    if !request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Host") && h.value.as_str() == host)
    {
        return Ok(None);
    }
    let url = reqwest::Url::parse(&format!("http://{host}{}", request.url()))?;
    if url.path() != "/oauth/callback" {
        return Ok(None);
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    if pairs.iter().filter(|(k, _)| k == "state").count() != 1
        || !pairs.iter().any(|(k, v)| k == "state" && v == state)
    {
        return Ok(None);
    }
    ensure!(
        !pairs.iter().any(|(k, _)| k == "error"),
        "Autorização recusada no browser"
    );
    ensure!(
        pairs.iter().filter(|(k, _)| k == "code").count() == 1,
        "Código OAuth em falta"
    );
    let code = pairs
        .into_iter()
        .find(|(k, _)| k == "code")
        .context("O callback OAuth não contém o código de autorização")?
        .1
        .into_owned();
    ensure!(
        !code.is_empty() && code.len() <= 4096,
        "Código OAuth inválido"
    );
    Ok(Some(code))
}
pub fn disconnect(root: &Path, provider: Provider) -> Result<()> {
    let _access = access_guard();
    let folder = folder(root, provider);
    fs::create_dir_all(&folder)?;
    let _guard = crate::config::lock(&folder)?;
    if let Some(flow) = flows()
        .lock()
        .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
        .get_mut(&account_path(root, provider))
    {
        flow.pending = false;
        flow.error = None;
    }
    let path = account_path(root, provider);
    if path.try_exists()? {
        backups::ordinary(&path)?;
        fs::remove_file(path)?;
    }
    Ok(())
}
pub(crate) fn generation(root: &Path, provider: Provider) -> Result<String> {
    Ok(load(root, provider)?
        .context("Liga primeiro a conta cloud")?
        .generation)
}
pub(crate) fn access_token(root: &Path, provider: Provider, generation: &str) -> Result<String> {
    let _access = access_guard();
    let folder = folder(root, provider);
    let _guard = crate::config::lock(&folder)?;
    let mut account = load(root, provider)?.context("A conta foi desligada")?;
    ensure!(
        account.generation == generation,
        "A conta mudou durante a operação; volta a iniciar o envio"
    );
    if account.expires_at <= backup_schedule::now()?.saturating_add(60000) {
        let mut params = vec![
            ("client_id", account.config.client_id.clone()),
            ("grant_type", "refresh_token".into()),
            ("refresh_token", account.refresh_token.clone()),
        ];
        if !account.config.client_secret.is_empty() {
            params.push(("client_secret", account.config.client_secret.clone()));
        }
        let value = token_response(
            http()?
                .post(provider.token_url())
                .form(&params)
                .send()
                .map_err(|_| anyhow::anyhow!("Falha ao renovar a ligação cloud"))?,
        )?;
        update_token(&mut account, &value, false)?;
        save(root, provider, &account)?;
    }
    Ok(account.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_scopes_and_browser_origins_are_restricted() {
        let config = ClientConfig {
            client_id: "example.apps.googleusercontent.com".into(),
            client_secret: String::new(),
        };
        let url = reqwest::Url::parse(
            &auth_url(
                Provider::GoogleDrive,
                &config,
                "http://127.0.0.1:1234/oauth/callback",
                "state",
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            )
            .unwrap(),
        )
        .unwrap();
        let query = url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(
            query["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(
            query["scope"],
            "https://www.googleapis.com/auth/drive.appdata"
        );
        assert!(allowed_browser_url(url.as_str()));
        for value in [
            "https://accounts.google.com.evil.test/o/oauth2/v2/auth",
            "http://accounts.google.com/o/oauth2/v2/auth",
            "https://user@www.dropbox.com/oauth2/authorize",
            "https://www.dropbox.com:444/oauth2/authorize",
            "https://www.dropbox.com/oauth2/authorize#token",
        ] {
            assert!(!allowed_browser_url(value));
        }
    }
    #[test]
    fn callback_requires_matching_state_host_path_and_single_code() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap().to_string();
        let host = address.clone();
        let thread = std::thread::spawn(move || {
            let mut accepted = vec![];
            for _ in 0..5 {
                let request = server.recv().unwrap();
                accepted.push(callback_code(&request, &host, "expected").ok().flatten());
                request.respond(tiny_http::Response::empty(200)).unwrap();
            }
            accepted
        });
        for path in [
            "/oauth/callback?state=wrong&code=secret",
            "/oauth/callback?state=expected&state=wrong&code=secret",
            "/wrong?state=expected&code=secret",
            "/oauth/callback?state=expected&code=a&code=b",
            "/oauth/callback?state=expected&code=valid%2Bcode",
        ] {
            http()
                .unwrap()
                .get(format!("http://{address}{path}"))
                .send()
                .unwrap();
        }
        let accepted = thread.join().unwrap();
        assert_eq!(
            accepted.into_iter().flatten().collect::<Vec<_>>(),
            vec!["valid+code"]
        );
    }
    #[test]
    #[cfg(windows)]
    fn encrypted_accounts_hide_tokens_and_refuse_old_account_generation() {
        let root =
            std::env::temp_dir().join(format!("ng-oauth-{}", crate::transfer::key().unwrap()));
        let mut account = Account {
            config: ClientConfig {
                client_id: "test-client".into(),
                client_secret: "test-secret".into(),
            },
            generation: "first".into(),
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
        };
        update_token(&mut account,&json!({"access_token":"private-access-token","refresh_token":"private-refresh-token","expires_in":3600}),true).unwrap();
        save(&root, Provider::Dropbox, &account).unwrap();
        let public = status(&root, Provider::Dropbox).unwrap().to_string();
        for secret in [
            &account.access_token,
            &account.refresh_token,
            &account.config.client_secret,
        ] {
            assert!(!public.contains(secret));
        }
        let encrypted = fs::read(account_path(&root, Provider::Dropbox)).unwrap();
        assert!(!encrypted
            .windows(account.access_token.len())
            .any(|s| s == account.access_token.as_bytes()));
        assert!(access_token(&root, Provider::Dropbox, "old-generation").is_err());
        assert_eq!(
            access_token(&root, Provider::Dropbox, "first").unwrap(),
            account.access_token
        );
        disconnect(&root, Provider::Dropbox).unwrap();
        assert_eq!(
            status(&root, Provider::Dropbox).unwrap()["connected"],
            false
        );
    }
}
