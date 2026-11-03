use crate::{
    config::{self, Config, Software},
    paper::hash_file,
};
use anyhow::{ensure, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
pub struct Installed {
    pub minecraft: String,
    pub loader: String,
    pub installer: String,
    pub sha256: String,
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("Nethergate/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn get(http: &Client, endpoint: &str) -> Result<Value> {
    let response = http
        .get(format!("https://meta.fabricmc.net/v2/versions/{endpoint}"))
        .send()?
        .error_for_status()?;
    let mut data = Vec::new();
    response.take(4 * 1024 * 1024 + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() <= 4 * 1024 * 1024,
        "Resposta Fabric demasiado grande"
    );
    Ok(serde_json::from_slice(&data)?)
}
pub fn versions() -> Result<Vec<String>> {
    let values = get(&client()?, "game")?;
    Ok(values
        .as_array()
        .context("Lista Fabric inválida")?
        .iter()
        .filter(|v| v["stable"] == true)
        .filter_map(|v| v["version"].as_str().map(str::to_string))
        .collect())
}
fn valid_version(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() < 64
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b)),
        "Versão Fabric inválida"
    );
    Ok(())
}
pub fn install(directory: &Path, cfg: &Config) -> Result<()> {
    ensure!(cfg.software == Software::Fabric, "Este perfil não é Fabric");
    let jar = directory.join(cfg.software.jar());
    let receipt = directory.join(cfg.software.manifest());
    ensure!(
        !jar.exists() && !receipt.exists() && !directory.join("paper.jar").exists(),
        "Já existe uma instalação neste perfil; cria um perfil Fabric separado"
    );
    let http = client()?;
    let game = get(&http, "game")?;
    ensure!(
        game.as_array()
            .context("Lista Fabric inválida")?
            .iter()
            .any(|g| g["stable"] == true && g["version"] == cfg.minecraft),
        "A versão Minecraft não está marcada como estável no Fabric"
    );
    let loaders = get(&http, &format!("loader/{}", cfg.minecraft))?;
    let loader = loaders
        .as_array()
        .context("Lista de loaders Fabric inválida")?
        .iter()
        .find(|v| v["loader"]["stable"] == true)
        .and_then(|v| v["loader"]["version"].as_str())
        .context("Não existe loader Fabric estável para este Minecraft")?;
    let installers = get(&http, "installer")?;
    let installer = installers
        .as_array()
        .context("Lista de instaladores Fabric inválida")?
        .iter()
        .find(|v| v["stable"] == true)
        .and_then(|v| v["version"].as_str())
        .context("Não existe launcher Fabric estável")?;
    valid_version(loader)?;
    valid_version(installer)?;
    let partial = directory.join("fabric-server-launch.jar.part");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| -> Result<String> {
        let response = http
            .get(format!(
                "https://meta.fabricmc.net/v2/versions/loader/{}/{loader}/{installer}/server/jar",
                cfg.minecraft
            ))
            .send()?
            .error_for_status()?;
        let size = std::io::copy(&mut response.take(16 * 1024 * 1024 + 1), &mut output)?;
        output.flush()?;
        output.sync_all()?;
        ensure!(
            (4..=16 * 1024 * 1024).contains(&size),
            "Launcher Fabric com tamanho inválido"
        );
        let mut magic = [0u8; 4];
        fs::File::open(&partial)?.read_exact(&mut magic)?;
        ensure!(magic == *b"PK\x03\x04", "A resposta Fabric não é um JAR");
        hash_file(&partial)
    })();
    drop(output);
    let hash = match result {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    };
    fs::rename(&partial, &jar)?;
    config::write_json(
        &receipt,
        &Installed {
            minecraft: cfg.minecraft.clone(),
            loader: loader.into(),
            installer: installer.into(),
            sha256: hash,
        },
    )?;
    Ok(())
}
pub fn verify(directory: &Path, cfg: &Config) -> Result<Installed> {
    let receipt: Installed = serde_json::from_slice(
        &fs::read(directory.join("fabric.lock.json"))
            .context("Prepara primeiro o servidor Fabric")?,
    )?;
    valid_version(&receipt.loader)?;
    valid_version(&receipt.installer)?;
    ensure!(
        receipt.minecraft == cfg.minecraft
            && hash_file(&directory.join("fabric-server-launch.jar"))?
                .eq_ignore_ascii_case(&receipt.sha256),
        "A instalação Fabric difere do registo; preserva os ficheiros e verifica o perfil"
    );
    Ok(receipt)
}
