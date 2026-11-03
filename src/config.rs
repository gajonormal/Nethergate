use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Software {
    #[default]
    Paper,
    Fabric,
}
impl Software {
    pub fn jar(self) -> &'static str {
        match self {
            Self::Paper => "paper.jar",
            Self::Fabric => "fabric-server-launch.jar",
        }
    }
    pub fn manifest(self) -> &'static str {
        match self {
            Self::Paper => "paper.lock.json",
            Self::Fabric => "fabric.lock.json",
        }
    }
    pub fn loader(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Fabric => "fabric",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Self::Paper => "plugins",
            Self::Fabric => "mods",
        }
    }
    pub fn registry(self) -> &'static str {
        match self {
            Self::Paper => "nethergate-plugins.json",
            Self::Fabric => "nethergate-mods.json",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub software: Software,
    pub minecraft: String,
    pub contact: String,
    pub java: PathBuf,
    pub java_major: u32,
    pub memory_mb: u32,
    pub port: u16,
}

impl Config {
    pub fn new(minecraft: String, contact: String, java_major: u32) -> Result<Self> {
        let value = Self {
            software: Software::Paper,
            minecraft,
            contact,
            java: "java".into(),
            java_major,
            memory_mb: 2048,
            port: 25565,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let value: Self = serde_json::from_slice(&fs::read(directory.join("nethergate.json"))?)?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self
            .minecraft
            .split('.')
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .is_some_and(|v| v >= 26)
            && self.java_major < 25
        {
            bail!("Minecraft 26.1 e posteriores exigem Java 25 ou superior");
        }
        if self.minecraft.is_empty()
            || !self
                .minecraft
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        {
            bail!("Versão Minecraft inválida");
        }
        if self.contact.chars().any(char::is_control)
            || !(self.contact.contains('@') || self.contact.starts_with("https://"))
        {
            bail!("Indica um email ou URL HTTPS de contacto real");
        }
        if self.memory_mb < 512
            || self.port == 0
            || self.java_major < 21
            || self.java.as_os_str().is_empty()
        {
            bail!("Configuração inválida: RAM >= 512 MB, porta > 0 e Java >= 21");
        }
        Ok(())
    }
}

pub fn lock(directory: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".nethergate.lock"))?;
    file.try_lock_exclusive()
        .context("Outro processo Nethergate está a utilizar este servidor")?;
    Ok(file)
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}

pub fn property_text(text: &str, key: &str, value: &str) -> Result<String> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        || value.contains(['\r', '\n'])
    {
        bail!("Propriedade inválida; usa uma chave simples e um valor de uma linha");
    }
    let mut found = false;
    let mut output = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let candidate = trimmed.split(['=', ':', ' ', '\t']).next().unwrap_or("");
        if !trimmed.starts_with(['#', '!']) && candidate == key {
            if !found {
                output.push(format!("{key}={value}"));
            }
            found = true;
        } else {
            output.push(line.to_string());
        }
    }
    if !found {
        output.push(format!("{key}={value}"));
    }
    Ok(format!("{}\n", output.join("\n")))
}

pub fn set_property(path: &Path, key: &str, value: &str) -> Result<()> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    fs::write(path, property_text(&text, key, value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_profiles_remain_paper_and_fabric_is_explicit() {
        let old = serde_json::json!({"minecraft":"1.21.11","contact":"https://example.org","java":"java","java_major":21,"memory_mb":1024,"port":25565});
        let cfg: Config = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(cfg.software, Software::Paper);
        let mut fabric = old;
        fabric["software"] = serde_json::json!("fabric");
        let cfg: Config = serde_json::from_value(fabric).unwrap();
        assert_eq!(cfg.software.jar(), "fabric-server-launch.jar");
        assert_eq!(cfg.software.folder(), "mods");
    }
    #[test]
    fn preserves_comments_and_removes_duplicate_keys() {
        let result = property_text(
            "# hello\nmotd=Hi\nserver-port:1\nserver-port=2\n",
            "server-port",
            "25565",
        )
        .unwrap();
        assert_eq!(result, "# hello\nmotd=Hi\nserver-port=25565\n");
    }
    #[test]
    fn rejects_property_injection() {
        assert!(property_text("", "motd", "hello\nonline-mode=false").is_err());
        assert!(property_text("", "bad=key", "value").is_err());
    }
}
