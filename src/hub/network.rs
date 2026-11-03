//! Connection, firewall, router and sharing coordination.
use super::*;

impl Hub {
    pub(super) fn internet_operation(
        &self,
        operation: impl FnOnce(&mut crate::internet::Internet) -> Result<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut internet = self
            .internet
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Aguarda o diagnóstico de internet em curso"))?;
        let before = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .revision();
        internet.observe_network(before);
        let result = operation(&mut internet);
        let after = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .revision();
        if before != after {
            internet.observe_network(after);
            bail!("A rede mudou durante o diagnóstico. Atualiza o endereço e verifica novamente");
        }
        result
    }

    pub fn connection(&self, id: &str, diagnose: bool) -> Result<serde_json::Value> {
        let revision = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .revision();
        let entry = self.entry(id)?;
        let cfg = Config::load(&entry.directory)?;
        let ip = connection::bind_ip(&entry.directory)?;
        let (adapters, discovery_error) = match connection::adapters() {
            Ok(adapters) => (adapters, None),
            Err(error) => (vec![], Some(error.to_string())),
        };
        let (state, managed) = {
            let runtime = self.runtime(id)?;
            let rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            (rt.state, rt.child.is_some())
        };
        let target = connection::endpoint(ip, cfg.port);
        // Never turn a server.properties value into an arbitrary network probe.
        let can_test = target.ip().is_loopback()
            || adapters
                .iter()
                .any(|a| std::net::IpAddr::V4(a.ip) == target.ip());
        let local_probe = if diagnose && can_test {
            Some(connection::probe(target))
        } else {
            None
        };
        let port_state = if diagnose && can_test && !managed {
            connection::available(target)
        } else {
            "not_tested"
        };
        let lan: Vec<_> = adapters.iter().enumerate().map(|(index, adapter)| {
                let listening = (ip.is_unspecified() && ip.is_ipv4()) || ip == std::net::IpAddr::V4(adapter.ip);
            let endpoint = std::net::SocketAddr::new(adapter.ip.into(), cfg.port);
            let probe = if diagnose && listening && index < 4 && endpoint != target { Some(connection::probe(endpoint)) } else { None };
            serde_json::json!({"name": adapter.name, "ip": adapter.ip, "gateway":adapter.gateway, "address": endpoint.to_string(), "configured": listening, "probe": probe})
        }).collect();
        let properties = fs::read_to_string(entry.directory.join("server.properties"))?;
        let authenticated: bool = connection::property(&properties, "online-mode", "true")?
            .parse()
            .context("online-mode inválido")?;
        let whitelist_enabled: bool = connection::property(&properties, "white-list", "false")?
            .parse()
            .context("white-list inválido")?;
        let monitor = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        if diagnose && revision != monitor.revision() {
            bail!("A rede mudou durante o diagnóstico local. Testa novamente");
        }
        let network = monitor.view(ip, &adapters, discovery_error.as_deref());
        Ok(serde_json::json!({
            "scope": connection::scope(ip), "bindAddress": ip.to_string(), "port": cfg.port,
            "localAddress": connection::local_address(&entry.directory, cfg.port),
            "managedProcess": managed, "state": state, "adapters": lan,
            "discoveryError": discovery_error, "probe": local_probe, "portState": port_state,
            "authenticated": authenticated, "whitelistEnabled": whitelist_enabled,
            "internet": "not_tested", "externalLan": "not_tested",
            "network":network,
            "testedAt": if diagnose { Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis()) } else { None }
        }))
    }

    pub fn firewall(
        &self,
        id: &str,
        repair: bool,
        expected: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        let guard;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.firewall_busy || !matches!(rt.state.as_str(), "offline" | "error" | "online") {
                bail!("Aguarda a operação em curso antes de verificar a firewall");
            }
            guard = if rt.guard.is_none() {
                Some(config::lock(&entry.directory)?)
            } else {
                None
            };
            rt.firewall_busy = true;
        }
        let result = (|| {
            if repair {
                let preview = firewall::run(&self.root, &entry.directory, id, false)?;
                let expected =
                    expected.context("Verifica a regra antes de confirmar a reparação")?;
                for key in ["address", "port", "interface", "program", "ruleName"] {
                    if preview.get(key).is_none() || preview.get(key) != expected.get(key) {
                        bail!("O plano de firewall mudou. Verifica novamente e confirma os novos detalhes.");
                    }
                }
            }
            let report = firewall::run(&self.root, &entry.directory, id, repair)?;
            if repair && report.get("applied").and_then(|v| v.as_bool()) == Some(true) {
                let verified = firewall::run(&self.root, &entry.directory, id, false)?;
                if verified.get("state").and_then(|v| v.as_str()) != Some("aligned") {
                    bail!("A alteração foi pedida, mas a regra final não está alinhada. Verifica o diagnóstico antes de repetir.");
                }
                return Ok(verified);
            }
            Ok(report)
        })();
        drop(guard);
        runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .firewall_busy = false;
        result
    }

    pub fn public_access(
        &self,
        id: &str,
        operation: &str,
        expected: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        let guard;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.firewall_busy || !matches!(rt.state.as_str(), "offline" | "error" | "online") {
                bail!("Aguarda a operação em curso antes de gerir o acesso público");
            }
            guard = if rt.guard.is_none() {
                Some(config::lock(&entry.directory)?)
            } else {
                None
            };
            rt.firewall_busy = true;
        }
        let result = (|| {
            let report =
                crate::public_access::run(&self.root, &entry.directory, id, operation, expected)?;
            if operation == "inspect" {
                return Ok(report);
            }
            let verified =
                crate::public_access::run(&self.root, &entry.directory, id, "inspect", None)?;
            match operation {
                "disable" => crate::public_access::require_disabled(&verified)?,
                "enable" if verified.get("aligned").and_then(|v| v.as_bool()) != Some(true) => {
                    bail!(
                        "A permissão final não está alinhada. Verifica novamente antes de repetir."
                    );
                }
                _ => {}
            }
            Ok(verified)
        })();
        drop(guard);
        runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .firewall_busy = false;
        result
    }

    pub fn set_connection(&self, id: &str, mode: &str, address: &str) -> Result<()> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        let rt = runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        if rt.firewall_busy
            || !matches!(rt.state.as_str(), "offline" | "error")
            || rt.child.is_some()
        {
            bail!("Para o servidor e aguarda a gravação antes de alterar a ligação");
        }
        let _guard = config::lock(&entry.directory)?;
        let desired = if mode == "local" {
            "127.0.0.1"
        } else {
            address
        };
        if connection::bind_ip(&entry.directory)?.to_string() != desired {
            self.router
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .guard_change(id, crate::backup_schedule::now()?)?;
            crate::public_access::guard_change(&self.root, &entry.directory, id)?;
        }
        let adapters = if mode == "lan" {
            connection::adapters()?
        } else {
            vec![]
        };
        connection::save(&entry.directory, mode, address, &adapters)
    }

    pub(super) fn router_context(&self, id: &str) -> Result<crate::router::ContextData> {
        let entry = self.entry(id)?;
        let cfg = Config::load(&entry.directory)?;
        let bind = connection::bind_ip(&entry.directory)?;
        let std::net::IpAddr::V4(local) = bind else {
            bail!("O encaminhamento exige um IPv4 LAN");
        };
        let adapter = connection::adapters()?
            .into_iter()
            .find(|a| a.ip == local)
            .context("Escolhe primeiro o IP LAN atual em Jogar com amigos")?;
        let gateway = adapter
            .gateway
            .context("A interface não tem gateway IPv4")?;
        let access = players::access(&entry.directory)?;
        let secure = access.authenticated && access.enabled && !access.players.is_empty();
        let signature = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(
                &serde_json::json!({"local":local,"gateway":gateway,"port":cfg.port,"java":cfg.java,"authenticated":access.authenticated,"whitelist":access.enabled,"hasPlayers":!access.players.is_empty()})
            )?)
        );
        Ok(crate::router::ContextData {
            local,
            gateway,
            port: cfg.port,
            revision: self
                .network
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .revision(),
            signature,
            online: self
                .runtime(id)?
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .state
                == "online",
            secure,
        })
    }

    pub(super) fn cleanup_router(&self, id: &str) {
        let context = self.router_context(id).ok();
        let result = self
            .router
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(
                id,
                context.as_ref(),
                crate::backup_schedule::now().unwrap_or(0),
            );
        if let Err(error) = result {
            if let Ok(rt) = self.runtime(id) {
                rt.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .log(format!(
                        "[Nethergate] Encaminhamento: limpeza por confirmar: {error:#}"
                    ));
            }
        }
    }

    pub(super) fn sharing_advice(
        &self,
        id: &str,
        port: u16,
        consent: bool,
        wan: &str,
    ) -> Result<serde_json::Value> {
        if port == 0 {
            bail!("Escolhe uma porta externa válida");
        }
        if !wan.trim().is_empty() {
            wan.trim()
                .parse::<std::net::Ipv4Addr>()
                .context("Indica apenas o IPv4 WAN mostrado no router")?;
        }
        let entry = self.entry(id)?;
        let state = self
            .runtime(id)?
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .state;
        let access = players::access(&entry.directory)?;
        let cfg = Config::load(&entry.directory)?;
        let bind = connection::bind_ip(&entry.directory)?;
        let scope = connection::scope(bind);
        let target = connection::endpoint(bind, cfg.port);
        let revision = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .revision();
        let bind_available = scope == "local"
            || (scope == "lan"
                && connection::adapters().is_ok_and(|adapters| {
                    adapters
                        .iter()
                        .any(|adapter| std::net::IpAddr::V4(adapter.ip) == bind)
                }));
        let access_ready = access.authenticated && access.enabled && !access.players.is_empty();
        let mut direct = None;
        let mut wan_hint = "not_provided".to_owned();
        if consent && scope == "lan" && bind_available && state == "online" && access_ready {
            let checked = self.internet_operation(|internet| {
                let ip = internet.lookup(id, true)?;
                wan_hint = internet.compare_wan(id, wan)?["state"]
                    .as_str()
                    .unwrap_or("not_provided")
                    .into();
                let test = internet.test(id, ip["ip"].as_str().unwrap_or(""), port, true)?;
                Ok(test)
            });
            match checked {
                Ok(value) => {
                    direct = Some(value);
                }
                Err(error) => {
                    direct =
                        Some(serde_json::json!({"state":"error","error":format!("{error:#}")}));
                }
            }
        }
        if revision
            != self
                .network
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .revision()
            || connection::endpoint(
                connection::bind_ip(&entry.directory)?,
                Config::load(&entry.directory)?.port,
            ) != target
            || self
                .runtime(id)?
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .state
                != state
        {
            bail!("A rede ou o servidor mudou durante o diagnóstico. Verifica novamente");
        }
        let mut advice = crate::connection_advice::recommend(crate::connection_advice::Facts {
            online: state == "online",
            access_ready,
            scope,
            bind_available,
            direct: direct.as_ref(),
            wan_hint: &wan_hint,
        });
        advice["directTest"] = serde_json::json!(direct);
        advice["wanHint"] = serde_json::json!(wan_hint);
        advice["target"] = serde_json::json!(target.to_string());
        advice["networkRevision"] = serde_json::json!(revision);
        advice["checkedAt"] = serde_json::json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis());
        advice["playitReady"] = serde_json::json!(crate::playit::status(&self.root, id)
            .ok()
            .is_some_and(|v| v["ready"] == true));
        Ok(advice)
    }

    pub(super) fn share(
        &self,
        id: &str,
        port: u16,
        consent: bool,
        temporary: bool,
        recover: bool,
        assisted: Option<&AssistedStart>,
    ) -> Result<serde_json::Value> {
        if !consent {
            bail!("Confirma os serviços externos antes de partilhar");
        }
        if port == 0 {
            bail!("Escolhe uma porta externa válida");
        }
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        if runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .state
            != "online"
        {
            bail!("Inicia o servidor antes de partilhar");
        }
        let network_revision = self
            .network
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .revision();
        let access = players::access(&entry.directory)?;
        if !access.authenticated || !access.enabled || access.players.is_empty() {
            bail!(
                "Ativa a autenticação oficial e adiciona jogadores à whitelist antes de partilhar"
            );
        }
        let cfg = Config::load(&entry.directory)?;
        let bind = connection::bind_ip(&entry.directory)?;
        let target = connection::endpoint(bind, cfg.port);
        if assisted.is_some_and(|plan| plan.revision != network_revision || plan.target != target) {
            bail!(
                "O diagnóstico está desatualizado. Verifica a ligação novamente antes de partilhar"
            );
        }
        if !matches!(connection::scope(bind), "local" | "lan") || !bind.is_ipv4() {
            bail!("Escolhe o modo local ou um IPv4 da rede de casa antes de partilhar");
        }
        if connection::scope(bind) == "lan"
            && !connection::adapters()?
                .iter()
                .any(|adapter| std::net::IpAddr::V4(adapter.ip) == bind)
        {
            bail!("O IP configurado já não pertence a uma interface de rede ativa deste PC");
        }
        let ticket = self
            .sharing
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .begin_check(id)?;
        let result = (|| -> Result<serde_json::Value> {
            let direct = if !temporary && connection::scope(bind) == "lan" {
                self.internet_operation(|internet| {
                    let ip = internet.lookup(id, true)?;
                    internet.test(id, ip["ip"].as_str().unwrap_or(""), port, true)
                })
                .unwrap_or_else(
                    |error| serde_json::json!({"state":"error","error":format!("{error:#}")}),
                )
                .into()
            } else {
                None
            };
            let direct_confirmed = crate::connection_advice::direct_confirmed(direct.as_ref());
            if !temporary && !direct_confirmed && !assisted.is_some_and(|plan| plan.allow_fallback)
            {
                bail!("O acesso direto não foi confirmado. O fallback playit não está autorizado; revê a ligação ou autoriza a alternativa");
            }
            if !self
                .sharing
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .check_matches(id, ticket)
            {
                bail!("A preparação da partilha foi cancelada");
            }
            if network_revision
                != self
                    .network
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                    .revision()
            {
                bail!("A rede mudou durante o teste. Verifica novamente antes de partilhar");
            }
            if !direct_confirmed {
                crate::playit::validate(&self.root, id, connection::endpoint(bind, cfg.port))?;
            }
            // Serialize creation with stop. No network diagnostics while holding the runtime lock.
            let rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state != "online" {
                bail!("O servidor parou durante a verificação; a partilha foi cancelada");
            }
            if network_revision
                != self
                    .network
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                    .revision()
            {
                bail!(
                "A rede mudou durante a preparação da partilha. Revê a ligação antes de partilhar"
            );
            }
            if connection::endpoint(
                connection::bind_ip(&entry.directory)?,
                Config::load(&entry.directory)?.port,
            ) != target
            {
                bail!("O destino local mudou durante a preparação da partilha");
            }
            let access = players::access(&entry.directory)?;
            if !access.authenticated || !access.enabled || access.players.is_empty() {
                bail!("A autenticação e a whitelist têm de permanecer ativas");
            }
            let mut sharing = self
                .sharing
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if !sharing.check_matches(id, ticket) {
                bail!("A preparação da partilha foi cancelada");
            }
            if let Some(result) = direct.as_ref().filter(|r| r["state"] == "reported_online") {
                sharing.direct(id, result.clone());
            } else {
                sharing.tunnel_with_recovery(
                    &self.root,
                    id,
                    connection::endpoint(bind, cfg.port),
                    recover,
                )?;
            }
            sharing.decision(id, direct, !temporary && !direct_confirmed);
            Ok(sharing.status(id))
        })();
        if let Err(error) = &result {
            self.sharing
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .fail_check(id, ticket, &format!("{error:#}"));
        }
        result
    }

    pub(super) fn recover_share(&self, request: crate::sharing::RetryRequest) {
        if !self
            .sharing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retry_matches(&request)
        {
            return;
        }
        let mut retryable = false;
        let prerequisites = (|| -> Result<_> {
            let entry = self.entry(&request.id)?;
            let runtime = self.runtime(&request.id)?;
            if runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .state
                != "online"
            {
                bail!("O servidor deixou de estar pronto. Inicia a partilha novamente quando estiver online");
            }
            let cfg = Config::load(&entry.directory)?;
            let bind = connection::bind_ip(&entry.directory)?;
            if connection::endpoint(bind, cfg.port) != request.target {
                bail!(
                    "O IP ou porta mudou. Revê a configuração playit antes de voltar a partilhar"
                );
            }
            if connection::scope(bind) == "lan" {
                let adapters = match connection::adapters() {
                    Ok(value) => value,
                    Err(error) => {
                        retryable = true;
                        return Err(error);
                    }
                };
                if adapters.is_empty() {
                    retryable = true;
                    bail!("A rede local está temporariamente indisponível");
                }
                if !adapters
                    .iter()
                    .any(|adapter| std::net::IpAddr::V4(adapter.ip) == bind)
                {
                    bail!(
                        "O IP do servidor já não pertence à rede atual. Revê o IP local e o playit"
                    );
                }
            }
            let access = players::access(&entry.directory)?;
            if !access.authenticated || !access.enabled || access.players.is_empty() {
                bail!("A recuperação exige autenticação oficial e whitelist com jogadores");
            }
            Ok((entry, runtime))
        })();
        let (entry, runtime) = match prerequisites {
            Ok(value) => value,
            Err(error) => {
                let mut sharing = self
                    .sharing
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if retryable {
                    sharing.finish_retry(&self.root, &request, Err(error));
                } else {
                    sharing.abort_retry(&request, &format!("Recuperação cancelada: {error:#}"));
                }
                return;
            }
        };
        let revision = self
            .network
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revision();
        // Network I/O never holds a runtime or sharing lock: Stop can cancel immediately.
        let validation = crate::playit::validate(&self.root, &request.id, request.target);
        let rt = runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = (|| -> Result<()> {
            if rt.state != "online" {
                bail!("O servidor parou durante a recuperação");
            }
            let cfg = Config::load(&entry.directory)?;
            if connection::endpoint(connection::bind_ip(&entry.directory)?, cfg.port)
                != request.target
            {
                bail!("O destino local mudou durante a recuperação");
            }
            let access = players::access(&entry.directory)?;
            if !access.authenticated || !access.enabled || access.players.is_empty() {
                bail!("A autenticação ou whitelist mudou durante a recuperação");
            }
            Ok(())
        })();
        let mut sharing = self
            .sharing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(error) = current {
            sharing.abort_retry(&request, &format!("Recuperação cancelada: {error:#}"));
        } else {
            let result = if revision
                != self
                    .network
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .revision()
            {
                Err(anyhow::anyhow!("A rede mudou durante a verificação"))
            } else {
                validation
            };
            sharing.finish_retry(&self.root, &request, result);
        }
    }
}
