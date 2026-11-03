//! Authenticated player operations and online-player observation.
use super::*;

impl Hub {
    pub fn player_access(&self, id: &str) -> Result<players::Access> {
        players::access(&self.entry(id)?.directory)
    }

    pub fn player_operation(
        &self,
        id: &str,
        operation: &str,
        name: &str,
    ) -> Result<players::Access> {
        let command = players::command(operation, name)?;
        let command = if Config::load(&self.entry(id)?.directory)?.software
            == crate::config::Software::Fabric
        {
            command.trim_start_matches("minecraft:").to_string()
        } else {
            command
        };
        let directory = self.entry(id)?.directory;
        let runtime = self.runtime(id)?;
        let after;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state != "online" {
                bail!("Inicia o servidor para gerir a whitelist");
            }
            if rt.players_busy {
                bail!("Aguarda a operação de jogadores em curso");
            }
            let current = players::access(&directory)?;
            if !current.authenticated {
                bail!("Esta gestão exige online-mode=true para confirmar as contas Minecraft");
            }
            if players::reached(&current, operation, name) {
                return Ok(current);
            }
            let input = rt.input.as_mut().context("Consola indisponível")?;
            writeln!(input, "{command}")?;
            input.flush()?;
            rt.log(format!("> {command}"));
            after = rt.seq;
            rt.players_busy = true;
        }
        let result = (|| {
            for _ in 0..100 {
                thread::sleep(Duration::from_millis(100));
                let rt = runtime
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
                if rt.state != "online" {
                    bail!("O servidor deixou de estar online. Atualiza a lista para confirmar o resultado.");
                }
                // The server's persisted data, not command submission, confirms success.
                if let Ok(current) = players::access(&directory) {
                    if players::reached(&current, operation, name) {
                        return Ok(current);
                    }
                }
                if rt
                    .logs
                    .iter()
                    .any(|l| l.seq > after && l.text.contains("That player does not exist"))
                {
                    bail!("O Minecraft não encontrou essa conta. Confirma o nome Java e a ligação à internet.");
                }
            }
            bail!("O Minecraft ainda não confirmou a alteração. Atualiza a lista e consulta a consola antes de repetir.")
        })();
        runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .players_busy = false;
        result
    }

    pub fn online_players(&self, id: &str) -> Result<Vec<String>> {
        let runtime = self.runtime(id)?;
        let after;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state != "online" {
                bail!("A lista de ligados está disponível com o servidor online");
            }
            if rt.players_busy {
                bail!("Aguarda a operação de jogadores em curso");
            }
            let input = rt.input.as_mut().context("Consola indisponível")?;
            let command = if Config::load(&self.entry(id)?.directory)?.software
                == crate::config::Software::Fabric
            {
                "list"
            } else {
                "minecraft:list"
            };
            writeln!(input, "{command}")?;
            input.flush()?;
            after = rt.seq;
            rt.players_busy = true;
        }
        let result = (|| {
            for _ in 0..50 {
                thread::sleep(Duration::from_millis(100));
                let rt = runtime
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
                if rt.state != "online" {
                    bail!("O servidor deixou de estar online");
                }
                if let Some(names) = rt
                    .logs
                    .iter()
                    .filter(|l| l.seq > after)
                    .find_map(|l| players::online_response(&l.text))
                {
                    return Ok(names);
                }
            }
            bail!("Não foi possível interpretar a resposta do Minecraft. Consulta a consola e tenta atualizar.")
        })();
        runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .players_busy = false;
        result
    }
}
