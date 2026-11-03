use crate::config;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gameplay {
    pub difficulty: String,
    pub gamemode: String,
    pub max_players: u16,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub pvp: bool,
}

impl Gameplay {
    pub fn load(directory: &Path) -> Result<Self> {
        let text = fs::read_to_string(directory.join("server.properties"))?;
        let value = |key: &str, default: &str| -> Result<String> {
            let mut result = default.to_string();
            for line in text.lines() {
                let line = line.trim_start();
                if line.starts_with(['#', '!']) {
                    continue;
                }
                let candidate = line.split(['=', ':', ' ', '\t']).next().unwrap_or("");
                if candidate == key {
                    let rest = line[key.len()..].trim_start();
                    result = rest
                        .strip_prefix(['=', ':'])
                        .unwrap_or(rest)
                        .trim()
                        .to_string();
                    if result.contains('\\') {
                        bail!(
                            "A propriedade {key} usa escapes; simplifica-a antes de editar no hub"
                        );
                    }
                }
            }
            Ok(result)
        };
        let difficulty = value("difficulty", "easy")?;
        let gamemode = value("gamemode", "survival")?;
        let settings = Self {
            difficulty: match difficulty.as_str() {
                "0" => "peaceful",
                "1" => "easy",
                "2" => "normal",
                "3" => "hard",
                _ => &difficulty,
            }
            .into(),
            gamemode: match gamemode.as_str() {
                "0" => "survival",
                "1" => "creative",
                "2" => "adventure",
                "3" => "spectator",
                _ => &gamemode,
            }
            .into(),
            max_players: value("max-players", "20")?
                .parse()
                .context("Limite de jogadores inválido")?,
            view_distance: value("view-distance", "10")?
                .parse()
                .context("Distância de visão inválida")?,
            simulation_distance: value("simulation-distance", "10")?
                .parse()
                .context("Distância de simulação inválida")?,
            pvp: value("pvp", "true")?.parse().context("PvP inválido")?,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<()> {
        if !["peaceful", "easy", "normal", "hard"].contains(&self.difficulty.as_str())
            || !["survival", "creative", "adventure", "spectator"].contains(&self.gamemode.as_str())
            || !(1..=1000).contains(&self.max_players)
            || !(2..=32).contains(&self.view_distance)
            || !(2..=32).contains(&self.simulation_distance)
        {
            bail!("Escolhe opções válidas: 1–1000 jogadores e distâncias entre 2 e 32 chunks");
        }
        Ok(())
    }

    pub fn save(&self, directory: &Path) -> Result<()> {
        self.validate()?;
        let path = directory.join("server.properties");
        let mut text = fs::read_to_string(&path)?;
        // Keep a complete previous file; publish all six settings together.
        let previous = directory.join("server.properties.before-gameplay");
        fs::write(&previous, &text)?;
        for (key, value) in [
            ("difficulty", self.difficulty.clone()),
            ("gamemode", self.gamemode.clone()),
            ("max-players", self.max_players.to_string()),
            ("view-distance", self.view_distance.to_string()),
            ("simulation-distance", self.simulation_distance.to_string()),
            ("pvp", self.pvp.to_string()),
        ] {
            text = config::property_text(&text, key, &value)?;
        }
        let temp = directory.join("server.properties.gameplay.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .context("Existe uma gravação pendente das definições de jogo")?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path).context("Não foi possível aplicar as definições de jogo")?;
        Ok(())
    }
}
