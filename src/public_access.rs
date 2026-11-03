use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub fn run(
    root: &Path,
    directory: &Path,
    id: &str,
    operation: &str,
    expected: Option<&Value>,
) -> Result<Value> {
    if !cfg!(windows) {
        return Ok(
            json!({"state":"unsupported","activeCount":0,"rules":[],"routerState":"not_queried"}),
        );
    }
    if !["inspect", "enable", "disable"].contains(&operation) {
        bail!("Operação de acesso público inválida");
    }
    let cfg = crate::config::Config::load(directory)?;
    let ip = crate::connection::bind_ip(directory).ok();
    let adapter = crate::connection::adapters()
        .unwrap_or_default()
        .into_iter()
        .find(|a| Some(std::net::IpAddr::V4(a.ip)) == ip);
    let props = fs::read_to_string(directory.join("server.properties")).unwrap_or_default();
    let auth =
        crate::connection::property(&props, "online-mode", "true").unwrap_or_default() == "true";
    let whitelist =
        crate::connection::property(&props, "white-list", "false").unwrap_or_default() == "true";
    let mut plan = json!({"id":id,"serverDirectory":crate::firewall::ps_path(directory),"port":cfg.port,"java":crate::firewall::ps_path(&cfg.java),"address":ip.map(|i|i.to_string()).unwrap_or_default(),"interface":adapter.map(|a|a.name),"authenticated":auth,"whitelist":whitelist});
    if operation != "inspect" {
        let expected = expected.context("Verifica a permissão pública antes de confirmar")?;
        plan["expectedRules"] = expected
            .get("rulesSignature")
            .cloned()
            .context("Falta a confirmação das regras")?;
        plan["expectedPlan"] = expected
            .get("planSignature")
            .cloned()
            .unwrap_or(Value::Null);
    }
    crate::firewall::execute(
        root,
        include_str!("../scripts/PublicFirewall.ps1"),
        &plan,
        match operation {
            "enable" => "Enable",
            "disable" => "Disable",
            _ => "Inspect",
        },
        operation != "inspect",
    )
}

pub fn require_disabled(report: &Value) -> Result<()> {
    let count = report
        .get("activeCount")
        .and_then(Value::as_u64)
        .context("Não foi possível confirmar as permissões públicas")?;
    if count > 0 {
        bail!("Desativa primeiro a permissão pública em Ligação → Gerir acesso público. Depois podes alterar o IP, porta ou Java.");
    }
    Ok(())
}

pub fn guard_change(root: &Path, directory: &Path, id: &str) -> Result<()> {
    require_disabled(&run(root, directory, id, "inspect", None)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_or_unknown_permissions_never_allow_a_network_change() {
        assert!(require_disabled(&json!({"activeCount":1})).is_err());
        assert!(require_disabled(&json!({"activeCount":0})).is_ok());
        assert!(require_disabled(&json!({"state":"unknown"})).is_err());
        assert!(require_disabled(&json!({"activeCount":"0"})).is_err());
    }
}
