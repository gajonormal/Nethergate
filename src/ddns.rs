use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    label: String,
    port: u16,
    automatic: bool,
    protected_token: Vec<u8>,
}
#[derive(Default)]
struct Session {
    attempted: Option<Instant>,
    checked: Option<u128>,
    published: Option<Ipv4Addr>,
    error: Option<String>,
    network_stale: bool,
    wan_changed_at: Option<u128>,
}
#[derive(Default)]
pub struct Ddns {
    sessions: HashMap<String, Session>,
    network_revision: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDdns {
    pub hostname: String,
    #[serde(default)]
    pub token: String,
    pub external_port: u16,
    pub automatic: bool,
    pub consent: bool,
}

fn label(input: &str) -> Result<String> {
    let value = input.trim().to_ascii_lowercase();
    let value = value.strip_suffix(".duckdns.org").unwrap_or(&value);
    if value.is_empty()
        || value.len() > 63
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("Indica apenas o nome DuckDNS que criaste, por exemplo o-meu-mundo.duckdns.org");
    }
    Ok(value.into())
}
fn validate_token(token: &str) -> Result<()> {
    if token.len() != 36
        || !token.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        bail!("O token DuckDNS tem de ser o identificador completo mostrado na tua conta");
    }
    Ok(())
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        bail!("Identificador DDNS inválido");
    }
    Ok(root.join("hub-data/ddns").join(format!("{id}.json")))
}
fn load(root: &Path, id: &str) -> Result<Option<Settings>> {
    let path = path(root, id)?;
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path)?;
    if data.len() > 32768 {
        bail!("Configuração DDNS inválida");
    }
    let settings: Settings =
        serde_json::from_slice(&data).context("Não foi possível ler a configuração DDNS")?;
    if label(&settings.label)? != settings.label
        || settings.port == 0
        || settings.protected_token.is_empty()
    {
        bail!("Configuração DDNS inválida");
    }
    Ok(Some(settings))
}
fn parse_update(body: &str, expected: Ipv4Addr) -> Result<()> {
    let lines: Vec<_> = body.lines().map(str::trim).collect();
    if lines.len() != 4
        || lines[0] != "OK"
        || lines[1] != expected.to_string()
        || !["UPDATED", "NOCHANGE"].contains(&lines[3])
    {
        bail!("O DuckDNS não confirmou a atualização para este IPv4. Verifica o nome e o token na tua conta");
    }
    Ok(())
}
fn publish(settings: &Settings) -> Result<Ipv4Addr> {
    let secret = crate::secrets::protect(&settings.protected_token, true)?;
    let token = String::from_utf8(secret).context("Credencial DDNS inválida")?;
    validate_token(&token)?;
    let client = Client::builder()
        .no_proxy()
        .user_agent(concat!("Nethergate/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| anyhow::anyhow!("Não foi possível preparar a ligação DDNS"))?;
    let response = client
        .get("https://api.ipify.org?format=json")
        .send()
        .map_err(|_| anyhow::anyhow!("Não foi possível consultar o IPv4 público"))?
        .error_for_status()
        .map_err(|_| anyhow::anyhow!("O serviço de IPv4 público não está disponível"))?;
    let mut body = String::new();
    response
        .take(4096)
        .read_to_string(&mut body)
        .context("Falha ao ler o IPv4 público")?;
    let ip: Ipv4Addr = serde_json::from_str::<Value>(&body)?
        .get("ip")
        .and_then(Value::as_str)
        .context("IPv4 público ausente")?
        .parse()?;
    if !crate::internet::public_ipv4(ip) {
        bail!("O endereço detetado não é um IPv4 público utilizável");
    }
    // Never propagate reqwest errors here: they may contain the secret-bearing URL.
    let response = client
        .get("https://www.duckdns.org/update")
        .query(&[
            ("domains", settings.label.as_str()),
            ("token", token.as_str()),
            ("ip", &ip.to_string()),
            ("verbose", "true"),
        ])
        .send()
        .map_err(|_| anyhow::anyhow!("Não foi possível contactar o DuckDNS. Tenta mais tarde"))?
        .error_for_status()
        .map_err(|_| {
            anyhow::anyhow!("O DuckDNS recusou o pedido. Verifica a conta e tenta mais tarde")
        })?;
    let mut body = String::new();
    response
        .take(4096)
        .read_to_string(&mut body)
        .map_err(|_| anyhow::anyhow!("Falha ao ler a confirmação DuckDNS"))?;
    parse_update(&body, ip)?;
    Ok(ip)
}
impl Ddns {
    pub fn status(&self, root: &Path, id: &str) -> Result<Value> {
        let settings = load(root, id)?;
        let session = self.sessions.get(id);
        Ok(
            json!({"supported":cfg!(windows),"configured":settings.is_some(),"hostname":settings.as_ref().map(|s|format!("{}.duckdns.org",s.label)),"externalPort":settings.as_ref().map(|s|s.port),"automatic":settings.as_ref().is_some_and(|s|s.automatic),"hasToken":settings.is_some(),"lastPublishedIp":session.and_then(|s|s.published),"lastSuccess":session.and_then(|s|s.checked),"error":session.and_then(|s|s.error.as_ref()),"intervalSeconds":300,"connectionVerified":false,
                "needsNetworkUpdate":session.is_some_and(|s|s.network_stale),"wanChangedAt":session.and_then(|s|s.wan_changed_at)}),
        )
    }
    pub fn save(&mut self, root: &Path, id: &str, input: SaveDdns) -> Result<Value> {
        let SaveDdns {
            hostname,
            token,
            external_port: port,
            automatic,
            consent,
        } = input;
        if !consent {
            bail!("Confirma o uso do ipify e DuckDNS para publicar o endereço");
        }
        if !cfg!(windows) || port == 0 {
            bail!("É necessário Windows e uma porta externa válida");
        }
        let label = label(&hostname)?;
        let old = load(root, id)?;
        let token = token.trim();
        let encrypted = if token.is_empty() {
            old.filter(|s| s.label == label)
                .context("Introduz o token para este nome DuckDNS")?
                .protected_token
        } else {
            validate_token(token)?;
            crate::secrets::protect(token.as_bytes(), false)?
        };
        let settings = Settings {
            label,
            port,
            automatic,
            protected_token: encrypted,
        };
        let file = path(root, id)?;
        fs::create_dir_all(file.parent().context("O ficheiro DDNS não tem pasta pai")?)?;
        crate::hub::replace_json(&file, &settings)?;
        let session = self.sessions.entry(id.into()).or_default();
        session.checked = None;
        session.published = None;
        session.error = None;
        session.wan_changed_at = None;
        self.status(root, id)
    }
    pub fn pause(&mut self, root: &Path, id: &str) -> Result<Value> {
        let mut settings = load(root, id)?.context("Ainda não configuraste DDNS")?;
        settings.automatic = false;
        crate::hub::replace_json(&path(root, id)?, &settings)?;
        self.status(root, id)
    }
    pub fn update(&mut self, root: &Path, id: &str, expected: Option<&str>) -> Result<Value> {
        let settings = load(root, id)?.context("Configura primeiro o teu nome DuckDNS")?;
        if let Some(expected) = expected {
            if expected != format!("{}.duckdns.org", settings.label) {
                bail!("O nome configurado mudou. Atualiza o painel antes de publicar");
            }
        } else if !settings.automatic {
            return self.status(root, id);
        }
        let session = self.sessions.entry(id.into()).or_default();
        let interval = if expected.is_some() || session.network_stale {
            30
        } else {
            300
        };
        if session
            .attempted
            .is_some_and(|t| t.elapsed() < Duration::from_secs(interval))
        {
            if expected.is_some() {
                bail!("Aguarda 30 segundos antes de repetir a atualização DDNS");
            }
            return self.status(root, id);
        }
        session.attempted = Some(Instant::now());
        match publish(&settings) {
            Ok(ip) => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
                if session.published.is_some_and(|previous| previous != ip) {
                    session.wan_changed_at = Some(now);
                }
                session.published = Some(ip);
                session.checked = Some(now);
                session.error = None;
                session.network_stale = false;
            }
            Err(error) => {
                session.error = Some(format!("{error:#}"));
                return Err(error);
            }
        }
        self.status(root, id)
    }
    pub fn observe_network(&mut self, revision: u64, ids: &[String]) {
        if revision == self.network_revision {
            return;
        }
        self.network_revision = revision;
        for id in ids {
            self.sessions.entry(id.clone()).or_default().network_stale = true;
        }
    }
    pub fn tick(&mut self, root: &Path, ids: &[String]) {
        for id in ids {
            if load(root, id).ok().flatten().is_some_and(|s| s.automatic) {
                let _ = self.update(root, id, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_changes_mark_records_stale_without_enabling_publication() {
        let root = std::env::temp_dir().join(format!("ng-ddns-network-{}", std::process::id()));
        let mut dns = Ddns::default();
        dns.observe_network(1, &["fixture".into()]);
        let status = dns.status(&root, "fixture").unwrap();
        assert_eq!(status["needsNetworkUpdate"], true);
        assert_eq!(status["configured"], false);
        assert_eq!(status["automatic"], false);
        dns.tick(&root, &["fixture".into()]);
        assert!(dns.sessions["fixture"].attempted.is_none());
        dns.sessions.get_mut("fixture").unwrap().network_stale = false;
        dns.observe_network(1, &["fixture".into()]);
        assert!(!dns.sessions["fixture"].network_stale);
        dns.observe_network(2, &["fixture".into()]);
        assert!(dns.sessions["fixture"].network_stale);
    }
    #[test]
    fn restricts_domains_tokens_and_success_responses() {
        assert_eq!(label(" My-World.duckdns.org ").unwrap(), "my-world");
        for input in [
            "",
            "a.example.org",
            "a,b",
            "a/b",
            "-world",
            "world-",
            "https://example.org",
            "a\n&token=x",
        ] {
            assert!(label(input).is_err());
        }
        assert!(validate_token("01234567-89ab-cdef-0123-456789abcdef").is_ok());
        assert!(validate_token("secret&ip=1.1.1.1").is_err());
        let ip = "8.8.8.8".parse().unwrap();
        assert!(parse_update("OK\n8.8.8.8\n\nUPDATED\n", ip).is_ok());
        assert!(parse_update("OK\n8.8.8.8\n\nNOCHANGE\n", ip).is_ok());
        for body in ["KO", "OK", "OK\n1.1.1.1\n\nUPDATED", "OK\n8.8.8.8\n\nMAYBE"] {
            assert!(parse_update(body, ip).is_err());
        }
    }
    #[test]
    fn local_status_and_refused_save_have_no_secrets_or_side_effects() {
        let root = std::env::temp_dir().join(format!("ng-ddns-{}", std::process::id()));
        let mut dns = Ddns::default();
        let status = dns.status(&root, "test").unwrap();
        assert_eq!(status["configured"], false);
        assert!(dns
            .save(
                &root,
                "test",
                SaveDdns {
                    hostname: "test".into(),
                    token: "secret".into(),
                    external_port: 25565,
                    automatic: true,
                    consent: false
                }
            )
            .is_err());
        assert!(!root.join("hub-data/ddns/test.json").exists());
        assert!(path(&root, "../escape").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn encrypted_settings_pause_and_stale_requests_do_not_publish() {
        let root = std::env::temp_dir().join(format!("ng-ddns-settings-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let token = "01234567-89ab-cdef-0123-456789abcdef";
        let mut dns = Ddns::default();
        let input = |name: &str, token: &str| SaveDdns {
            hostname: name.into(),
            token: token.into(),
            external_port: 25566,
            automatic: true,
            consent: true,
        };
        let status = dns
            .save(&root, "fixture", input("test-only", token))
            .unwrap();
        let data = fs::read(path(&root, "fixture").unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&data).contains(token));
        assert!(!status.to_string().contains(token));
        let settings = load(&root, "fixture").unwrap().unwrap();
        assert_eq!(
            crate::secrets::protect(&settings.protected_token, true).unwrap(),
            token.as_bytes()
        );
        assert!(dns
            .update(&root, "fixture", Some("other.duckdns.org"))
            .is_err());
        dns.sessions.entry("fixture".into()).or_default().attempted = Some(Instant::now());
        assert!(dns
            .update(&root, "fixture", Some("test-only.duckdns.org"))
            .is_err());
        assert!(dns.save(&root, "fixture", input("different", "")).is_err());
        dns.save(&root, "fixture", input("test-only", "")).unwrap();
        assert!(dns
            .update(&root, "fixture", Some("test-only.duckdns.org"))
            .is_err());
        let paused = dns.pause(&root, "fixture").unwrap();
        assert_eq!(paused["automatic"], false);
        dns.update(&root, "fixture", None).unwrap();
        dns.tick(&root, &["fixture".into()]);
        assert_eq!(
            dns.status(&root, "fixture").unwrap()["lastSuccess"],
            Value::Null
        );
    }
}
