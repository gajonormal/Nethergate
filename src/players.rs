use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Access {
    pub enabled: bool,
    pub authenticated: bool,
    pub players: Vec<Player>,
}

pub fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub fn access(directory: &Path) -> Result<Access> {
    let text = fs::read_to_string(directory.join("server.properties"))?;
    let boolean = |key: &str, default: bool| -> Result<bool> {
        let mut result = default;
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
                    .parse()
                    .with_context(|| format!("Propriedade {key} inválida"))?;
            }
        }
        Ok(result)
    };
    let path = directory.join("whitelist.json");
    let mut players: Vec<Player> = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context(
            "Não foi possível ler whitelist.json; preserva o ficheiro e consulta a consola",
        )?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };
    players.sort_by_key(|p| p.name.to_ascii_lowercase());
    Ok(Access {
        enabled: boolean("white-list", false)?,
        authenticated: boolean("online-mode", true)?,
        players,
    })
}

pub fn command(operation: &str, name: &str) -> Result<String> {
    match operation {
        "enable" => Ok("minecraft:whitelist on".into()),
        "disable" => Ok("minecraft:whitelist off".into()),
        "add" | "remove" if valid_name(name) => {
            Ok(format!("minecraft:whitelist {operation} {name}"))
        }
        "add" | "remove" => bail!("Usa o nome Java: 3–16 letras, números ou underscore"),
        _ => bail!("Operação de whitelist inválida"),
    }
}

pub fn reached(access: &Access, operation: &str, name: &str) -> bool {
    match operation {
        "enable" => access.enabled,
        "disable" => !access.enabled,
        "add" => access
            .players
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name)),
        "remove" => !access
            .players
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name)),
        _ => false,
    }
}

/// Accept only the vanilla console response, never player chat containing it.
pub fn online_response(line: &str) -> Option<Vec<String>> {
    let (_, message) = line
        .split_once(" INFO]: ")
        .or_else(|| line.split_once("Server thread/INFO]: "))?;
    let rest = message.strip_prefix("There are ")?;
    let (counts, names) = rest.split_once(" players online:")?;
    let (count, maximum) = counts.split_once(" of a max of ")?;
    let count = count.parse::<usize>().ok()?;
    maximum.parse::<usize>().ok()?;
    let names: Vec<String> = names
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if names.len() != count || names.iter().any(|n| !valid_name(n)) {
        return None;
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_names_and_parses_only_console_lists() {
        assert_eq!(
            online_response(
                "[21:00:00] [Server thread/INFO]: There are 0 of a max of 20 players online: "
            )
            .unwrap(),
            Vec::<String>::new()
        );
        assert!(online_response(
            "[21:00:00] [Server thread/INFO]: <Alice> There are 0 of a max of 20 players online: "
        )
        .is_none());
        assert!(valid_name("PlayerOne"));
        for name in [
            "ab",
            "a b",
            "../ops",
            "test\nstop",
            "player;op",
            "ábc",
            "12345678901234567",
        ] {
            assert!(command("add", name).is_err());
        }
        assert!(command("op", "PlayerOne").is_err());
        assert_eq!(
            online_response(
                "[12:00:00 INFO]: There are 2 of a max of 20 players online: Alice, Bob"
            )
            .unwrap(),
            vec!["Alice", "Bob"]
        );
        assert_eq!(
            online_response("[12:00:00 INFO]: There are 0 of a max of 20 players online: ")
                .unwrap()
                .len(),
            0
        );
        assert!(online_response(
            "[12:00:00 INFO]: <Alice> There are 0 of a max of 20 players online: "
        )
        .is_none());
        assert!(online_response(
            "[12:00:00 INFO]: There are 2 of a max of 20 players online: Alice"
        )
        .is_none());
    }
}
