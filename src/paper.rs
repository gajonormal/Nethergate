use crate::config::{self, Config};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[derive(Deserialize)]
struct Build {
    id: u64,
    channel: String,
    downloads: HashMap<String, Download>,
}
#[derive(Deserialize)]
struct Download {
    url: String,
    checksums: Checksums,
}
#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}

#[derive(Serialize, Deserialize)]
pub struct Installed {
    pub minecraft: String,
    pub build: u64,
    pub sha256: String,
}

pub fn versions(contact: &str) -> Result<Vec<String>> {
    Config::new("lookup".into(), contact.into(), 21)?;
    let client = Client::builder()
        .user_agent(format!(
            "Nethergate/{} ({contact})",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?;
    let result: serde_json::Value = client
        .get("https://fill.papermc.io/v3/projects/paper")
        .send()?
        .error_for_status()?
        .json()?;
    let groups = result
        .get("versions")
        .and_then(|v| v.as_object())
        .context("Lista de versões inesperada")?;
    let mut versions: Vec<String> = groups
        .values()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    versions.sort_by_key(|v| {
        std::cmp::Reverse(
            v.split('.')
                .map(|p| p.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>(),
        )
    });
    versions.dedup();
    Ok(versions)
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn install(directory: &Path, config: &Config) -> Result<()> {
    let jar = directory.join("paper.jar");
    let manifest = directory.join("paper.lock.json");
    if jar.exists() || manifest.exists() {
        bail!("Já existe uma instalação. Cria outro diretório para uma nova versão; atualização de mundos ainda não é suportada.");
    }
    let client = Client::builder()
        .user_agent(format!(
            "Nethergate/{} ({})",
            env!("CARGO_PKG_VERSION"),
            config.contact
        ))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()?;
    let url = format!(
        "https://fill.papermc.io/v3/projects/paper/versions/{}/builds",
        config.minecraft
    );
    let builds: Vec<Build> = client.get(url).send()?.error_for_status()?.json()?;
    let build = builds
        .into_iter()
        .filter(|b| b.channel == "STABLE")
        .max_by_key(|b| b.id)
        .context("Não existem builds estáveis para esta versão")?;
    let download = build
        .downloads
        .get("server:default")
        .context("Resposta PaperMC sem server:default")?;
    if !download.url.starts_with("https://") {
        bail!("O download tem de usar HTTPS");
    }
    if download.checksums.sha256.len() != 64
        || !download
            .checksums
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        bail!("Checksum SHA-256 inválido na resposta PaperMC");
    }
    let partial = directory.join("paper.jar.part");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .context(
            "Não foi possível criar paper.jar.part; verifica se existe um download interrompido",
        )?;
    println!(
        "A descarregar Paper {} build {}…",
        config.minecraft, build.id
    );
    let result = (|| -> Result<()> {
        client
            .get(&download.url)
            .send()?
            .error_for_status()?
            .copy_to(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        if !hash_file(&partial)?.eq_ignore_ascii_case(&download.checksums.sha256) {
            bail!("Falha de integridade: SHA-256 diferente do publicado pelo PaperMC");
        }
        Ok(())
    })();
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, jar)?;
    config::write_json(
        &manifest,
        &Installed {
            minecraft: config.minecraft.clone(),
            build: build.id,
            sha256: download.checksums.sha256.clone(),
        },
    )?;
    println!("Paper instalado e checksum verificado.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_official_build_response() {
        let build: Build = serde_json::from_str(r#"{"id":42,"channel":"STABLE","downloads":{"server:default":{"url":"https://example.org/paper.jar","checksums":{"sha256":"abc"}}}}"#).unwrap();
        assert_eq!(build.id, 42);
        assert!(build.downloads.contains_key("server:default"));
    }
}
