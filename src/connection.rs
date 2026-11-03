use crate::config;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct Adapter {
    pub name: String,
    pub ip: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
}

pub fn property(text: &str, key: &str, default: &str) -> Result<String> {
    let mut result = default.to_string();
    let mut continued = false;
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with(['#', '!']) {
            continue;
        }
        if continued {
            bail!("As propriedades usam continuação de linhas. Simplifica o ficheiro antes de gerir a ligação.");
        }
        continued = line.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1;
        let candidate = line.split(['=', ':', ' ', '\t']).next().unwrap_or("");
        if candidate == key {
            let rest = line[key.len()..].trim_start();
            result = rest
                .strip_prefix(['=', ':'])
                .unwrap_or(rest)
                .trim()
                .to_string();
            if result.contains('\\') {
                bail!("A propriedade {key} usa escapes não suportados pelo painel");
            }
        }
    }
    if continued {
        bail!("Existe uma continuação de linha incompleta nas propriedades");
    }
    Ok(result)
}

pub fn bind_ip(directory: &Path) -> Result<IpAddr> {
    let text = fs::read_to_string(directory.join("server.properties"))?;
    let host = property(&text, "server-ip", "")?;
    match host.as_str() {
        "" => Ok(Ipv4Addr::UNSPECIFIED.into()),
        "localhost" => Ok(Ipv4Addr::LOCALHOST.into()),
        _ => host.parse().context("server-ip não é um IP literal suportado. O painel não faz consultas DNS para este campo"),
    }
}

pub fn endpoint(ip: IpAddr, port: u16) -> SocketAddr {
    SocketAddr::new(
        if ip.is_unspecified() {
            if ip.is_ipv4() {
                Ipv4Addr::LOCALHOST.into()
            } else {
                std::net::Ipv6Addr::LOCALHOST.into()
            }
        } else {
            ip
        },
        port,
    )
}

pub fn local_address(directory: &Path, port: u16) -> Option<String> {
    let endpoint = endpoint(bind_ip(directory).ok()?, port);
    Some(if endpoint.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) {
        format!("localhost:{port}")
    } else {
        endpoint.to_string()
    })
}

pub fn scope(ip: IpAddr) -> &'static str {
    if ip.is_loopback() {
        "local"
    } else if ip.is_unspecified() {
        "all"
    } else if matches!(ip, IpAddr::V4(ip) if ip.is_private()) {
        "lan"
    } else {
        "custom"
    }
}

#[cfg(windows)]
pub fn adapters() -> Result<Vec<Adapter>> {
    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA},
        NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
        Networking::WinSock::{AF_INET, SOCKADDR_IN},
    };
    let mut bytes = 15_360u32;
    for _ in 0..3 {
        if bytes > 1_048_576 {
            bail!("Lista de interfaces demasiado grande");
        }
        // u64 storage provides the native structures' required 8-byte alignment.
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: owned aligned buffer stays alive throughout pointer traversal.
        let result = unsafe {
            GetAdaptersAddresses(
                AF_INET as u32,
                GAA_FLAG_SKIP_ANYCAST
                    | GAA_FLAG_SKIP_MULTICAST
                    | GAA_FLAG_SKIP_DNS_SERVER
                    | GAA_FLAG_INCLUDE_GATEWAYS,
                std::ptr::null(),
                first,
                &mut bytes,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if result == ERROR_NO_DATA {
            return Ok(vec![]);
        }
        if result != 0 {
            bail!("Não foi possível listar interfaces de rede (Windows {result})");
        }
        let mut list = Vec::new();
        let mut adapter = first;
        while !adapter.is_null() {
            // SAFETY: Windows returned linked structures inside the valid buffer.
            let item = unsafe { &*adapter };
            if item.OperStatus == IfOperStatusUp
                && [6, 71].contains(&item.IfType)
                && !item.FirstGatewayAddress.is_null()
            {
                let mut name = Vec::new();
                if !item.FriendlyName.is_null() {
                    for index in 0..256 {
                        let ch = unsafe { *item.FriendlyName.add(index) };
                        if ch == 0 {
                            break;
                        }
                        name.push(ch);
                    }
                }
                let name = String::from_utf16_lossy(&name);
                let mut gateway = None;
                let mut gateway_address = item.FirstGatewayAddress;
                while !gateway_address.is_null() {
                    // SAFETY: linked structures are owned by the live adapter buffer.
                    let entry = unsafe { &*gateway_address };
                    if !entry.Address.lpSockaddr.is_null()
                        && entry.Address.iSockaddrLength as usize
                            >= std::mem::size_of::<SOCKADDR_IN>()
                    {
                        let socket = unsafe { &*entry.Address.lpSockaddr.cast::<SOCKADDR_IN>() };
                        if socket.sin_family == AF_INET {
                            let octets = unsafe { socket.sin_addr.S_un.S_un_b };
                            gateway = Some(Ipv4Addr::new(
                                octets.s_b1,
                                octets.s_b2,
                                octets.s_b3,
                                octets.s_b4,
                            ));
                            break;
                        }
                    }
                    gateway_address = entry.Next;
                }
                let mut address = item.FirstUnicastAddress;
                while !address.is_null() {
                    let entry = unsafe { &*address };
                    if !entry.Address.lpSockaddr.is_null()
                        && entry.Address.iSockaddrLength as usize
                            >= std::mem::size_of::<SOCKADDR_IN>()
                    {
                        let socket = unsafe { &*entry.Address.lpSockaddr.cast::<SOCKADDR_IN>() };
                        if socket.sin_family == AF_INET {
                            let octets = unsafe { socket.sin_addr.S_un.S_un_b };
                            let ip =
                                Ipv4Addr::new(octets.s_b1, octets.s_b2, octets.s_b3, octets.s_b4);
                            if ip.is_private() {
                                list.push(Adapter {
                                    name: name.clone(),
                                    ip,
                                    gateway,
                                });
                            }
                        }
                    }
                    address = entry.Next;
                }
            }
            adapter = item.Next;
        }
        list.sort_by_key(|a| a.ip);
        list.dedup_by_key(|a| a.ip);
        return Ok(list);
    }
    bail!("As interfaces mudaram durante a consulta. Tenta atualizar.")
}

#[cfg(not(windows))]
pub fn adapters() -> Result<Vec<Adapter>> {
    bail!("A deteção de IPs da rede de casa está disponível apenas no Windows nesta versão")
}

pub fn save(directory: &Path, mode: &str, address: &str, adapters: &[Adapter]) -> Result<()> {
    let target = match mode {
        "local" => Ipv4Addr::LOCALHOST,
        "lan" => {
            let ip: Ipv4Addr = address
                .parse()
                .context("Escolhe um IP da lista da rede de casa")?;
            if !ip.is_private() || !adapters.iter().any(|a| a.ip == ip) {
                bail!(
                    "Esse IP não pertence a uma interface de rede de casa ativa. Atualiza a lista."
                );
            }
            ip
        }
        _ => bail!("Modo de ligação inválido"),
    };
    let path = directory.join("server.properties");
    let text = fs::read_to_string(&path)?;
    let authenticated = property(&text, "online-mode", "true")?;
    if mode == "lan" && authenticated != "true" {
        bail!("Ativa a autenticação oficial antes de permitir ligações da rede");
    }
    // Parse before rewriting: do not silently edit unsupported Properties syntax.
    property(&text, "server-ip", "")?;
    let updated = config::property_text(&text, "server-ip", &target.to_string())?;
    fs::write(directory.join("server.properties.before-connection"), &text)?;
    let temp = directory.join("server.properties.connection.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("Existe uma gravação de ligação pendente")?;
    file.write_all(updated.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path).context("Não foi possível aplicar a ligação")?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub status: &'static str,
    pub address: String,
    pub version: Option<String>,
    pub elapsed_ms: Option<u128>,
    pub detail: String,
}

fn varint(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut b = (value & 127) as u8;
        value >>= 7;
        if value != 0 {
            b |= 128;
        }
        bytes.push(b);
        if value == 0 {
            break;
        }
    }
    bytes
}
fn read_varint(input: &mut impl Read) -> Result<usize> {
    let mut result = 0u32;
    for index in 0..5 {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        if index == 4 && byte[0] & 0xf0 != 0 {
            bail!("VarInt inválido");
        }
        result |= ((byte[0] & 127) as u32) << (index * 7);
        if byte[0] & 128 == 0 {
            return Ok(result as usize);
        }
    }
    bail!("VarInt demasiado longo")
}

fn status_response(stream: &mut TcpStream, endpoint: SocketAddr) -> Result<String> {
    let host = endpoint.ip().to_string();
    let mut handshake = vec![0];
    handshake.extend(varint(47)); // Discovery only; not a login/version assertion.
    handshake.extend(varint(host.len() as u32));
    handshake.extend(host.as_bytes());
    handshake.extend(endpoint.port().to_be_bytes());
    handshake.push(1);
    stream.write_all(&varint(handshake.len() as u32))?;
    stream.write_all(&handshake)?;
    stream.write_all(&[1, 0])?;
    struct DeadlineReader<'a> {
        stream: &'a mut TcpStream,
        deadline: Instant,
    }
    impl Read for DeadlineReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(remaining))?;
            self.stream.read(buffer)
        }
    }
    let mut reader = DeadlineReader {
        stream,
        deadline: Instant::now() + Duration::from_millis(1500),
    };
    let packet_len = read_varint(&mut reader)?;
    if packet_len == 0 || packet_len > 1_048_576 {
        bail!("Tamanho de resposta Minecraft inválido");
    }
    let mut packet = vec![0; packet_len];
    reader.read_exact(&mut packet)?;
    parse_status_packet(&packet)
}

fn parse_status_packet(packet: &[u8]) -> Result<String> {
    let mut cursor = Cursor::new(packet);
    if read_varint(&mut cursor)? != 0 {
        bail!("Pacote de estado inesperado");
    }
    let len = read_varint(&mut cursor)?;
    let start = cursor.position() as usize;
    if len != packet.len().saturating_sub(start) {
        bail!("Comprimento JSON inválido");
    }
    let json: serde_json::Value = serde_json::from_slice(&packet[start..])?;
    let version = json
        .pointer("/version/name")
        .and_then(|v| v.as_str())
        .context("Resposta sem versão Minecraft")?;
    if json
        .pointer("/version/protocol")
        .and_then(|v| v.as_i64())
        .is_none()
        || version.len() > 200
    {
        bail!("Resposta Minecraft inválida");
    }
    Ok(version.to_string())
}

pub fn probe(endpoint: SocketAddr) -> Probe {
    let start = Instant::now();
    let mut result = Probe {
        status: "closed",
        address: endpoint.to_string(),
        version: None,
        elapsed_ms: None,
        detail: String::new(),
    };
    let mut stream = match TcpStream::connect_timeout(&endpoint, Duration::from_millis(1200)) {
        Ok(stream) => stream,
        Err(error) => {
            result.detail = format!("A ligação TCP falhou: {error}");
            return result;
        }
    };
    result.status = "tcp_only";
    let response = (|| {
        stream.set_read_timeout(Some(Duration::from_millis(1200)))?;
        stream.set_write_timeout(Some(Duration::from_millis(1200)))?;
        status_response(&mut stream, endpoint)
    })();
    result.elapsed_ms = Some(start.elapsed().as_millis());
    match response {
        Ok(version) => {
            result.status = "minecraft";
            result.version = Some(version);
            result.detail = "O protocolo de estado Minecraft respondeu neste computador.".into();
        }
        Err(error) => {
            result.detail =
                format!("A porta TCP respondeu, mas o estado Minecraft não foi confirmado: {error}")
        }
    }
    result
}

pub fn available(endpoint: SocketAddr) -> &'static str {
    match TcpListener::bind(endpoint) {
        Ok(_listener) => "available",
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => "occupied",
        Err(_) => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_addresses_without_claiming_public_access() {
        assert_eq!(scope("127.0.0.1".parse().unwrap()), "local");
        assert_eq!(scope("192.168.1.5".parse().unwrap()), "lan");
        assert_eq!(scope("0.0.0.0".parse().unwrap()), "all");
        assert_eq!(scope("100.64.1.1".parse().unwrap()), "custom");
        assert_eq!(
            endpoint("::".parse().unwrap(), 25565).to_string(),
            "[::1]:25565"
        );
        assert!(property("motd=hello\\\nserver-ip=0.0.0.0\n", "server-ip", "").is_err());
    }
    #[test]
    fn rejects_malformed_status_lengths_and_varints() {
        assert!(read_varint(&mut Cursor::new([255, 255, 255, 255, 255])).is_err());
        assert!(parse_status_packet(&[0, 127, b'{']).is_err());
        assert!(parse_status_packet(&[1, 2, b'{', b'}']).is_err());
        let json = br#"{"version":{"name":"1.21.11","protocol":774}}"#;
        let mut packet = vec![0];
        packet.extend(varint(json.len() as u32));
        packet.extend(json);
        assert_eq!(parse_status_packet(&packet).unwrap(), "1.21.11");
    }
    #[test]
    fn tcp_only_service_is_not_misreported_as_minecraft() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
        });
        assert_eq!(probe(endpoint).status, "tcp_only");
        worker.join().unwrap();
    }
}
