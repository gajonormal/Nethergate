//! Portable Windows runtimes. No registry, PATH or system Java changes.
use crate::{
    config::{self, Config},
    paper, process,
};
use anyhow::{bail, Context, Result};
use reqwest::{blocking::Client, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
struct Asset {
    binary: Binary,
    vendor: String,
    version: Version,
    release_name: String,
}
#[derive(Deserialize)]
struct Binary {
    architecture: String,
    image_type: String,
    os: String,
    jvm_impl: String,
    package: Package,
}
#[derive(Deserialize)]
struct Version {
    major: u32,
}
#[derive(Deserialize)]
struct Package {
    link: String,
    checksum: String,
    size: u64,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    major: u32,
    executable: String,
    executable_sha256: String,
    archive_sha256: String,
    release: String,
    source: String,
}

fn verify_download(path: &Path, size: u64, checksum: &str) -> Result<()> {
    if fs::metadata(path)?.len() != size || !paper::hash_file(path)?.eq_ignore_ascii_case(checksum)
    {
        bail!("Falha de integridade no download Java (tamanho/SHA-256)");
    }
    Ok(())
}

fn select_asset(body: &str, major: u32) -> Result<Asset> {
    let assets: Vec<Asset> = serde_json::from_str(body)?;
    let asset = assets
        .into_iter()
        .find(|a| {
            a.version.major == major
                && a.vendor == "eclipse"
                && a.binary.architecture == "x64"
                && a.binary.os == "windows"
                && a.binary.image_type == "jre"
                && a.binary.jvm_impl == "hotspot"
        })
        .context("A Adoptium não publicou um runtime compatível")?;
    let p = &asset.binary.package;
    let url = Url::parse(&p.link)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.path().starts_with(&format!(
            "/adoptium/temurin{major}-binaries/releases/download/"
        ))
        || !url.path().ends_with(".zip")
        || p.size == 0
        || p.size > 256 * 1024 * 1024
        || p.checksum.len() != 64
        || !p.checksum.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("Metadados do runtime inválidos; nenhum executável foi instalado");
    }
    Ok(asset)
}

fn cached(directory: &Path, major: u32, cfg: &Config) -> Result<PathBuf> {
    let receipt: Receipt = serde_json::from_slice(&fs::read(directory.join("installed.json"))?)?;
    let relative = Path::new(&receipt.executable);
    if receipt.major != major
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("Registo do Java gerido inválido");
    }
    let exe = directory.join(relative).canonicalize()?;
    if !exe.starts_with(directory.canonicalize()?)
        || !paper::hash_file(&exe)?.eq_ignore_ascii_case(&receipt.executable_sha256)
    {
        bail!("O Java gerido foi alterado. Escolhe um runtime válido nas definições");
    }
    let mut probe = Config::new(cfg.minecraft.clone(), cfg.contact.clone(), major)?;
    probe.java = exe.clone();
    process::check_java(&probe)?;
    Ok(exe)
}

pub fn managed(root: &Path, cfg: &Config, mut log: impl FnMut(String)) -> Result<PathBuf> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        bail!("A instalação automática está disponível em Windows x64; indica um Java instalado nas definições");
    }
    let major = cfg.java_major;
    if ![21, 25].contains(&major) {
        bail!("A instalação automática suporta Java 21 e 25. Para outras versões indica o executável nas definições");
    }
    let base = root.join("runtimes");
    fs::create_dir_all(&base)?;
    let _guard = config::lock(&base)
        .context("Outro servidor está a preparar Java. Aguarda e tenta novamente")?;
    let destination = base.join(format!("temurin-{major}-windows-x64"));
    if destination.exists() {
        log(format!(
            "[Nethergate] A verificar o Java {major} já guardado…"
        ));
        return cached(&destination, major, cfg);
    }
    log(format!(
        "[Nethergate] A consultar o Temurin Java {major} oficial…"
    ));
    let client = Client::builder()
        .user_agent(concat!("Nethergate/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .redirect(Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" || attempt.previous().len() >= 10 {
                attempt.error("Redirecionamento de download recusado")
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let response=client.get(format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse")).send()?.error_for_status()?;
    let mut body = String::new();
    response.take(1024 * 1024).read_to_string(&mut body)?;
    let asset = select_asset(&body, major)?;
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| anyhow::anyhow!("{e}"))?;
    let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let stage = base.join(format!("stage-{nonce}"));
    fs::create_dir(&stage)?;
    let result = (|| {
        let archive = stage.join("runtime.zip");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&archive)?;
        log(format!(
            "[Nethergate] A descarregar Java {major} ({:.0} MB)…",
            asset.binary.package.size as f64 / 1048576.0
        ));
        let response = client
            .get(&asset.binary.package.link)
            .send()?
            .error_for_status()?;
        let count = std::io::copy(&mut response.take(asset.binary.package.size + 1), &mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        if count != asset.binary.package.size {
            bail!("Falha de integridade no download Java (tamanho/SHA-256)");
        }
        verify_download(
            &archive,
            asset.binary.package.size,
            &asset.binary.package.checksum,
        )?;
        log("[Nethergate] SHA-256 confirmado. A extrair e testar o Java…".into());
        let extracted = stage.join("extracted");
        let script = stage.join("extract.ps1");
        fs::write(&script, include_str!("../scripts/Extract-Java.ps1"))?;
        let mut command = std::process::Command::new(crate::firewall::powershell()?);
        process::hide_window(&mut command);
        let output = command
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(crate::firewall::ps_path(&script))
            .arg("-Archive")
            .arg(crate::firewall::ps_path(&archive))
            .arg("-Destination")
            .arg(crate::firewall::ps_path(&extracted))
            .output()?;
        if !output.status.success() {
            bail!(
                "Falha ao extrair Java: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let candidates: Vec<PathBuf> = fs::read_dir(&extracted)?
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("bin/java.exe"))
            .filter(|p| p.is_file())
            .collect();
        if candidates.len() != 1 {
            bail!("Arquivo Java sem um único bin/java.exe reconhecido");
        }
        let executable = candidates[0]
            .strip_prefix(&extracted)?
            .to_string_lossy()
            .into_owned();
        let receipt = Receipt {
            major,
            executable,
            executable_sha256: paper::hash_file(&candidates[0])?,
            archive_sha256: asset.binary.package.checksum,
            release: asset.release_name,
            source: asset.binary.package.link,
        };
        config::write_json(&extracted.join("installed.json"), &receipt)?;
        cached(&extracted, major, cfg)?;
        fs::rename(&extracted, &destination)?;
        cached(&destination, major, cfg)
    })();
    // Only our newly created staging folder, resolved inside the runtime root.
    if let (Ok(resolved), Ok(base)) = (stage.canonicalize(), base.canonicalize()) {
        if resolved.starts_with(&base) && resolved != base {
            let _ = fs::remove_dir_all(resolved);
        }
    }
    let executable = result?;
    log(format!("[Nethergate] Java {major} instalado e verificado."));
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_truncated_or_modified_archive_before_extraction() {
        let directory =
            std::env::temp_dir().join(format!("nethergate-java-integrity-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let archive = directory.join("sample.zip");
        fs::write(&archive, b"verified bytes").unwrap();
        let sha = paper::hash_file(&archive).unwrap();
        assert!(verify_download(&archive, 14, &sha).is_ok());
        assert!(verify_download(&archive, 15, &sha).is_err());
        assert!(verify_download(&archive, 14, &"0".repeat(64)).is_err());
    }
    #[test]
    fn rejects_untrusted_or_incompatible_download_metadata() {
        let good = serde_json::json!([{"vendor":"eclipse","version":{"major":21},"release_name":"test","binary":{"architecture":"x64","image_type":"jre","os":"windows","jvm_impl":"hotspot","package":{"link":"https://github.com/adoptium/temurin21-binaries/releases/download/test/runtime.zip","checksum":"a".repeat(64),"size":100}}}]);
        assert!(select_asset(&good.to_string(), 21).is_ok());
        assert!(select_asset(&good.to_string(), 25).is_err());
        for (key, value) in [
            (
                "link",
                serde_json::json!(
                    "http://github.com/adoptium/temurin21-binaries/releases/download/test/a.zip"
                ),
            ),
            ("link", serde_json::json!("https://example.org/a.zip")),
            ("checksum", serde_json::json!("bad")),
            ("size", serde_json::json!(999999999)),
        ] {
            let mut bad = good.clone();
            bad[0]["binary"]["package"][key] = value;
            assert!(select_asset(&bad.to_string(), 21).is_err());
        }
    }
    #[test]
    #[ignore = "Downloads and runs official Temurin; opt-in network integration"]
    fn installs_and_reuses_verified_portable_java() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("hub-data/java-integration");
        fs::create_dir_all(&root).unwrap();
        for major in [21, 25] {
            let cfg = Config::new("1.21.11".into(), "https://example.org".into(), major).unwrap();
            let first = managed(&root, &cfg, |line| println!("{line}")).unwrap();
            let second = managed(&root, &cfg, |line| println!("{line}")).unwrap();
            assert_eq!(first, second);
            println!("Verified portable runtime: {}", first.display());
        }
    }
}
