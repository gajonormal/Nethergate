use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    net::Ipv4Addr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Session {
    last_attempt: Option<Instant>,
    ip: Option<(Ipv4Addr, Instant, u128)>,
    test: Option<(Ipv4Addr, u16, Instant, Value)>,
}
#[derive(Default)]
pub struct Internet {
    sessions: HashMap<String, Session>,
    shared_tests: HashMap<String, (String, Instant, Option<Value>)>,
    network_revision: u64,
    publications: HashMap<String, String>,
}

fn now() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

pub fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && a != 0
        && a < 224
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 192 && b == 0 && (c == 0 || c == 2))
        && !(a == 192 && b == 88 && c == 99)
        && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        && !(a == 203 && b == 0 && c == 113)
}

fn ip_response(value: &Value) -> Result<Ipv4Addr> {
    let ip: Ipv4Addr = value
        .get("ip")
        .and_then(Value::as_str)
        .context("O serviço não devolveu um IPv4")?
        .parse()?;
    if !public_ipv4(ip) {
        bail!("O serviço não devolveu um IPv4 público utilizável");
    }
    Ok(ip)
}

fn fetch(url: &str) -> Result<Value> {
    let client = Client::builder()
        .no_proxy()
        .user_agent(concat!(
            "Nethergate/",
            env!("CARGO_PKG_VERSION"),
            " desktop connectivity diagnostic"
        ))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    let mut bytes = Vec::new();
    response.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        bail!("Resposta de diagnóstico demasiado grande");
    }
    serde_json::from_slice(&bytes).context("O serviço devolveu uma resposta inválida")
}

fn status_response(
    value: &Value,
    expected: Ipv4Addr,
    port: u16,
    checked_at: u128,
) -> Result<Value> {
    let online = value
        .get("online")
        .and_then(Value::as_bool)
        .context("Resposta sem estado online válido")?;
    let reported_ip = value.get("ip").and_then(Value::as_str).unwrap_or("");
    let reported_port = value.get("port").and_then(Value::as_u64);
    if (!reported_ip.is_empty() && reported_ip != expected.to_string())
        || reported_port.is_some_and(|p| p != port as u64)
        || (online && (reported_ip.is_empty() || reported_port.is_none()))
    {
        bail!("O serviço respondeu para um destino diferente. O resultado não foi aceite.");
    }
    Ok(status_view(
        value,
        &format!("{expected}:{port}"),
        online,
        checked_at,
    ))
}

fn status_view(value: &Value, address: &str, online: bool, checked_at: u128) -> Value {
    let protocol = value
        .pointer("/debug/ping")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .pointer("/debug/query")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let state = if online && protocol {
        "reported_online"
    } else if online {
        "unconfirmed"
    } else {
        "reported_offline"
    };
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .map(|v| v.chars().take(200).collect::<String>());
    json!({"state":state,"address":address,"checkedAt":checked_at,
        "source":"mcsrvstat.us","version":version,
        "cacheHit":value.pointer("/debug/cachehit").and_then(Value::as_bool),
        "observedAt":value.pointer("/debug/cachetime").and_then(Value::as_u64),
        "cacheExpiresAt":value.pointer("/debug/cacheexpire").and_then(Value::as_u64),
        "cacheWindowSeconds":300,"loginConfirmed":false})
}

// Only the hub supplies this address, from an active share; never accept a URL.
fn shared_endpoint(address: &str) -> Result<(&str, Option<u16>)> {
    let (host, port) = match address.split_once(':') {
        Some((host, port)) => (
            host,
            Some(port.parse::<u16>().context("Porta pública inválida")?),
        ),
        None => (address, None),
    };
    if port == Some(0)
        || host.len() > 253
        || !host.contains('.')
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
        || host.parse::<Ipv4Addr>().is_ok_and(|ip| !public_ipv4(ip))
    {
        bail!("Endereço público inválido");
    }
    Ok((host, port))
}

fn shared_status_response(value: &Value, address: &str, checked_at: u128) -> Result<Value> {
    let (host, port) = shared_endpoint(address)?;
    let online = value["online"]
        .as_bool()
        .context("Resposta sem estado online válido")?;
    if let Some(hostname) = value["hostname"].as_str().filter(|h| !h.is_empty()) {
        if !hostname.eq_ignore_ascii_case(host) {
            bail!("O serviço respondeu para outro nome público");
        }
    }
    if online {
        let ip = ip_response(value)?;
        let reported_port = value["port"]
            .as_u64()
            .filter(|p| (1..=65535).contains(p))
            .context("Resposta sem porta pública válida")?;
        if port.is_some_and(|p| u64::from(p) != reported_port)
            || host
                .parse::<Ipv4Addr>()
                .is_ok_and(|expected| expected != ip)
        {
            bail!("O serviço respondeu para outro destino público");
        }
    }
    Ok(status_view(value, address, online, checked_at))
}

pub fn wan_hint(public: Ipv4Addr, wan: &str) -> Result<&'static str> {
    if wan.trim().is_empty() {
        return Ok("not_provided");
    }
    let wan: Ipv4Addr = wan
        .trim()
        .parse()
        .context("Indica apenas o IPv4 WAN mostrado no router")?;
    if wan == public {
        Ok("same")
    } else if wan.is_private() || (wan.octets()[0] == 100 && (64..=127).contains(&wan.octets()[1]))
    {
        Ok("upstream_nat")
    } else {
        Ok("different")
    }
}

impl Internet {
    /// A WAN-only change can leave local interfaces unchanged. Retire the old
    /// DNS test while keeping its rate-limit timestamp.
    pub fn observe_publication(&mut self, id: &str, ip: &str) {
        if self
            .publications
            .get(id)
            .is_some_and(|previous| previous == ip)
        {
            return;
        }
        self.publications.insert(id.into(), ip.into());
        if let Some((_, _, result)) = self.shared_tests.get_mut(id) {
            *result = None;
        }
    }
    pub fn observe_network(&mut self, revision: u64) {
        if revision == self.network_revision {
            return;
        }
        self.network_revision = revision;
        for session in self.sessions.values_mut() {
            session.ip = None;
            session.test = None;
        }
        // Keep attempt times so switching networks cannot bypass request limits.
        for (_, _, result) in self.shared_tests.values_mut() {
            *result = None;
        }
    }
    pub fn test_shared(&mut self, id: &str, address: &str, consent: bool) -> Result<Value> {
        if !consent {
            bail!("Confirma o envio do endereço público ao serviço mcsrvstat.us");
        }
        shared_endpoint(address)?;
        if let Some((previous, at, result)) = self.shared_tests.get(id) {
            if previous == address && at.elapsed() < Duration::from_secs(300) {
                if let Some(result) = result {
                    return Ok(result.clone());
                }
            }
            if at.elapsed() < Duration::from_secs(30) {
                bail!("Aguarda 30 segundos antes de repetir o teste externo");
            }
        }
        self.shared_tests
            .insert(id.into(), (address.into(), Instant::now(), None));
        let result = shared_status_response(
            &fetch(&format!("https://api.mcsrvstat.us/3/{address}"))?,
            address,
            now()?,
        )?;
        self.shared_tests.insert(
            id.into(),
            (address.into(), Instant::now(), Some(result.clone())),
        );
        Ok(result)
    }

    pub fn lookup(&mut self, id: &str, consent: bool) -> Result<Value> {
        if !consent {
            bail!("Confirma a consulta a api.ipify.org antes de continuar");
        }
        let session = self.sessions.entry(id.to_string()).or_default();
        if let Some((ip, at, checked)) = session.ip {
            if at.elapsed() < Duration::from_secs(30) {
                return Ok(json!({"ip":ip,"checkedAt":checked,"source":"api.ipify.org"}));
            }
        }
        let ip = ip_response(&fetch("https://api.ipify.org?format=json")?)?;
        let checked = now()?;
        session.ip = Some((ip, Instant::now(), checked));
        Ok(json!({"ip":ip,"checkedAt":checked,"source":"api.ipify.org"}))
    }
    pub fn test(&mut self, id: &str, expected: &str, port: u16, consent: bool) -> Result<Value> {
        if !consent {
            bail!("Confirma o envio do IP e porta ao serviço mcsrvstat.us");
        }
        if port == 0 {
            bail!("A porta externa deve estar entre 1 e 65535");
        }
        let session = self
            .sessions
            .get_mut(id)
            .context("Consulta primeiro o IP público")?;
        let (ip, at, _) = session.ip.context("Consulta primeiro o IP público")?;
        if ip.to_string() != expected || at.elapsed() > Duration::from_secs(600) {
            bail!("O IP apresentado está desatualizado. Consulta novamente antes do teste.");
        }
        if let Some((tested_ip, tested_port, at, result)) = &session.test {
            if *tested_ip == ip && *tested_port == port && at.elapsed() < Duration::from_secs(300) {
                return Ok(result.clone());
            }
        }
        if session
            .last_attempt
            .is_some_and(|at| at.elapsed() < Duration::from_secs(15))
        {
            bail!("Aguarda 15 segundos antes de repetir um teste externo");
        }
        session.last_attempt = Some(Instant::now());
        let result = status_response(
            &fetch(&format!("https://api.mcsrvstat.us/3/{ip}:{port}"))?,
            ip,
            port,
            now()?,
        )?;
        session.test = Some((ip, port, Instant::now(), result.clone()));
        Ok(result)
    }
    pub fn compare_wan(&self, id: &str, wan: &str) -> Result<Value> {
        let (ip, at, _) = self
            .sessions
            .get(id)
            .and_then(|s| s.ip)
            .context("Consulta primeiro o IP público")?;
        if at.elapsed() > Duration::from_secs(600) {
            bail!("Consulta novamente o IP público antes de comparar");
        }
        Ok(json!({"state":wan_hint(ip, wan)?}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wan_publication_changes_retire_dns_cache_without_bypassing_throttling() {
        let mut internet = Internet::default();
        internet.observe_publication("fixture", "1.1.1.1");
        internet.shared_tests.insert(
            "fixture".into(),
            (
                "world.duckdns.org".into(),
                Instant::now(),
                Some(json!({"state":"reported_online"})),
            ),
        );
        internet.observe_publication("fixture", "1.1.1.1");
        assert!(internet.shared_tests["fixture"].2.is_some());
        internet.observe_publication("fixture", "8.8.8.8");
        assert!(internet.shared_tests["fixture"].2.is_none());
        assert!(internet
            .test_shared("fixture", "world.duckdns.org", true)
            .unwrap_err()
            .to_string()
            .contains("30 segundos"));
    }
    #[test]
    fn network_changes_invalidate_results_without_resetting_request_limits() {
        let mut internet = Internet::default();
        let session = Session {
            ip: Some(("1.1.1.1".parse().unwrap(), Instant::now(), 123)),
            test: Some((
                "1.1.1.1".parse().unwrap(),
                25565,
                Instant::now(),
                json!({"state":"reported_online"}),
            )),
            ..Session::default()
        };
        internet.sessions.insert("fixture".into(), session);
        internet.shared_tests.insert(
            "fixture".into(),
            (
                "world.example.org".into(),
                Instant::now(),
                Some(json!({"state":"reported_online"})),
            ),
        );
        internet.observe_network(1);
        assert!(internet.sessions["fixture"].ip.is_none());
        assert!(internet.sessions["fixture"].test.is_none());
        assert!(internet.shared_tests["fixture"].2.is_none());
        assert!(internet
            .test_shared("fixture", "world.example.org", true)
            .unwrap_err()
            .to_string()
            .contains("30 segundos"));
    }
    #[test]
    fn shared_checks_accept_dns_and_srv_but_reject_urls_and_private_targets() {
        assert_eq!(
            shared_endpoint("world.example.org").unwrap(),
            ("world.example.org", None)
        );
        assert_eq!(
            shared_endpoint("world.example.org:25570").unwrap().1,
            Some(25570)
        );
        for address in [
            "https://world.example.org",
            "world.example.org/path",
            "world.example.org?x=1",
            "user@world.example.org",
            "localhost",
            "127.0.0.1:25565",
            "192.168.1.2",
            "100.64.0.1",
            "world.example.org:0",
            "world.example.org:65536",
            "world..example.org",
            "-world.example.org",
            "world.example.org:1:2",
        ] {
            assert!(shared_endpoint(address).is_err(), "accepted {address}");
        }
        let payload = json!({"online":true,"hostname":"world.example.org","ip":"1.1.1.1","port":30123,
            "debug":{"ping":true,"srv":true,"cachehit":true,"cachetime":123}});
        let result = shared_status_response(&payload, "world.example.org", 456).unwrap();
        assert_eq!(result["state"], "reported_online");
        assert_eq!(result["address"], "world.example.org");
        assert_eq!(result["loginConfirmed"], false);
        assert_eq!(result["observedAt"], 123);
        assert!(shared_status_response(&payload, "other.example.org", 456).is_err());
        assert!(shared_status_response(&payload, "world.example.org:25565", 456).is_err());
        let mut wrong = payload.clone();
        wrong["ip"] = json!("127.0.0.1");
        assert!(shared_status_response(&wrong, "world.example.org", 456).is_err());
        let mut inconclusive = payload.clone();
        inconclusive["debug"] = json!({});
        assert_eq!(
            shared_status_response(&inconclusive, "world.example.org", 456).unwrap()["state"],
            "unconfirmed"
        );
        assert_eq!(
            shared_status_response(&json!({"online":false}), "world.example.org", 456).unwrap()
                ["state"],
            "reported_offline"
        );
    }

    #[test]
    fn shared_checks_require_consent_cache_by_address_and_throttle_failures() {
        let mut internet = Internet::default();
        assert!(internet
            .test_shared("fixture", "world.example.org", false)
            .is_err());
        assert!(internet.shared_tests.is_empty());
        let cached = json!({"state":"reported_online","checkedAt":123});
        internet.shared_tests.insert(
            "fixture".into(),
            (
                "world.example.org".into(),
                Instant::now(),
                Some(cached.clone()),
            ),
        );
        assert_eq!(
            internet
                .test_shared("fixture", "world.example.org", true)
                .unwrap(),
            cached
        );
        assert!(internet
            .test_shared("fixture", "other.example.org", true)
            .unwrap_err()
            .to_string()
            .contains("30 segundos"));
        internet.shared_tests.insert(
            "fixture".into(),
            ("world.example.org".into(), Instant::now(), None),
        );
        assert!(internet
            .test_shared("fixture", "world.example.org", true)
            .unwrap_err()
            .to_string()
            .contains("30 segundos"));
    }
    #[test]
    fn refuses_private_reserved_and_shared_addresses() {
        for ip in [
            "127.0.0.1",
            "0.0.0.0",
            "192.168.1.4",
            "100.64.0.1",
            "172.16.2.1",
            "203.0.113.1",
            "198.51.100.1",
            "198.18.0.1",
            "224.0.0.1",
        ] {
            assert!(ip_response(&json!({"ip":ip})).is_err());
        }
        assert!(public_ipv4("1.1.1.1".parse().unwrap()));
        assert!(ip_response(&json!({"ip":"::1"})).is_err());
    }
    #[test]
    fn does_not_confuse_cached_status_with_login_or_another_endpoint() {
        let ip = "1.1.1.1".parse().unwrap();
        let mut payload = json!({"online":true,"ip":"1.1.1.1","port":25565,"debug":{"ping":true,"cachehit":true,"cachetime":123},"version":"Paper"});
        let result = status_response(&payload, ip, 25565, 456).unwrap();
        assert_eq!(result["state"], "reported_online");
        assert_eq!(result["loginConfirmed"], false);
        assert_eq!(result["observedAt"], 123);
        payload["ip"] = json!("8.8.8.8");
        assert!(status_response(&payload, ip, 25565, 456).is_err());
        payload = json!({"online":true,"ip":"1.1.1.1","port":25565});
        assert_eq!(
            status_response(&payload, ip, 25565, 456).unwrap()["state"],
            "unconfirmed"
        );
        assert_eq!(
            status_response(
                &json!({"online":false,"ip":"","port":25565}),
                ip,
                25565,
                456
            )
            .unwrap()["state"],
            "reported_offline"
        );
    }
    #[test]
    fn requires_consent_current_ip_and_preserves_nat_uncertainty() {
        let mut session = Internet::default();
        assert!(session.lookup("test", false).is_err());
        assert!(session.test("test", "1.1.1.1", 25565, true).is_err());
        let ip = "1.1.1.1".parse().unwrap();
        session.sessions.insert(
            "test".into(),
            Session {
                last_attempt: None,
                ip: Some((ip, Instant::now(), 1)),
                test: None,
            },
        );
        assert!(session.test("test", "8.8.8.8", 25565, true).is_err());
        assert!(session.test("test", "1.1.1.1", 25565, false).is_err());
        assert_eq!(wan_hint(ip, "100.64.2.1").unwrap(), "upstream_nat");
        assert_eq!(wan_hint(ip, "192.168.0.1").unwrap(), "upstream_nat");
        assert_eq!(wan_hint(ip, "1.1.1.1").unwrap(), "same");
        assert_eq!(wan_hint(ip, "8.8.8.8").unwrap(), "different");
        assert!(wan_hint(ip, "router.local").is_err());
    }
}
