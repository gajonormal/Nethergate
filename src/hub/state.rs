//! Stable server lifecycle values shared by runtime state and API serialization.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    Offline,
    Starting,
    Online,
    Stopping,
    Installing,
    BackingUp,
    Restoring,
    ManagingPlugins,
    ExportingModpack,
    ExportingServer,
    Migrating,
    CheckingMigration,
    Error,
    RestartWaiting,
}
impl ServerState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Starting => "starting",
            Self::Online => "online",
            Self::Stopping => "stopping",
            Self::Installing => "installing",
            Self::BackingUp => "backing_up",
            Self::Restoring => "restoring",
            Self::ManagingPlugins => "managing_plugins",
            Self::ExportingServer => "exporting_server",
            Self::ExportingModpack => "exporting_modpack",
            Self::Migrating => "migrating",
            Self::CheckingMigration => "checking_migration",
            Self::Error => "error",
            Self::RestartWaiting => "restart_waiting",
        }
    }
}
impl PartialEq<&str> for ServerState {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
impl std::str::FromStr for ServerState {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "offline" => Ok(Self::Offline),
            "starting" => Ok(Self::Starting),
            "online" => Ok(Self::Online),
            "stopping" => Ok(Self::Stopping),
            "installing" => Ok(Self::Installing),
            "backing_up" => Ok(Self::BackingUp),
            "restoring" => Ok(Self::Restoring),
            "managing_plugins" => Ok(Self::ManagingPlugins),
            "exporting_server" => Ok(Self::ExportingServer),
            "exporting_modpack" => Ok(Self::ExportingModpack),
            "migrating" => Ok(Self::Migrating),
            "checking_migration" => Ok(Self::CheckingMigration),
            "error" => Ok(Self::Error),
            "restart_waiting" => Ok(Self::RestartWaiting),
            _ => anyhow::bail!("Unknown server state: {value}"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_values_roundtrip_and_unknown_states_are_rejected() {
        assert_eq!(
            serde_json::to_value(ServerState::Offline).unwrap(),
            "offline"
        );
        assert_eq!(
            serde_json::to_value(ServerState::Starting).unwrap(),
            "starting"
        );
        assert_eq!(serde_json::to_value(ServerState::Online).unwrap(), "online");
        assert_eq!(
            serde_json::to_value(ServerState::Stopping).unwrap(),
            "stopping"
        );
        assert_eq!(
            serde_json::to_value(ServerState::Installing).unwrap(),
            "installing"
        );
        assert_eq!(
            serde_json::to_value(ServerState::BackingUp).unwrap(),
            "backing_up"
        );
        assert_eq!(
            serde_json::to_value(ServerState::Restoring).unwrap(),
            "restoring"
        );
        assert_eq!(
            serde_json::to_value(ServerState::ManagingPlugins).unwrap(),
            "managing_plugins"
        );
        assert_eq!(
            serde_json::to_value(ServerState::ExportingModpack).unwrap(),
            "exporting_modpack"
        );
        assert_eq!(
            serde_json::to_value(ServerState::Migrating).unwrap(),
            "migrating"
        );
        assert_eq!(
            serde_json::to_value(ServerState::CheckingMigration).unwrap(),
            "checking_migration"
        );
        assert_eq!(serde_json::to_value(ServerState::Error).unwrap(), "error");
        assert_eq!(
            serde_json::to_value(ServerState::RestartWaiting).unwrap(),
            "restart_waiting"
        );

        assert!("typo".parse::<ServerState>().is_err());
        assert!(serde_json::from_str::<ServerState>("\"typo\"").is_err());
    }
}
