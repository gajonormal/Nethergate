//! Explicit, finite TCP mappings. Discovery never grants authority to another gateway.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    path::PathBuf,
    time::{Duration, Instant},
};
const LIFETIME: u32 = 600;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Upnp,
    NatPmp,
}
#[derive(Clone, Debug)]
pub struct ContextData {
    pub local: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub port: u16,
    pub revision: u64,
    pub signature: String,
    pub online: bool,
    pub secure: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gateway {
    provider: Provider,
    local: Ipv4Addr,
    address: Ipv4Addr,
    public_ip: Ipv4Addr,
    location: Option<String>,
    control_url: Option<String>,
    service: Option<String>,
    udn: Option<String>,
    epoch: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct Mapping {
    pub external: u16,
    pub internal: u16,
    pub client: Ipv4Addr,
    pub description: String,
    pub seconds: u32,
    pub epoch: Option<u32>,
}
trait Driver: Send {
    fn discover(&self, context: &ContextData, provider: Provider) -> Result<Gateway>;
    fn refresh(&self, gateway: &Gateway) -> Result<Gateway>;
    fn occupied(&self, gateway: &Gateway, external: u16) -> Result<Option<Mapping>>;
    fn map(
        &self,
        gateway: &Gateway,
        port: u16,
        external: u16,
        description: &str,
    ) -> Result<Mapping>;
    fn remove(&self, gateway: &Gateway, port: u16, external: u16, description: &str) -> Result<()>;
}
#[derive(Debug)]
struct Fault(u32);
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "O router recusou a operação UPnP (código {})", self.0)
    }
}
impl std::error::Error for Fault {}
fn private(ip: Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local()
}
fn router_url(value: &str, address: Ipv4Addr) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        ["http", "https"].contains(&url.scheme())
            && url.host_str().and_then(|h| h.parse::<Ipv4Addr>().ok()) == Some(address)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.query().is_none()
            && value.len() <= 2048,
        "O router anunciou um URL fora do gateway autorizado"
    );
    Ok(url)
}
fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()?)
}
fn xml_bytes(response: reqwest::blocking::Response) -> Result<String> {
    let mut bytes = vec![];
    response.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 256 * 1024, "Resposta UPnP demasiado grande");
    Ok(String::from_utf8(bytes)?)
}
fn device(location: &str, local: Ipv4Addr, address: Ipv4Addr) -> Result<Gateway> {
    let location = router_url(location, address)?;
    let response = http()?.get(location.clone()).send()?.error_for_status()?;
    let xml = xml_bytes(response)?;
    let doc = roxmltree::Document::parse(&xml)?;
    let udn = doc
        .descendants()
        .find(|n| n.has_tag_name("UDN"))
        .and_then(|n| n.text())
        .context("O router não publicou uma identidade UDN")?;
    ensure!(
        !udn.is_empty() && udn.len() <= 256 && !udn.chars().any(char::is_control),
        "UDN inválido"
    );
    let base = if let Some(base) = doc
        .descendants()
        .find(|n| n.has_tag_name("URLBase"))
        .and_then(|n| n.text())
    {
        router_url(base.trim(), address)?
    } else {
        location.clone()
    };
    for service in doc.descendants().filter(|n| n.has_tag_name("service")) {
        let text = |tag: &str| {
            service
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == tag)
                .and_then(|n| n.text())
        };
        let kind = text("serviceType").unwrap_or("");
        if ![
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "urn:schemas-upnp-org:service:WANIPConnection:2",
            "urn:schemas-upnp-org:service:WANPPPConnection:1",
        ]
        .contains(&kind)
        {
            continue;
        }
        let control = base.join(text("controlURL").context("Falta o endpoint UPnP")?.trim())?;
        router_url(control.as_str(), address)?;
        return Ok(Gateway {
            provider: Provider::Upnp,
            local,
            address,
            public_ip: Ipv4Addr::UNSPECIFIED,
            location: Some(location.to_string()),
            control_url: Some(control.to_string()),
            service: Some(kind.into()),
            udn: Some(udn.into()),
            epoch: None,
        });
    }
    anyhow::bail!("O gateway não disponibiliza WANIPConnection/WANPPPConnection")
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn soap(
    gateway: &Gateway,
    action: &str,
    args: &[(&str, String)],
) -> Result<BTreeMap<String, String>> {
    let url = gateway
        .control_url
        .as_deref()
        .context("Endpoint UPnP inválido")?;
    router_url(url, gateway.address)?;
    let service = gateway
        .service
        .as_deref()
        .context("Serviço UPnP inválido")?;
    ensure!(
        [
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "urn:schemas-upnp-org:service:WANIPConnection:2",
            "urn:schemas-upnp-org:service:WANPPPConnection:1"
        ]
        .contains(&service),
        "Serviço UPnP não autorizado"
    );
    let fields = args
        .iter()
        .map(|(k, v)| format!("<{k}>{}</{k}>", escape(v)))
        .collect::<String>();
    let body=format!("<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service}\">{fields}</u:{action}></s:Body></s:Envelope>");
    let response = http()?
        .post(url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{service}#{action}\""))
        .body(body)
        .send()?;
    let success = response.status().is_success();
    let xml = xml_bytes(response)?;
    let doc = roxmltree::Document::parse(&xml)?;
    if let Some(code) = doc
        .descendants()
        .find(|n| n.has_tag_name("errorCode"))
        .and_then(|n| n.text())
        .and_then(|s| s.parse().ok())
    {
        return Err(Fault(code).into());
    }
    ensure!(success, "O router recusou o pedido HTTP UPnP");
    let mut fields = BTreeMap::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        if let Some(text) = node.text() {
            fields.insert(node.tag_name().name().into(), text.trim().to_string());
        }
    }
    Ok(fields)
}
fn specific(gateway: &Gateway, external: u16) -> Result<Option<Mapping>> {
    let args = [
        ("NewRemoteHost", String::new()),
        ("NewExternalPort", external.to_string()),
        ("NewProtocol", "TCP".into()),
    ];
    let response = match soap(gateway, "GetSpecificPortMappingEntry", &args) {
        Ok(v) => v,
        Err(e) if e.downcast_ref::<Fault>().is_some_and(|f| f.0 == 714) => return Ok(None),
        Err(e) => return Err(e),
    };
    ensure!(
        matches!(
            response.get("NewEnabled").map(String::as_str),
            Some("1" | "true")
        ),
        "Existe uma regra desativada nessa porta; não será substituída"
    );
    Ok(Some(Mapping {
        external,
        internal: response
            .get("NewInternalPort")
            .context("Resposta UPnP sem porta interna")?
            .parse()?,
        client: response
            .get("NewInternalClient")
            .context("Resposta UPnP sem IP interno")?
            .parse()?,
        description: response
            .get("NewPortMappingDescription")
            .cloned()
            .unwrap_or_default(),
        seconds: response
            .get("NewLeaseDuration")
            .context("O router não confirmou a duração do lease")?
            .parse()?,
        epoch: None,
    }))
}
fn nat_exchange(local: Ipv4Addr, address: SocketAddrV4, request: &[u8]) -> Result<Vec<u8>> {
    let socket = UdpSocket::bind((local, 0))?;
    socket.connect(address)?;
    let mut buffer = [0u8; 64];
    for millis in [250, 500, 1000, 2000] {
        socket.send(request)?;
        socket.set_read_timeout(Some(Duration::from_millis(millis)))?;
        match socket.recv(&mut buffer) {
            Ok(size) => {
                let bytes = &buffer[..size];
                ensure!(
                    bytes.len() >= 8 && bytes[0] == 0 && bytes[1] == request[1] + 128,
                    "Resposta NAT-PMP inesperada"
                );
                let code = u16::from_be_bytes([bytes[2], bytes[3]]);
                ensure!(code == 0, "O router recusou NAT-PMP (código {code})");
                return Ok(bytes.to_vec());
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("O gateway não respondeu a NAT-PMP")
}
fn nat_public(local: Ipv4Addr, address: Ipv4Addr) -> Result<(Ipv4Addr, u32)> {
    let bytes = nat_exchange(local, SocketAddrV4::new(address, 5351), &[0, 0])?;
    ensure!(bytes.len() == 12, "Resposta NAT-PMP pública inválida");
    Ok((
        Ipv4Addr::new(bytes[8], bytes[9], bytes[10], bytes[11]),
        u32::from_be_bytes(bytes[4..8].try_into()?),
    ))
}
fn nat_packet(port: u16, external: u16, seconds: u32) -> [u8; 12] {
    let mut packet = [0u8; 12];
    packet[1] = 2;
    packet[4..6].copy_from_slice(&port.to_be_bytes());
    packet[6..8].copy_from_slice(&external.to_be_bytes());
    packet[8..12].copy_from_slice(&seconds.to_be_bytes());
    packet
}
fn nat_mapping(bytes: &[u8], local: Ipv4Addr, port: u16, description: &str) -> Result<Mapping> {
    ensure!(
        bytes.len() == 16
            && bytes[0] == 0
            && bytes[1] == 130
            && u16::from_be_bytes(bytes[2..4].try_into()?) == 0
            && u16::from_be_bytes(bytes[8..10].try_into()?) == port,
        "Resposta NAT-PMP de outra porta/protocolo"
    );
    Ok(Mapping {
        external: u16::from_be_bytes(bytes[10..12].try_into()?),
        internal: port,
        client: local,
        description: description.into(),
        seconds: u32::from_be_bytes(bytes[12..16].try_into()?),
        epoch: Some(u32::from_be_bytes(bytes[4..8].try_into()?)),
    })
}
struct Network;
impl Driver for Network {
    fn discover(&self, context: &ContextData, provider: Provider) -> Result<Gateway> {
        if provider == Provider::NatPmp {
            let (public_ip, epoch) = nat_public(context.local, context.gateway)?;
            return Ok(Gateway {
                provider,
                local: context.local,
                address: context.gateway,
                public_ip,
                location: None,
                control_url: None,
                service: None,
                udn: None,
                epoch: Some(epoch),
            });
        }
        let socket = UdpSocket::bind((context.local, 0))?;
        socket.set_multicast_ttl_v4(1)?;
        socket.set_read_timeout(Some(Duration::from_millis(400)))?;
        let request=b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: ssdp:all\r\n\r\n";
        socket.send_to(request, (Ipv4Addr::new(239, 255, 255, 250), 1900))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut buffer = [0u8; 8192];
        let mut seen = std::collections::HashSet::new();
        while Instant::now() < deadline && seen.len() < 16 {
            match socket.recv_from(&mut buffer) {
                Ok((size, sender)) if sender.ip() == std::net::IpAddr::V4(context.gateway) => {
                    let Ok(text) = std::str::from_utf8(&buffer[..size]) else {
                        continue;
                    };
                    if !text.starts_with("HTTP/1.1 200") && !text.starts_with("HTTP/1.0 200") {
                        continue;
                    }
                    let location = text
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(key, _)| key.eq_ignore_ascii_case("location"))
                        .map(|(_, v)| v.trim());
                    if let Some(location) = location {
                        if seen.insert(location.to_string())
                            && router_url(location, context.gateway).is_ok()
                        {
                            if let Ok(gateway) = device(location, context.local, context.gateway) {
                                return self.refresh(&gateway);
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        anyhow::bail!("UPnP indisponível no gateway desta interface. Usa NAT-PMP, configuração manual ou playit")
    }
    fn refresh(&self, gateway: &Gateway) -> Result<Gateway> {
        if gateway.provider == Provider::NatPmp {
            let (ip, epoch) = nat_public(gateway.local, gateway.address)?;
            let mut current = gateway.clone();
            current.public_ip = ip;
            current.epoch = Some(epoch);
            return Ok(current);
        }
        let mut current = device(
            gateway
                .location
                .as_deref()
                .context("Localização UPnP inválida")?,
            gateway.local,
            gateway.address,
        )?;
        ensure!(
            current.udn == gateway.udn,
            "A identidade do router mudou; não foram alteradas regras"
        );
        let reply = soap(&current, "GetExternalIPAddress", &[])?;
        current.public_ip = reply
            .get("NewExternalIPAddress")
            .context("O router não devolveu IPv4 WAN")?
            .parse()?;
        Ok(current)
    }
    fn occupied(&self, gateway: &Gateway, external: u16) -> Result<Option<Mapping>> {
        if gateway.provider == Provider::Upnp {
            specific(gateway, external)
        } else {
            Ok(None)
        }
    }
    fn map(
        &self,
        gateway: &Gateway,
        port: u16,
        external: u16,
        description: &str,
    ) -> Result<Mapping> {
        if gateway.provider == Provider::NatPmp {
            let bytes = nat_exchange(
                gateway.local,
                SocketAddrV4::new(gateway.address, 5351),
                &nat_packet(port, external, LIFETIME),
            )?;
            return nat_mapping(&bytes, gateway.local, port, description);
        }
        if let Some(previous) = specific(gateway, external)? {
            ensure!(
                previous.client == gateway.local
                    && previous.internal == port
                    && previous.description == description,
                "Essa porta pertence a outra regra; não será substituída"
            );
        }
        soap(
            gateway,
            "AddPortMapping",
            &[
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", external.to_string()),
                ("NewProtocol", "TCP".into()),
                ("NewInternalPort", port.to_string()),
                ("NewInternalClient", gateway.local.to_string()),
                ("NewEnabled", "1".into()),
                ("NewPortMappingDescription", description.into()),
                ("NewLeaseDuration", LIFETIME.to_string()),
            ],
        )?;
        let mapping =
            specific(gateway, external)?.context("O router não confirmou a regra criada")?;
        ensure!(
            mapping.client == gateway.local
                && mapping.internal == port
                && mapping.description == description,
            "O router confirmou um destino diferente; não será removida uma regra alheia"
        );
        Ok(mapping)
    }
    fn remove(&self, gateway: &Gateway, port: u16, external: u16, description: &str) -> Result<()> {
        if gateway.provider == Provider::NatPmp {
            let bytes = nat_exchange(
                gateway.local,
                SocketAddrV4::new(gateway.address, 5351),
                &nat_packet(port, external, 0),
            )?;
            let ack = nat_mapping(&bytes, gateway.local, port, description)?;
            ensure!(ack.seconds == 0, "O router não confirmou a remoção NAT-PMP");
            return Ok(());
        }
        let Some(mapping) = specific(gateway, external)? else {
            return Ok(());
        };
        ensure!(
            mapping.client == gateway.local
                && mapping.internal == port
                && mapping.description == description,
            "A regra atual não pertence a esta sessão Nethergate; não foi removida"
        );
        soap(
            gateway,
            "DeletePortMapping",
            &[
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", external.to_string()),
                ("NewProtocol", "TCP".into()),
            ],
        )?;
        ensure!(
            specific(gateway, external)?.is_none(),
            "O router não confirmou a remoção UPnP"
        );
        Ok(())
    }
}
#[derive(Clone)]
struct Plan {
    ticket: String,
    gateway: Gateway,
    context: ContextData,
    external: u16,
    expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lease {
    gateway: Gateway,
    port: u16,
    external: u16,
    description: String,
    signature: String,
    revision: u64,
    observed_at: u64,
    expires_at: u64,
    renew_at: u64,
    needs_cleanup: bool,
    #[serde(default)]
    finite_confirmed: bool,
    error: Option<String>,
    #[serde(default)]
    session: String,
    #[serde(skip)]
    verified: bool,
}
pub struct Manager {
    root: PathBuf,
    session: String,
    plans: HashMap<String, Plan>,
    leases: HashMap<String, Lease>,
    driver: Box<dyn Driver>,
}
impl Manager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            session: crate::transfer::key().unwrap_or_else(|_| {
                format!("{}-{:?}", std::process::id(), std::time::SystemTime::now())
            }),
            plans: HashMap::new(),
            leases: HashMap::new(),
            driver: Box::new(Network),
        }
    }
    fn path(&self, id: &str) -> Result<PathBuf> {
        crate::backups::component(id)?;
        Ok(self
            .root
            .join("hub-data/router-leases")
            .join(format!("{id}.json")))
    }
    fn load(&mut self, id: &str) -> Result<()> {
        if !self.leases.contains_key(id) {
            let path = self.path(id)?;
            if path.exists() {
                ensure!(
                    crate::backups::ordinary(&path)?.len() <= 16384,
                    "Registo do router inválido"
                );
                let lease: Lease = serde_json::from_slice(&fs::read(path)?)?;
                ensure!(
                    lease.port > 0
                        && lease.external > 0
                        && lease.description.starts_with("NetherGate-")
                        && lease.description.len() <= 96
                        && private(lease.gateway.local)
                        && private(lease.gateway.address),
                    "Registo de lease inválido"
                );
                self.leases.insert(id.into(), lease);
            }
        }
        Ok(())
    }
    fn persist(&self, id: &str, lease: &Lease) -> Result<()> {
        let path = self.path(id)?;
        fs::create_dir_all(
            path.parent()
                .context("O registo dos mapeamentos não tem pasta pai")?,
        )?;
        crate::hub::replace_json(&path, lease)
    }
    fn same_network(&self, lease: &Lease, context: &ContextData) -> bool {
        lease.gateway.local == context.local
            && lease.gateway.address == context.gateway
            && (lease.session != self.session || lease.revision == context.revision)
    }
    pub fn status(&mut self, id: &str, now: u64) -> Result<Value> {
        self.load(id)?;
        Ok(if let Some(lease) = self.leases.get(id) {
            json!({"state":if lease.needs_cleanup{"needs_cleanup"}else if now>=lease.expires_at{"expired"}else if lease.verified{"mapped"}else{"review_required"},"provider":lease.gateway.provider,"externalPort":lease.external,"internalPort":lease.port,"localAddress":lease.gateway.local,"gateway":lease.gateway.address,"wan":lease.gateway.public_ip,"address":format!("{}:{}",lease.gateway.public_ip,lease.external),"publicIp":crate::internet::public_ipv4(lease.gateway.public_ip),"expiresAt":lease.expires_at,"canForget":lease.finite_confirmed&&now>=lease.expires_at,"error":lease.error})
        } else {
            json!({"state":"idle"})
        })
    }
    pub fn guard_change(&mut self, id: &str, now: u64) -> Result<()> {
        self.load(id)?;
        ensure!(
            !self
                .leases
                .get(id)
                .is_some_and(|l| l.needs_cleanup || l.expires_at > now),
            "Remove primeiro o encaminhamento UPnP/NAT-PMP antes de alterar o acesso do servidor"
        );
        Ok(())
    }
    pub fn preview(
        &mut self,
        id: &str,
        context: ContextData,
        provider: Provider,
        external: u16,
        now: u64,
    ) -> Result<Value> {
        self.load(id)?;
        ensure!(context.secure&&private(context.local)&&private(context.gateway)&&context.port>0&&external>0,"Configura um IP LAN desta interface, autenticação e whitelist com jogadores antes de encaminhar a porta");
        ensure!(
            !self.leases.contains_key(id),
            "Remove primeiro o mapeamento anterior"
        );
        let gateway = self.driver.discover(&context, provider)?;
        ensure!(
            gateway.local == context.local && gateway.address == context.gateway,
            "Foi encontrado outro gateway"
        );
        ensure!(
            self.driver.occupied(&gateway, external)?.is_none(),
            "Essa porta externa já tem uma regra; escolhe outra"
        );
        let ticket = crate::transfer::key()?;
        let result = json!({"ticket":ticket,"provider":provider,"localAddress":context.local,"internalPort":context.port,"externalPort":external,"gateway":context.gateway,"wan":gateway.public_ip,"publicIp":crate::internet::public_ipv4(gateway.public_ip),"leaseSeconds":LIFETIME,"expiresAt":now+120_000});
        self.plans.insert(
            id.into(),
            Plan {
                ticket,
                gateway,
                context,
                external,
                expires: now + 120_000,
            },
        );
        Ok(result)
    }
    pub fn create(
        &mut self,
        id: &str,
        ticket: &str,
        context: &ContextData,
        now: u64,
    ) -> Result<Value> {
        let plan = self
            .plans
            .get(id)
            .cloned()
            .context("Revê o encaminhamento antes de criar")?;
        ensure!(plan.ticket==ticket&&plan.expires>now&&context.signature==plan.context.signature&&context.revision==plan.context.revision&&context.online&&context.secure&&context.local==plan.context.local&&context.gateway==plan.context.gateway&&context.port==plan.context.port,"O plano expirou, o servidor não está pronto ou a rede/configuração mudou; revê novamente");
        self.plans.remove(id);
        let gateway = self.driver.refresh(&plan.gateway)?;
        let description = format!("NetherGate-{}", &crate::transfer::key()?[..16]);
        let mut lease = Lease {
            gateway,
            port: context.port,
            external: plan.external,
            description,
            signature: context.signature.clone(),
            revision: context.revision,
            observed_at: now,
            expires_at: now + u64::from(LIFETIME) * 1000,
            renew_at: now + u64::from(LIFETIME) * 500,
            needs_cleanup: true,
            finite_confirmed: false,
            error: None,
            verified: false,
            session: self.session.clone(),
        };
        self.persist(id, &lease)?;
        self.leases.insert(id.into(), lease.clone());
        let result = self.driver.map(
            &lease.gateway,
            lease.port,
            lease.external,
            &lease.description,
        );
        match result {
            Ok(mapping)
                if mapping.client == context.local
                    && mapping.internal == context.port
                    && mapping.external > 0
                    && mapping.description == lease.description
                    && (1..=3600).contains(&mapping.seconds) =>
            {
                lease.external = mapping.external;
                lease.gateway.epoch = mapping.epoch.or(lease.gateway.epoch);
                lease.expires_at = now + u64::from(mapping.seconds) * 1000;
                lease.renew_at = now + u64::from(mapping.seconds) * 500;
                lease.needs_cleanup = false;
                lease.finite_confirmed = true;
                lease.verified = true;
                self.persist(id, &lease)?;
                self.leases.insert(id.into(), lease);
                self.status(id, now)
            }
            other => {
                lease.error = Some(match other {
                    Err(e) => format!("{e:#}"),
                    _ => "O router não confirmou um lease finito e o destino correto".into(),
                });
                self.persist(id, &lease)?;
                self.leases.insert(id.into(), lease);
                let cleanup = self.remove(id, Some(context), now);
                anyhow::bail!(
                    "O encaminhamento não foi confirmado. {}",
                    match cleanup {
                        Ok(_) =>
                            "A limpeza foi confirmada; revê o router antes de tentar novamente"
                                .to_string(),
                        Err(e) => format!("A limpeza também precisa de atenção: {e}"),
                    }
                )
            }
        }
    }
    pub fn remove(&mut self, id: &str, context: Option<&ContextData>, now: u64) -> Result<Value> {
        self.plans.remove(id);
        self.load(id)?;
        let Some(mut lease) = self.leases.get(id).cloned() else {
            return self.status(id, now);
        };
        let result = (|| -> Result<()> {
            let context = context.context(
                "Volta à rede original para confirmar a remoção. A renovação foi interrompida",
            )?;
            ensure!(
                self.same_network(&lease, context),
                "A rede mudou; não serão enviadas ordens de remoção para outro gateway"
            );
            let refreshed = self.driver.refresh(&lease.gateway)?;
            if let (Some(old), Some(new)) = (lease.gateway.epoch, refreshed.epoch) {
                ensure!(new>=old,"O gateway NAT-PMP reiniciou; não é possível provar que conserva o mesmo mapeamento");
            }
            self.driver
                .remove(&refreshed, lease.port, lease.external, &lease.description)
        })();
        if let Err(e) = result {
            lease.needs_cleanup = true;
            lease.verified = false;
            lease.error = Some(format!("{e:#}"));
            self.persist(id, &lease)?;
            self.leases.insert(id.into(), lease);
            return Err(e);
        }
        let path = self.path(id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        self.leases.remove(id);
        self.status(id, now)
    }
    pub fn tick(&mut self, id: &str, context: Option<&ContextData>, now: u64) -> Result<()> {
        self.load(id)?;
        let Some(mut lease) = self.leases.get(id).cloned() else {
            return Ok(());
        };
        if lease.needs_cleanup || !lease.verified {
            return Ok(());
        }
        if now >= lease.expires_at {
            lease.verified = false;
            lease.error =
                Some("O lease expirou. Revê o registo antes de criar outro mapeamento".into());
            self.persist(id, &lease)?;
            self.leases.insert(id.into(), lease);
            return Ok(());
        }
        let valid = context.is_some_and(|c| {
            c.online
                && c.secure
                && self.same_network(&lease, c)
                && c.signature == lease.signature
                && c.port == lease.port
        });
        if !valid {
            let _ = self.remove(id, context, now);
            return Ok(());
        }
        if now < lease.renew_at {
            return Ok(());
        }
        let mut mapping_attempted = false;
        let result = (|| -> Result<Mapping> {
            let gateway = self.driver.refresh(&lease.gateway)?;
            if let (Some(old), Some(epoch)) = (lease.gateway.epoch, gateway.epoch) {
                let elapsed = (now.saturating_sub(lease.observed_at) / 1000) as u32;
                ensure!(
                    epoch >= old && epoch <= old.saturating_add(elapsed).saturating_add(300),
                    "O gateway NAT-PMP mudou ou reiniciou; a renovação foi interrompida"
                );
            }
            mapping_attempted = true;
            let mapping =
                self.driver
                    .map(&gateway, lease.port, lease.external, &lease.description)?;
            ensure!(
                mapping.external == lease.external
                    && mapping.internal == lease.port
                    && mapping.client == lease.gateway.local
                    && mapping.description == lease.description
                    && (1..=3600).contains(&mapping.seconds),
                "O router mudou a porta/destino ou não confirmou um lease finito"
            );
            lease.gateway = gateway;
            Ok(mapping)
        })();
        match result {
            Ok(mapping) => {
                lease.gateway.epoch = mapping.epoch.or(lease.gateway.epoch);
                lease.observed_at = now;
                lease.expires_at = now + u64::from(mapping.seconds) * 1000;
                lease.renew_at = now + u64::from(mapping.seconds) * 500;
                self.persist(id, &lease)?;
                self.leases.insert(id.into(), lease);
            }
            Err(e) => {
                if mapping_attempted {
                    lease.finite_confirmed = false;
                }
                lease.error = Some(format!("Renovação interrompida: {e:#}"));
                lease.needs_cleanup = true;
                lease.verified = false;
                self.persist(id, &lease)?;
                self.leases.insert(id.into(), lease);
                return Err(e);
            }
        }
        Ok(())
    }
    pub fn forget_expired(&mut self, id: &str, now: u64) -> Result<Value> {
        self.load(id)?;
        let lease = self.leases.get(id).context("Não existe registo de lease")?;
        ensure!(
            lease.finite_confirmed && now >= lease.expires_at,
            "Ainda não é possível confirmar a expiração de um lease finito"
        );
        let path = self.path(id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        self.leases.remove(id);
        self.plans.remove(id);
        self.status(id, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Sim {
        mapping: Option<Mapping>,
        maps: usize,
        removes: usize,
        seconds: Option<u32>,
        changed_identity: bool,
    }
    struct Fake(Arc<Mutex<Sim>>);
    impl Driver for Fake {
        fn discover(&self, c: &ContextData, p: Provider) -> Result<Gateway> {
            Ok(Gateway {
                provider: p,
                local: c.local,
                address: c.gateway,
                public_ip: Ipv4Addr::new(100, 64, 1, 1),
                location: None,
                control_url: None,
                service: None,
                udn: Some("uuid:router".into()),
                epoch: (p == Provider::NatPmp).then_some(1000),
            })
        }
        fn refresh(&self, g: &Gateway) -> Result<Gateway> {
            ensure!(
                !self.0.lock().unwrap().changed_identity,
                "Gateway identity changed"
            );
            Ok(g.clone())
        }
        fn occupied(&self, _: &Gateway, _: u16) -> Result<Option<Mapping>> {
            Ok(self.0.lock().unwrap().mapping.clone())
        }
        fn map(&self, g: &Gateway, p: u16, e: u16, d: &str) -> Result<Mapping> {
            let mut s = self.0.lock().unwrap();
            if let Some(m) = &s.mapping {
                ensure!(
                    m.client == g.local && m.internal == p && m.description == d,
                    "foreign mapping"
                );
            }
            s.maps += 1;
            let m = Mapping {
                external: e,
                internal: p,
                client: g.local,
                description: d.into(),
                seconds: s.seconds.unwrap_or(600),
                epoch: g.epoch,
            };
            s.mapping = Some(m.clone());
            Ok(m)
        }
        fn remove(&self, g: &Gateway, p: u16, _: u16, d: &str) -> Result<()> {
            let mut s = self.0.lock().unwrap();
            if let Some(m) = &s.mapping {
                ensure!(
                    m.client == g.local && m.internal == p && m.description == d,
                    "foreign mapping"
                );
            }
            s.removes += 1;
            s.mapping = None;
            Ok(())
        }
    }
    fn context() -> ContextData {
        ContextData {
            local: Ipv4Addr::new(192, 168, 23, 2),
            gateway: Ipv4Addr::new(192, 168, 23, 1),
            port: 25565,
            revision: 7,
            signature: "configuration".into(),
            online: true,
            secure: true,
        }
    }
    fn manager() -> (Manager, Arc<Mutex<Sim>>) {
        let root =
            std::env::temp_dir().join(format!("ng-router-{}", crate::transfer::key().unwrap()));
        fs::create_dir(&root).unwrap();
        let mut m = Manager::new(root);
        let state = Arc::new(Mutex::new(Sim::default()));
        m.driver = Box::new(Fake(state.clone()));
        (m, state)
    }
    #[test]
    fn reviewed_mapping_renews_and_stops_without_claiming_cgnat_access() {
        let (mut m, state) = manager();
        let c = context();
        let plan = m
            .preview("server", c.clone(), Provider::Upnp, 25565, 1000)
            .unwrap();
        let status = m
            .create("server", plan["ticket"].as_str().unwrap(), &c, 1001)
            .unwrap();
        assert_eq!(status["state"], "mapped");
        assert_eq!(status["publicIp"], false);
        m.tick("server", Some(&c), 302000).unwrap();
        assert_eq!(state.lock().unwrap().maps, 2);
        let mut stopped = c;
        stopped.online = false;
        m.tick("server", Some(&stopped), 303000).unwrap();
        assert_eq!(state.lock().unwrap().removes, 1);
        assert_eq!(m.status("server", 303000).unwrap()["state"], "idle");
    }
    #[test]
    fn stale_tickets_and_foreign_rules_never_get_overwritten_or_removed() {
        let (mut m, state) = manager();
        let c = context();
        let plan = m
            .preview("server", c.clone(), Provider::Upnp, 25565, 1000)
            .unwrap();
        let mut changed = c.clone();
        changed.revision += 1;
        assert!(m
            .create("server", plan["ticket"].as_str().unwrap(), &changed, 1001)
            .is_err());
        assert_eq!(state.lock().unwrap().maps, 0);
        m.create("server", plan["ticket"].as_str().unwrap(), &c, 1001)
            .unwrap();
        state.lock().unwrap().mapping.as_mut().unwrap().description = "other program".into();
        assert!(m.remove("server", Some(&c), 2000).is_err());
        assert_eq!(state.lock().unwrap().removes, 0);
        assert_eq!(m.status("server", 2000).unwrap()["state"], "needs_cleanup");
    }
    #[test]
    fn expired_leases_are_not_recreated_and_only_expired_finite_records_can_be_forgotten() {
        let (mut m, state) = manager();
        let c = context();
        let plan = m
            .preview("server", c.clone(), Provider::Upnp, 25565, 1000)
            .unwrap();
        m.create("server", plan["ticket"].as_str().unwrap(), &c, 1001)
            .unwrap();
        assert!(m.forget_expired("server", 2000).is_err());
        m.tick("server", Some(&c), 602000).unwrap();
        assert_eq!(state.lock().unwrap().maps, 1);
        assert_eq!(m.status("server", 602000).unwrap()["state"], "expired");
        assert!(m
            .preview("server", c, Provider::Upnp, 25565, 602000)
            .is_err());
        m.forget_expired("server", 602000).unwrap();
        assert_eq!(m.status("server", 602000).unwrap()["state"], "idle");
        assert_eq!(state.lock().unwrap().removes, 0);
    }
    #[test]
    fn network_changes_do_not_send_removals_to_another_gateway() {
        let (mut m, state) = manager();
        let c = context();
        let plan = m
            .preview("server", c.clone(), Provider::NatPmp, 25565, 1000)
            .unwrap();
        m.create("server", plan["ticket"].as_str().unwrap(), &c, 1001)
            .unwrap();
        let mut changed = c;
        changed.gateway = Ipv4Addr::new(192, 168, 23, 254);
        changed.revision += 1;
        m.tick("server", Some(&changed), 2000).unwrap();
        assert_eq!(state.lock().unwrap().removes, 0);
        assert_eq!(m.status("server", 2000).unwrap()["state"], "needs_cleanup");
    }
    #[test]
    fn permanent_leases_are_rejected_and_cleaned_and_restart_requires_review() {
        let (mut m, state) = manager();
        let c = context();
        state.lock().unwrap().seconds = Some(0);
        let plan = m
            .preview("server", c.clone(), Provider::Upnp, 25565, 1000)
            .unwrap();
        assert!(m
            .create("server", plan["ticket"].as_str().unwrap(), &c, 1001)
            .is_err());
        assert_eq!(state.lock().unwrap().removes, 1);
        state.lock().unwrap().seconds = None;
        let plan = m
            .preview("server", c.clone(), Provider::Upnp, 25565, 2000)
            .unwrap();
        m.create("server", plan["ticket"].as_str().unwrap(), &c, 2001)
            .unwrap();
        let mut reopened = Manager::new(m.root.clone());
        reopened.driver = Box::new(Fake(state.clone()));
        assert_eq!(
            reopened.status("server", 2100).unwrap()["state"],
            "review_required"
        );
        let mut new_session = c;
        new_session.revision = 0;
        reopened.remove("server", Some(&new_session), 2200).unwrap();
        assert_eq!(state.lock().unwrap().removes, 2);
    }
    #[test]
    fn nat_pmp_udp_packets_and_mismatched_responses() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = match server.local_addr().unwrap() {
            std::net::SocketAddr::V4(v) => v,
            _ => unreachable!(),
        };
        let join = std::thread::spawn(move || {
            let mut b = [0u8; 64];
            let (size, peer) = server.recv_from(&mut b).unwrap();
            assert_eq!(size, 12);
            assert_eq!(b[1], 2);
            assert_eq!(u16::from_be_bytes(b[4..6].try_into().unwrap()), 25565);
            let mut reply = [0u8; 16];
            reply[1] = 130;
            reply[4..8].copy_from_slice(&1000u32.to_be_bytes());
            reply[8..10].copy_from_slice(&25565u16.to_be_bytes());
            reply[10..12].copy_from_slice(&40000u16.to_be_bytes());
            reply[12..16].copy_from_slice(&600u32.to_be_bytes());
            server.send_to(&reply, peer).unwrap();
        });
        let bytes =
            nat_exchange(Ipv4Addr::LOCALHOST, address, &nat_packet(25565, 25565, 600)).unwrap();
        let m = nat_mapping(&bytes, Ipv4Addr::LOCALHOST, 25565, "test").unwrap();
        assert_eq!(m.external, 40000);
        assert_eq!(m.seconds, 600);
        assert!(nat_mapping(&bytes, Ipv4Addr::LOCALHOST, 25566, "wrong").is_err());
        join.join().unwrap();
        assert!(router_url(
            "http://169.254.169.254/latest/meta-data",
            Ipv4Addr::new(192, 168, 1, 1)
        )
        .is_err());
        assert!(router_url(
            "http://user:pass@192.168.1.1/ctl",
            Ipv4Addr::new(192, 168, 1, 1)
        )
        .is_err());
    }
    #[test]
    fn upnp_soap_roundtrip_and_ownership_checks() {
        let server = tiny_http::Server::http((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let root_url = format!("http://127.0.0.1:{port}");
        let base = root_url.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = stop.clone();
        let state = Arc::new(Mutex::new(None::<Mapping>));
        let remote = state.clone();
        let join = std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                let Some(mut r) = server.recv_timeout(Duration::from_millis(50)).unwrap() else {
                    continue;
                };
                if r.method() == &tiny_http::Method::Get {
                    let xml=format!("<root xmlns=\"urn:schemas-upnp-org:device-1-0\"><URLBase>{base}/</URLBase><device><UDN>uuid:fixture</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>/control</controlURL></service></serviceList></device></root>");
                    r.respond(tiny_http::Response::from_string(xml)).unwrap();
                    continue;
                }
                let action = r
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("SOAPAction"))
                    .unwrap()
                    .value
                    .as_str()
                    .to_string();
                let mut body = String::new();
                r.as_reader().read_to_string(&mut body).unwrap();
                let doc = roxmltree::Document::parse(&body).unwrap();
                let field = |tag: &str| {
                    doc.descendants()
                        .find(|n| n.is_element() && n.tag_name().name() == tag)
                        .and_then(|n| n.text())
                        .unwrap_or("")
                        .to_string()
                };
                let mut data = remote.lock().unwrap();
                let mut status = 200;
                let payload = if action.contains("GetExternalIPAddress") {
                    "<NewExternalIPAddress>100.64.1.1</NewExternalIPAddress>".to_string()
                } else if action.contains("GetSpecificPortMappingEntry") {
                    if let Some(m) = &*data {
                        format!("<NewInternalClient>{}</NewInternalClient><NewInternalPort>{}</NewInternalPort><NewEnabled>1</NewEnabled><NewPortMappingDescription>{}</NewPortMappingDescription><NewLeaseDuration>{}</NewLeaseDuration>",m.client,m.internal,escape(&m.description),m.seconds)
                    } else {
                        status = 500;
                        "<errorCode>714</errorCode>".into()
                    }
                } else if action.contains("AddPortMapping") {
                    *data = Some(Mapping {
                        external: field("NewExternalPort").parse().unwrap(),
                        internal: field("NewInternalPort").parse().unwrap(),
                        client: field("NewInternalClient").parse().unwrap(),
                        description: field("NewPortMappingDescription"),
                        seconds: field("NewLeaseDuration").parse().unwrap(),
                        epoch: None,
                    });
                    String::new()
                } else if action.contains("DeletePortMapping") {
                    *data = None;
                    String::new()
                } else {
                    panic!("Unexpected SOAP action")
                };
                r.respond(
                    tiny_http::Response::from_string(format!(
                        "<Envelope><Body>{payload}</Body></Envelope>"
                    ))
                    .with_status_code(status),
                )
                .unwrap();
            }
        });
        let gateway = device(
            &format!("{root_url}/device.xml"),
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::LOCALHOST,
        )
        .unwrap();
        let driver = Network;
        let gateway = driver.refresh(&gateway).unwrap();
        assert_eq!(gateway.public_ip, Ipv4Addr::new(100, 64, 1, 1));
        assert!(specific(&gateway, 25565).unwrap().is_none());
        let mapped = driver
            .map(&gateway, 25565, 25565, "NetherGate-fixture")
            .unwrap();
        assert_eq!(mapped.seconds, 600);
        assert!(driver.remove(&gateway, 25565, 25565, "foreign").is_err());
        assert!(state.lock().unwrap().is_some());
        driver
            .remove(&gateway, 25565, 25565, "NetherGate-fixture")
            .unwrap();
        assert!(state.lock().unwrap().is_none());
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        join.join().unwrap();
    }
}
