use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

pub(crate) fn ps_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
}

pub fn run(root: &Path, directory: &Path, id: &str, repair: bool) -> Result<Value> {
    if !cfg!(windows) {
        return Ok(json!({"state":"unsupported"}));
    }
    let ip = crate::connection::bind_ip(directory)?;
    if ip.is_loopback() {
        return Ok(json!({"state":"local_only"}));
    }
    let adapter = crate::connection::adapters()?.into_iter().find(|a| std::net::IpAddr::V4(a.ip) == ip)
        .context("O IP configurado já não pertence à rede ativa. Para o servidor e escolhe o novo IP em Ligação.")?;
    let cfg = crate::config::Config::load(directory)?;
    execute(
        root,
        include_str!("../scripts/LanFirewall.ps1"),
        &json!({"id":id,"address":adapter.ip,"interface":adapter.name,"port":cfg.port,"java":ps_path(&cfg.java),"serverDirectory":ps_path(directory)}),
        if repair { "Repair" } else { "Inspect" },
        repair,
    )
}

pub(crate) fn execute(
    root: &Path,
    source: &str,
    plan_value: &Value,
    mode: &str,
    mutation: bool,
) -> Result<Value> {
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce)
        .map_err(|e| anyhow::anyhow!("Falha ao preparar operação: {e}"))?;
    let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let operation = root.join("hub-data/firewall").join(nonce);
    fs::create_dir_all(&operation)?;
    let script = operation.join("engine.ps1");
    let plan = operation.join("plan.json");
    fs::write(&script, source)?;
    crate::config::write_json(&plan, plan_value)?;
    let mut command = std::process::Command::new(powershell()?);
    crate::process::hide_window(&mut command);
    command
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(ps_path(&script))
        .args(["-Mode", mode, "-Plan"])
        .arg(ps_path(&plan))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("Não foi possível consultar a firewall Windows")?;
    use std::io::Read;
    let stdout = child
        .stdout
        .take()
        .context("Não foi possível abrir a saída do auxiliar de firewall")?;
    let stderr = child
        .stderr
        .take()
        .context("Não foi possível abrir a saída de erros do auxiliar de firewall")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(131072).read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(32768).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let timeout = if mutation { 180 } else { 25 };
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Não foi possível confirmar a operação de firewall a tempo. Verifica novamente antes de repetir. O pedido de administrador pode ainda estar aberto.");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let output = out
        .join()
        .map_err(|_| anyhow::anyhow!("Falha ao ler o resultado"))??;
    let errors = err
        .join()
        .map_err(|_| anyhow::anyhow!("Falha ao ler o diagnóstico"))??;
    let text = String::from_utf8_lossy(&output);
    let result: Value = serde_json::from_str(text.trim_start_matches('\u{feff}').trim())
        .with_context(|| {
            format!(
                "A firewall não devolveu um diagnóstico legível: {}",
                String::from_utf8_lossy(&errors)
            )
        })?;
    if !status.success() || result.get("error").is_some() {
        bail!(
            "{}",
            result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Não foi possível confirmar a regra Windows")
        );
    }
    Ok(result)
}

#[cfg(windows)]
pub(crate) fn powershell() -> Result<std::path::PathBuf> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    let size = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if size == 0 || size >= buffer.len() {
        bail!("Não foi possível localizar o PowerShell do Windows");
    }
    Ok(
        std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..size]))
            .join("WindowsPowerShell/v1.0/powershell.exe"),
    )
}
#[cfg(not(windows))]
pub(crate) fn powershell() -> Result<std::path::PathBuf> {
    bail!("Firewall automática disponível apenas no Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_verbatim_paths_without_building_shell_code() {
        assert_eq!(
            ps_path(Path::new(r"\\?\C:\Users\a b\server")),
            r"C:\Users\a b\server"
        );
        assert_eq!(
            ps_path(Path::new(r"\\?\UNC\host\share\server")),
            r"\\host\share\server"
        );
        assert_eq!(ps_path(Path::new("java")), "java");
    }
}
