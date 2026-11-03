//! Pinned official agent and explicit account claim. Credentials never leave the backend.
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

const SHA256: &str = "9b00d6ff7d37d1052e5ae097e1348e11deae8617cd7a8ba39d1777f2006316a3";
const SIZE: usize = 5094152;
const DOWNLOAD: &str = "https://github.com/playit-cloud/playit-agent/releases/download/v0.17.1/playit-windows-x86_64-signed.exe";
#[derive(Default, Serialize, Deserialize)]
struct Account {
    claim: Option<String>,
    secret: Option<String>,
    tunnel: Option<Tunnel>,
    #[serde(default)]
    claim_until: Option<u128>,
    #[serde(default)]
    error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Tunnel {
    pub id: String,
    pub address: String,
    pub target: SocketAddr,
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        bail!("Identificador de servidor inválido");
    }
    Ok(root.join("hub-data/playit").join(format!("{id}.protected")))
}
fn load(root: &Path, id: &str) -> Result<Account> {
    let path = path(root, id)?;
    if !path.exists() {
        return Ok(Account::default());
    }
    let bytes = fs::read(path)?;
    if bytes.len() > 32768 {
        bail!("Configuração playit inválida");
    }
    serde_json::from_slice(&crate::secrets::protect(&bytes, true)?)
        .context("Não foi possível ler a associação playit")
}
fn save(root: &Path, id: &str, account: &Account) -> Result<()> {
    let path = path(root, id)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração playit não tem pasta pai")?,
    )?;
    let bytes = crate::secrets::protect(&serde_json::to_vec(account)?, false)?;
    let staging = path.with_extension("protected.tmp");
    fs::write(&staging, bytes)?;
    fs::rename(staging, path)?;
    Ok(())
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .no_proxy()
        .user_agent("Nethergate/0.1 playit integration")
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn decode(value: Value) -> Result<Value> {
    if value["status"] != "success" {
        // Never include raw responses: errors can contain credentials or account data.
        let code = value
            .pointer("/data/error")
            .and_then(Value::as_str)
            .or_else(|| value["data"].as_str())
            .unwrap_or("");
        if matches!(
            code,
            "AgentVersionUnknown"
                | "RequiresVerifiedAccount"
                | "AgentNotFound"
                | "InvalidConfig"
                | "InvalidTunnelConfig"
                | "AgentSchemaNotFound"
                | "TunnelsLimitReached"
                | "InvalidAgentKey"
        ) {
            bail!("O playit recusou a configuração ({code}). Verifica o agente e a conta no site oficial.");
        }
        bail!("O playit não aceitou o pedido. Confirma a associação, a verificação do email e os limites da conta no site oficial.");
    }
    value
        .get("data")
        .cloned()
        .context("Resposta playit inválida")
}
fn call(route: &str, secret: Option<&str>, body: Value) -> Result<Value> {
    let mut request = client()?
        .post(format!("https://api.playit.gg{route}"))
        .json(&body);
    if let Some(secret) = secret {
        request = request.header("Authorization", format!("Agent-Key {secret}"));
    }
    let response = request
        .send()
        .context("Não foi possível contactar o playit")?;
    let mut bytes = Vec::new();
    response.take(1048577).read_to_end(&mut bytes)?;
    if bytes.len() > 1048576 {
        bail!("Resposta playit demasiado grande");
    }
    decode(serde_json::from_slice(&bytes).context("Resposta playit inválida")?)
}
pub fn binary(root: &Path) -> PathBuf {
    root.join("runtimes/playit/0.17.1/playit.exe")
}
pub fn verify(bytes: &[u8]) -> Result<()> {
    if bytes.len() != SIZE || format!("{:x}", Sha256::digest(bytes)) != SHA256 {
        bail!(
            "O agente playit não corresponde ao SHA-256 oficial. Prepara novamente a integração."
        );
    }
    Ok(())
}
pub fn install(root: &Path) -> Result<()> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        bail!("Esta integração playit suporta Windows x64");
    }
    let path = binary(root);
    if path.exists() {
        return verify(&fs::read(path)?);
    }
    // GitHub release assets redirect to their download CDN. Only HTTPS redirects are accepted.
    let download = Client::builder()
        .no_proxy()
        .https_only(true)
        .user_agent("Nethergate/0.1 playit download")
        .timeout(Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let mut bytes = Vec::new();
    download
        .get(DOWNLOAD)
        .send()?
        .error_for_status()?
        .take((SIZE + 1) as u64)
        .read_to_end(&mut bytes)?;
    verify(&bytes)?;
    fs::create_dir_all(
        path.parent()
            .context("A configuração playit não tem pasta pai")?,
    )?;
    let stage = path.with_extension("exe.part");
    fs::write(&stage, bytes)?;
    fs::rename(stage, path)?;
    Ok(())
}
pub fn status(root: &Path, id: &str) -> Result<Value> {
    let account = load(root, id)?;
    Ok(
        json!({"associated":account.secret.is_some(),"ready":account.tunnel.is_some(),
        "claimUrl":account.claim.map(|c| format!("https://playit.gg/claim/{c}")),
        "address":account.tunnel.as_ref().map(|t| &t.address),
        "target":account.tunnel.as_ref().map(|t| t.target.to_string()),"error":account.error}),
    )
}
pub fn prepare(root: &Path, id: &str) -> Result<Value> {
    install(root)?;
    let mut account = load(root, id)?;
    if account.secret.is_some() {
        return status(root, id);
    }
    if account.claim_until.is_some_and(|deadline| deadline < now()) {
        account.claim = None;
    }
    if account.claim.is_none() {
        let mut random = [0; 5];
        getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("{e}"))?;
        account.claim = Some(random.iter().map(|b| format!("{b:02x}")).collect());
    }
    account.claim_until = Some(now() + 600_000);
    account.error = None;
    let result = call(
        "/claim/setup",
        None,
        json!({"code":account.claim,"agent_type":"self-managed","version":"Nethergate playit-cli 0.17.1"}),
    )?;
    if result == "UserRejected" {
        bail!("A associação foi recusada no playit");
    }
    save(root, id, &account)?;
    status(root, id)
}
fn now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn heartbeat(root: &Path, id: &str, target: SocketAddr) -> Result<()> {
    let mut account = load(root, id)?;
    let Some(claim) = account.claim.clone() else {
        return Ok(());
    };
    if account.claim_until.is_some_and(|deadline| deadline < now()) {
        return Ok(());
    }
    let state = call(
        "/claim/setup",
        None,
        json!({"code":claim,"agent_type":"self-managed","version":"Nethergate playit-cli 0.17.1"}),
    )?;
    if state == "UserAccepted" {
        if let Err(error) = confirm(root, id, target) {
            account = load(root, id)?;
            account.error = Some(error.to_string());
            save(root, id, &account)?;
        }
    }
    Ok(())
}
#[cfg(test)]
fn fields(target: SocketAddr) -> Value {
    json!({"fields":[{"name":"local_ip","value":target.ip().to_string()}, {"name":"local_port","value":target.port().to_string()}]})
}
fn matches_fields(value: &Value, target: SocketAddr) -> bool {
    let Some(fields) = value["fields"].as_array() else {
        return false;
    };
    if fields.len() != 2 {
        return false;
    }
    let entries = fields
        .iter()
        .filter_map(|f| Some((f["name"].as_str()?, f["value"].as_str()?)))
        .collect::<std::collections::BTreeMap<_, _>>();
    entries.len() == 2
        && entries.get("local_ip").copied() == Some(target.ip().to_string().as_str())
        && entries.get("local_port").copied() == Some(target.port().to_string().as_str())
}
fn checked_tunnel(value: &Value, target: SocketAddr) -> Result<Tunnel> {
    if value["port_type"] != "tcp"
        || value["port_count"] != 1
        || value["tunnel_type"] != "minecraft-java"
        || !value["disabled_reason"].is_null()
        || !matches_fields(&value["agent_config"], target)
    {
        bail!("A configuração do túnel playit não corresponde exclusivamente ao IP e porta deste servidor. Verifica a integração.");
    }
    let address = value["display_address"]
        .as_str()
        .context("Falta o endereço playit")?;
    if !valid_address(address) {
        bail!("O playit devolveu um endereço inválido");
    }
    Ok(Tunnel {
        id: value["id"]
            .as_str()
            .context("Falta o identificador do túnel")?
            .into(),
        address: address.into(),
        target,
    })
}
fn valid_address(address: &str) -> bool {
    !address.is_empty()
        && address.len() < 300
        && address
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b':'))
        && !address.starts_with('-')
        && !address.contains("..")
}
pub fn confirm(root: &Path, id: &str, target: SocketAddr) -> Result<Value> {
    let mut account = load(root, id)?;
    if account.secret.is_none() {
        let claim = account
            .claim
            .as_ref()
            .context("Prepara primeiro a integração playit")?;
        let state = call(
            "/claim/setup",
            None,
            json!({"code":claim,"agent_type":"self-managed","version":"Nethergate playit-cli 0.17.1"}),
        )?;
        if state != "UserAccepted" {
            bail!("Abre a ligação oficial, entra na tua conta playit e aprova o agente; depois verifica novamente.");
        }
        let exchange = call("/claim/exchange", None, json!({"code":claim}))?;
        let secret = exchange["secret_key"]
            .as_str()
            .context("O playit não devolveu a credencial")?;
        if !(32..=256).contains(&secret.len()) || !secret.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("Credencial playit inválida");
        }
        account.secret = Some(secret.into());
        account.claim = None;
        save(root, id, &account)?; // Persist the single-use exchange before any other network operation.
    }
    let secret = account
        .secret
        .as_deref()
        .context("A conta playit não contém a credencial do agente")?;
    let name = format!("NetherGate-{id}");
    let mut data = call("/v1/agents/rundata", Some(secret), json!({}))?;
    let tunnels = data["tunnels"]
        .as_array()
        .context("Resposta playit sem túneis")?;
    if tunnels.is_empty() && data["pending"].as_array().is_some_and(|p| p.is_empty()) {
        // The provider needs the official agent's version/schema registration before
        // it can validate a newly allocated tunnel. No tunnels exist at this point.
        verify(&fs::read(binary(root))?)?;
        let mut bootstrap = std::process::Command::new(binary(root));
        bootstrap
            .args(["--stdout", "start"])
            .current_dir(root.join("hub-data/playit"))
            .env("PLAYIT_SECRET", secret)
            .env_remove("PLAYIT_SECRET_PATH")
            .env_remove("API_BASE")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::process::hide_window(&mut bootstrap);
        let _bootstrap = crate::tunnel_process::OwnedChild::new(bootstrap.spawn()?)?;
        std::thread::sleep(Duration::from_secs(8));
        call(
            "/tunnels/create",
            Some(secret),
            json!({"tunnel_type":"minecraft-java","port_type":"tcp","port_count":1,
            "origin":{"type":"agent","data":{"agent_id":data["agent_id"],"local_ip":target.ip(),"local_port":target.port()}},
            "enabled":true,"alloc":null,"name":name,"firewall_id":null,"proxy_protocol":null}),
        )?;
        for _ in 0..12 {
            data = call("/v1/agents/rundata", Some(secret), json!({}))?;
            if data["tunnels"].as_array().is_some_and(|t| !t.is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let tunnels = data["tunnels"]
        .as_array()
        .context("Resposta playit sem túneis")?;
    if tunnels.len() != 1 || tunnels[0]["name"] != name {
        bail!("Aguarda a atribuição do túnel e verifica novamente. Este agente tem de ficar dedicado a este servidor.");
    }
    let tunnel = &tunnels[0];
    if !matches_fields(&tunnel["agent_config"], target) {
        // Preserve the allocation/address when the LAN IP or server port changes.
        call(
            "/tunnels/update",
            Some(secret),
            json!({"tunnel_id":tunnel["id"],"agent_id":data["agent_id"],"local_ip":target.ip(),"local_port":target.port(),"enabled":true}),
        )?;
        data = call("/v1/agents/rundata", Some(secret), json!({}))?;
    }
    account.tunnel = Some(checked_tunnel(&data["tunnels"][0], target)?);
    account.error = None;
    save(root, id, &account)?;
    status(root, id)
}
pub fn credentials(root: &Path, id: &str, target: SocketAddr) -> Result<(String, Tunnel)> {
    verify(&fs::read(binary(root)).context("Prepara primeiro o playit na aplicação")?)?;
    let account = load(root, id)?;
    let tunnel = account
        .tunnel
        .context("Associa o playit e verifica a configuração antes de partilhar")?;
    if tunnel.target != target {
        bail!("O IP ou porta mudou. Verifica novamente a configuração playit para preservar o endereço.");
    }
    Ok((
        account.secret.context("Falta associar a conta playit")?,
        tunnel,
    ))
}
pub fn validate(root: &Path, id: &str, target: SocketAddr) -> Result<()> {
    let (secret, saved) = credentials(root, id, target)?;
    let data = call("/v1/agents/rundata", Some(&secret), json!({}))?;
    let tunnels = data["tunnels"]
        .as_array()
        .context("Resposta playit sem túneis")?;
    if tunnels.len() != 1
        || tunnels[0]["id"] != saved.id
        || data["pending"].as_array().is_none_or(|p| !p.is_empty())
    {
        bail!(
            "Este agente tem de ter apenas o túnel deste servidor. Verifica a configuração playit."
        );
    }
    let live = checked_tunnel(&tunnels[0], target)?;
    if live.address != saved.address {
        bail!("O endereço playit mudou. Verifica a configuração antes de partilhar.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_wrong_download_and_never_echoes_api_error_secrets() {
        assert!(verify(b"tampered executable").is_err());
        let error =
            decode(json!({"status":"error","data":{"secret_key":"private-key"}})).unwrap_err();
        assert!(!error.to_string().contains("private-key"));
        assert!(path(Path::new("."), "../server").is_err());
    }
    #[test]
    fn requires_single_minecraft_tcp_destination() {
        let target: SocketAddr = "127.0.0.1:25565".parse().unwrap();
        let mut v = json!({"id":"id","display_address":"test.joinmc.link","port_type":"tcp","port_count":1,
            "tunnel_type":"minecraft-java","disabled_reason":null,"agent_config":fields(target)});
        assert!(checked_tunnel(&v, target).is_ok());
        v["agent_config"]["fields"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(checked_tunnel(&v, target).is_ok());
        v["port_count"] = json!(2);
        assert!(checked_tunnel(&v, target).is_err());
        v["port_count"] = json!(1);
        v["agent_config"] = fields("127.0.0.1:22".parse().unwrap());
        assert!(checked_tunnel(&v, target).is_err());
    }
    #[test]
    #[cfg(windows)]
    fn protected_association_survives_restart_without_returning_secrets() {
        let root = std::env::temp_dir().join(format!(
            "nethergate-playit-{}-{}",
            std::process::id(),
            now()
        ));
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let account = Account {
            secret: Some(secret.into()),
            tunnel: Some(Tunnel {
                id: "persistent-id".into(),
                address: "saved.joinmc.link".into(),
                target: "127.0.0.1:25565".parse().unwrap(),
            }),
            ..Account::default()
        };
        save(&root, "test", &account).unwrap();
        let bytes = fs::read(path(&root, "test").unwrap()).unwrap();
        assert!(!bytes.windows(secret.len()).any(|b| b == secret.as_bytes()));
        let view = status(&root, "test").unwrap();
        assert_eq!(view["address"], "saved.joinmc.link");
        assert_eq!(view["ready"], true);
        assert!(!view.to_string().contains(secret));
        assert_eq!(
            load(&root, "test").unwrap().tunnel.unwrap().id,
            "persistent-id"
        );
    }
}
