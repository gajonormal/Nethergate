use crate::{
    config::{self, Config},
    paper::{self, Installed},
};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

fn java_major(text: &str) -> Option<u32> {
    let version = text.split('"').nth(1)?;
    let mut parts = version.split(['.', '-', '+']);
    let first = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub fn check_java(config: &Config) -> Result<()> {
    let mut probe = Command::new(&config.java);
    probe.arg("-version");
    hide_window(&mut probe);
    let result = probe.output().context(
        "Java não encontrado. Instala Temurin e configura o caminho `java` em nethergate.json",
    )?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    if !result.status.success() {
        bail!("A verificação do Java falhou: {text}");
    }
    let major = java_major(&text).context("Não foi possível identificar a versão Java")?;
    if major < config.java_major {
        bail!(
            "Java {major} detetado; esta configuração exige Java {} ou superior",
            config.java_major
        );
    }
    if !text.contains("64-Bit") && !text.contains("64-bit") {
        bail!("É necessário um ambiente Java de 64 bits");
    }
    println!("Java {major} de 64 bits detetado.");
    Ok(())
}

pub fn prepare(directory: &Path, config: &Config, accept_eula: bool) -> Result<()> {
    crate::plugins::ensure_no_pending(directory)?;
    crate::backups::ensure_no_pending(directory)?;
    check_java(config)?;
    if config.software == crate::config::Software::Fabric {
        crate::fabric::verify(directory, config)?;
    } else {
        let installed: Installed = serde_json::from_slice(
            &fs::read(directory.join("paper.lock.json"))
                .context("Executa primeiro `nethergated install`")?,
        )?;
        if installed.minecraft != config.minecraft {
            bail!("A versão configurada difere da instalação");
        }
        if !paper::hash_file(&directory.join("paper.jar"))?.eq_ignore_ascii_case(&installed.sha256)
        {
            bail!("O paper.jar foi alterado desde a instalação");
        }
    }
    let eula = directory.join("eula.txt");
    let accepted = match fs::read_to_string(&eula) {
        Ok(text) => text.lines().any(|line| line.trim() == "eula=true"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if !accepted && !accept_eula {
        bail!("Lê https://aka.ms/MinecraftEULA e usa --accept-eula para confirmar a aceitação");
    }
    if accept_eula {
        config::set_property(&eula, "eula", "true")?;
    }
    config::set_property(
        &directory.join("server.properties"),
        "server-port",
        &config.port.to_string(),
    )?;
    Ok(())
}

pub fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

pub fn java_command(directory: &Path, config: &Config) -> Command {
    let mut command = Command::new(&config.java);
    command
        .arg("-Xms512M")
        .arg(format!("-Xmx{}M", config.memory_mb))
        .args(["-jar", config.software.jar()])
        .current_dir(directory);
    if config.software == crate::config::Software::Fabric {
        command.arg("nogui");
    } else {
        command.args(["--nogui", "--nojline"]);
    }
    command
}

pub fn start(directory: &Path, config: &Config, accept_eula: bool) -> Result<()> {
    prepare(directory, config, accept_eula)?;
    println!(
        "A iniciar Minecraft em localhost:{} — escreve `stop` para guardar e terminar.",
        config.port
    );
    let mut child = java_command(directory, config)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Não foi possível iniciar Java")?;
    let _sleep_guard = match crate::energy::SleepGuard::acquire() {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("Proteção de energia indisponível: {error}");
            None
        }
    };
    let status = child.wait().context("Falha ao aguardar Java")?;
    if !status.success() {
        bail!("O servidor terminou com erro: {status}");
    }
    println!("Servidor terminado.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_java_versions() {
        assert_eq!(java_major("openjdk version \"21.0.9\""), Some(21));
        assert_eq!(java_major("openjdk version \"25-ea\""), Some(25));
        assert_eq!(java_major("java version \"1.8.0_402\""), Some(8));
        assert_eq!(java_major("unrecognized output"), None);
    }
}
