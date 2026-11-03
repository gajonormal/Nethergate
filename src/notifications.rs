//! Local event detection. No network or OS delivery; adapters decide how to present events.
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub state: String,
    pub latest: u64,
    pub joins: Vec<(u64, String)>,
}
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub root: PathBuf,
    pub servers: Vec<Server>,
    pub on_battery: bool,
}
pub struct Event {
    pub title: String,
    pub body: String,
}
#[derive(Default)]
pub struct Monitor {
    root: Option<PathBuf>,
    previous: HashMap<String, (String, u64)>,
    battery: Option<bool>,
}
pub fn joined(text: &str) -> Option<String> {
    let (_, message) = text.rsplit_once("]: ")?;
    let name = message.strip_suffix(" joined the game")?;
    crate::players::valid_name(name).then(|| name.to_string())
}
impl Monitor {
    pub fn update(&mut self, snapshot: Snapshot) -> Vec<Event> {
        if self.root.as_ref() != Some(&snapshot.root) {
            *self = Self {
                root: Some(snapshot.root.clone()),
                ..Self::default()
            };
        }
        let mut events = Vec::new();
        let mut next = HashMap::new();
        let mut active = false;
        for server in snapshot.servers {
            active |= matches!(server.state.as_str(), "starting" | "online" | "stopping");
            if let Some((state, seq)) = self.previous.get(&server.id) {
                let status = if state != &server.state {
                    match server.state.as_str() {
                        "online" => Some("O servidor está pronto para jogar."),
                        "offline" => Some("O servidor está parado."),
                        "restart_waiting"=>Some("O Java terminou com erro. Está agendada uma nova tentativa de arranque."),
                        "error" => Some("O servidor precisa de atenção. Consulta a consola."),
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(body) = status {
                    events.push(Event {
                        title: server.name.clone(),
                        body: body.into(),
                    });
                }
                for (_, player) in server.joins.iter().filter(|(at, _)| at > seq) {
                    events.push(Event {
                        title: server.name.clone(),
                        body: format!("{player} entrou no servidor."),
                    });
                }
            }
            next.insert(server.id, (server.state, server.latest));
        }
        if self.battery == Some(false) && snapshot.on_battery && active {
            events.push(Event {
                title: "Nethergate · Bateria".into(),
                body: "O computador está a usar bateria com um servidor ativo. Liga o carregador."
                    .into(),
            });
        }
        self.previous = next;
        self.battery = Some(snapshot.on_battery);
        events
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    fn snapshot(state: &str, seq: u64, battery: bool) -> Snapshot {
        Snapshot {
            root: Path::new("library").into(),
            on_battery: battery,
            servers: vec![Server {
                id: "one".into(),
                name: "Mundo".into(),
                state: state.into(),
                latest: seq,
                joins: vec![(seq, "Player".into())],
            }],
        }
    }
    #[test]
    fn suppresses_history_and_duplicates_but_reports_ready_join_and_battery() {
        let mut monitor = Monitor::default();
        assert!(monitor.update(snapshot("starting", 1, false)).is_empty());
        let events = monitor.update(snapshot("online", 2, true));
        assert_eq!(events.len(), 3);
        assert!(monitor.update(snapshot("online", 2, true)).is_empty());
        let mut other = snapshot("online", 2, true);
        other.root = "other-library".into();
        assert!(monitor.update(other).is_empty());
    }
    #[test]
    fn parses_actual_player_join_lines_without_matching_chat_messages() {
        assert_eq!(
            joined("[10:00:00 INFO]: PlayerOne joined the game").as_deref(),
            Some("PlayerOne")
        );
        assert!(joined("[10:00:00 INFO]: <Player> PlayerOne joined the game").is_none());
        assert!(joined("[10:00:00 INFO]: token-secret joined the game").is_none());
    }
}
