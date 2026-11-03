//! Recommendations from observed evidence; no network I/O or configuration changes.
use serde_json::{json, Value};

pub struct Facts<'a> {
    pub online: bool,
    pub access_ready: bool,
    pub scope: &'a str,
    pub bind_available: bool,
    pub direct: Option<&'a Value>,
    pub wan_hint: &'a str,
}
pub fn direct_confirmed(test: Option<&Value>) -> bool {
    test.is_some_and(|value| value["state"] == "reported_online")
}
pub fn recommend(facts: Facts<'_>) -> Value {
    let (method, reason) = if !facts.bind_available {
        ("review", "Não foi possível confirmar o IP de escuta na rede atual. Revê o IP local antes de partilhar.")
    } else if !facts.online {
        (
            "review",
            "Inicia o servidor para verificar e partilhar a ligação.",
        )
    } else if !facts.access_ready {
        (
            "review",
            "Ativa a autenticação oficial e uma whitelist com jogadores antes de partilhar.",
        )
    } else if direct_confirmed(facts.direct) {
        ("direct", "O serviço externo confirmou uma resposta Minecraft no acesso direto. O resultado pode estar em cache; confirma a entrada de um jogador.")
    } else if facts.scope == "local" {
        ("playit", "O Minecraft escuta apenas neste PC. O playit permite partilhar sem alterar o IP de escuta nem configurar o router.")
    } else if facts.wan_hint == "upstream_nat" {
        ("playit", "O WAN indicado é privado ou partilhado e o acesso direto não foi confirmado. Pode existir CGNAT ou outro router a montante; isto não distingue as duas situações.")
    } else if facts.direct.is_some() {
        ("playit", "O acesso direto não foi confirmado. O diagnóstico não identifica sozinho se a causa é router, firewall, operador ou falha do serviço externo.")
    } else {
        ("playit", "O acesso direto ainda não foi testado. Podes autorizar o teste ou usar o playit associado sem configurar o router.")
    };
    json!({"recommendation":method,"reason":reason,"blocked":method=="review"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evidence_wins_over_nat_hints_but_never_over_local_requirements() {
        let test = json!({"state":"reported_online"});
        fn facts(
            online: bool,
            access_ready: bool,
            bind_available: bool,
            direct: Option<&Value>,
        ) -> Facts<'_> {
            Facts {
                online,
                access_ready,
                scope: "lan",
                bind_available,
                direct,
                wan_hint: "upstream_nat",
            }
        }
        assert_eq!(
            recommend(facts(true, true, true, Some(&test)))["recommendation"],
            "direct"
        );
        assert_eq!(
            recommend(facts(false, true, true, Some(&test)))["recommendation"],
            "review"
        );
        assert_eq!(
            recommend(facts(true, false, true, Some(&test)))["recommendation"],
            "review"
        );
        assert_eq!(
            recommend(facts(true, true, false, Some(&test)))["recommendation"],
            "review"
        );
        for state in ["reported_offline", "unconfirmed", "error", "not_tested"] {
            let test = json!({"state":state});
            assert!(!direct_confirmed(Some(&test)));
            assert_eq!(
                recommend(facts(true, true, true, Some(&test)))["recommendation"],
                "playit"
            );
        }
        assert!(!direct_confirmed(None));
    }
}
