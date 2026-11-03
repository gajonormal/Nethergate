//! Local wire actions: decode requests and delegate to the hub domains.
use super::*;

pub fn dispatch(
    hub: &Arc<Hub>,
    action: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value> {
    let id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "list" => Ok(serde_json::to_value(hub.list()?)?),
        "world_preview" => {
            let entry = hub.entry(id)?;
            Ok(serde_json::to_value(hub.previews.request(
                hub.root.join("hub-data/previews"),
                id,
                entry.directory,
                payload.get("revision").and_then(|value| value.as_str()),
            )?)?)
        }
        "router_status" => {
            hub.entry(id)?;
            hub.router
                .lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .status(id, crate::backup_schedule::now()?)
        }
        "router_forget_expired" => {
            hub.entry(id)?;
            hub.router
                .lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .forget_expired(id, crate::backup_schedule::now()?)
        }
        "router_preview" => {
            let context = hub.router_context(id)?;
            let provider = serde_json::from_value(payload["provider"].clone())?;
            let port = payload["externalPort"]
                .as_u64()
                .filter(|p| *p > 0 && *p <= 65535)
                .context("Escolhe uma porta externa válida")? as u16;
            hub.router.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.preview(
                id,
                context,
                provider,
                port,
                crate::backup_schedule::now()?,
            )
        }
        "router_create" => {
            let context = hub.router_context(id)?;
            let result = hub.router.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.create(
                id,
                payload["ticket"]
                    .as_str()
                    .context("Revê o plano do router")?,
                &context,
                crate::backup_schedule::now()?,
            )?;
            let current = hub.router_context(id).ok();
            if !current.as_ref().is_some_and(|c| {
                c.online
                    && c.secure
                    && c.signature == context.signature
                    && c.revision == context.revision
            }) {
                let _ = hub.router.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.remove(
                    id,
                    current.as_ref(),
                    crate::backup_schedule::now()?,
                );
                bail!("A rede ou as permissões mudaram durante a operação; a limpeza foi pedida");
            }
            Ok(result)
        }
        "router_remove" => {
            hub.entry(id)?;
            let context = hub.router_context(id).ok();
            hub.router
                .lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .remove(id, context.as_ref(), crate::backup_schedule::now()?)
        }
        "cloud_account_status" | "cloud_account_begin" | "cloud_account_disconnect" => {
            hub.entry(id)?;
            let provider=crate::cloud::accounts::Provider::parse(payload["provider"].as_str().context("Escolhe um fornecedor")?)?;
            match action {
                "cloud_account_begin"=>crate::cloud::accounts::begin(&hub.root,provider,serde_json::from_value(payload["config"].clone())?),
                "cloud_account_disconnect"=>{crate::cloud::accounts::disconnect(&hub.root,provider)?;crate::cloud::accounts::status(&hub.root,provider)},
                _=>crate::cloud::accounts::status(&hub.root,provider)
            }
        }
        "service_cloud_status" | "service_cloud_save" | "service_cloud_history" | "service_cloud_upload" | "service_cloud_download" => {
            hub.entry(id)?;
            let provider=crate::cloud::accounts::Provider::parse(payload["provider"].as_str().context("Escolhe um fornecedor")?)?;
            match action {
                "service_cloud_status"=>{let mut value=crate::cloud::services::status(&hub.root,id,provider)?;value["busy"]=serde_json::json!(hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.contains(id));Ok(value)},
                "service_cloud_save"=>{let jobs=hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;anyhow::ensure!(!jobs.contains(id),"Aguarda o envio antes de alterar o destino");crate::cloud::services::save(&hub.root,id,provider,serde_json::from_value(payload["settings"].clone())?)},
                "service_cloud_history"=>crate::cloud::services::history(&hub.root,id,provider),
                _=>{let backup=payload["backupId"].as_str().map(str::to_string).or_else(||if action=="service_cloud_upload"{hub.backups(id).ok()?.first().map(|b|b.id.clone())}else{None}).context("Escolhe um backup")?;hub.start_service_cloud_job(id,&backup,provider,action=="service_cloud_download")}
            }
        }
        "cloud_status" => {
            hub.entry(id)?;
            let mut value = crate::cloud::status(&hub.root, id)?;
            value["busy"] = serde_json::json!(hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.contains(id));
            Ok(value)
        }
        "cloud_save" | "cloud_pause" => {
            hub.entry(id)?;
            let jobs = hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            anyhow::ensure!(
                !jobs.contains(id),
                "Aguarda a transferência cloud antes de alterar o destino"
            );
            if action == "cloud_pause" {
                crate::cloud::pause(&hub.root, id)
            } else {
                crate::cloud::save(
                    &hub.root,
                    id,
                    serde_json::from_value(
                        payload
                            .get("settings")
                            .cloned()
                            .context("Preenche as definições S3")?,
                    )?,
                )
            }
        }
        "cloud_history" => {
            hub.entry(id)?;
            crate::cloud::history(&hub.root, id)
        }
        "cloud_retention_preview" | "cloud_retention_apply" => {
            hub.entry(id)?;
            anyhow::ensure!(!hub.cloud_cancel.load(std::sync::atomic::Ordering::SeqCst), "O motor está a terminar");
            let mut jobs = hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            anyhow::ensure!(!jobs.contains(id), "Aguarda a operação cloud em curso");
            let keep = payload["keep"].as_u64().and_then(|v|usize::try_from(v).ok()).context("Escolhe a retenção")?;
            jobs.insert(id.into()); drop(jobs);
            let result = if action == "cloud_retention_preview" { crate::cloud::retention_preview(&hub.root,id,keep) } else {
                (|| { anyhow::ensure!(payload["confirmed"].as_bool()==Some(true), "Confirma a limpeza cloud");
                    crate::cloud::retention_apply(&hub.root,id,keep,payload["fingerprint"].as_str().context("Revê a limpeza")?,&hub.cloud_cancel)
                })()
            };
            hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.remove(id);
            result
        }
        "cloud_upload" | "cloud_download" => {
            let backup = payload["backupId"]
                .as_str()
                .map(str::to_string)
                .or_else(|| {
                    if action == "cloud_upload" {
                        hub.backups(id).ok()?.first().map(|b| b.id.clone())
                    } else {
                        None
                    }
                })
                .context("Escolhe um backup")?;
            hub.start_cloud_job(id, &backup, action == "cloud_download")
        }
        "migration_versions" => Ok(serde_json::to_value(crate::migration::versions(
            &hub.entry(id)?.directory,
        )?)?),
        "migration_preview" => hub.world_operation(
            id,
            ServerState::CheckingMigration,
            "A verificar compatibilidade para migração…",
            |_, directory, _| {
                crate::migration::preview(
                    directory,
                    payload["target"]
                        .as_str()
                        .context("Escolhe a versão de destino")?,
                )
            },
        ),
        "migrate_server" => hub.migrate_server(
            id,
            serde_json::from_value(
                payload
                    .get("input")
                    .cloned()
                    .context("Revê o plano de migração")?,
            )?,
        ),
        "export_server" => hub.export_server(
            id,
            Path::new(
                payload["path"]
                    .as_str()
                    .context("Escolhe a pasta de destino")?,
            ),
        ),
        "preview_import" => crate::transfer::inspect(Path::new(
            payload["path"].as_str().context("Escolhe o pacote")?,
        )),
        "import_server" => {
            Ok(serde_json::json!({"id":hub.import_server(serde_json::from_value(payload)?)?}))
        }
        "metrics" => {
            let runtime = hub.runtime(id)?;
            let pid = runtime
                .lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .child
                .as_ref()
                .map(|child| child.id());
            Ok(serde_json::to_value(
                hub.metrics.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.measure(id, pid),
            )?)
        }
        "recovery" => {
            hub.entry(id)?;
            let policy = crate::recovery::load(&hub.root, id)?;
            let state = hub.runtime(id)?.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.recovery.view();
            Ok(serde_json::json!({"enabled":policy.enabled,"state":state}))
        }
        "save_recovery" => {
            hub.entry(id)?;
            let enabled = payload
                .get("enabled")
                .and_then(|v| v.as_bool())
                .context("Indica se pretendes recuperação automática")?;
            let runtime = hub.runtime(id)?;
            let mut rt = runtime.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            crate::recovery::save(&hub.root, id, enabled)?;
            if !enabled {
                rt.recovery.cancel();
                if rt.state == "restart_waiting" {
                    rt.state = ServerState::Offline;
                }
            }
            Ok(serde_json::json!({"enabled":enabled,"state":rt.recovery.view()}))
        }
        "energy" => Ok(hub.energy()),
        "connection" => hub.connection(id, false),
        "playit_status" => {
            hub.entry(id)?;
            crate::playit::status(&hub.root, id)
        }
        "playit_prepare" | "playit_confirm" => {
            let entry = hub.entry(id)?;
            if matches!(
                hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id)["state"].as_str(),
                Some("checking" | "connecting" | "tunnel" | "recovering")
            ) {
                bail!("Termina primeiro a partilha antes de configurar o playit");
            }
            let access = players::access(&entry.directory)?;
            if !access.authenticated || !access.enabled || access.players.is_empty() {
                bail!("Ativa a autenticação oficial e adiciona jogadores à whitelist antes de associar o playit");
            }
            let _guard = hub
                .playit_setup
                .try_lock()
                .map_err(|_| anyhow::anyhow!("Aguarda a configuração playit em curso"))?;
            if payload.get("consent").and_then(|v| v.as_bool()) != Some(true) {
                bail!("Confirma a associação playit");
            }
            let cfg = Config::load(&entry.directory)?;
            let bind = connection::bind_ip(&entry.directory)?;
            if !matches!(connection::scope(bind), "local" | "lan") || !bind.is_ipv4() {
                bail!("Escolhe primeiro o IP local ou da rede de casa");
            }
            if action == "playit_prepare" {
                crate::playit::prepare(&hub.root, id)
            } else {
                crate::playit::confirm(&hub.root, id, connection::endpoint(bind, cfg.port))
            }
        }
        "share_status" => {
            hub.entry(id)?;
            Ok(hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id))
        }
        "share_test" => {
            hub.entry(id)?;
            let network_revision = hub.network.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.revision();
            if hub.runtime(id)?.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.state != "online" {
                bail!("Inicia o servidor antes de testar a partilha");
            }
            let before = hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id);
            if !matches!(before["state"].as_str(), Some("tunnel" | "direct")) {
                bail!("Inicia a partilha e aguarda a ligação antes de testar");
            }
            let address = before["address"]
                .as_str()
                .context("Falta o endereço da partilha")?;
            let result = hub.internet_operation(|internet| {
                internet.test_shared(
                    id,
                    address,
                    payload.get("consent").and_then(|v| v.as_bool()) == Some(true),
                )
            })?;
            let after = hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id);
            if before["state"] != after["state"]
                || before["address"] != after["address"]
                || before["startedAt"] != after["startedAt"]
                || network_revision != hub.network.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.revision()
                || hub.runtime(id)?.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.state != "online"
            {
                bail!("A partilha mudou durante o teste. Verifica novamente");
            }
            Ok(result)
        }
        "share_diagnose" => hub.sharing_advice(
            id,
            u16::try_from(
                payload
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .context("Indica a porta externa")?,
            )?,
            payload.get("consent").and_then(|v| v.as_bool()) == Some(true),
            payload.get("wan").and_then(|v| v.as_str()).unwrap_or(""),
        ),
        "share_start" | "share_assisted_start" => {
            let assisted = if action == "share_assisted_start" {
                Some(AssistedStart {
                    revision: payload
                        .get("expectedNetworkRevision")
                        .and_then(|v| v.as_u64())
                        .context("Executa primeiro o diagnóstico")?,
                    target: payload
                        .get("expectedTarget")
                        .and_then(|v| v.as_str())
                        .context("Falta o destino do diagnóstico")?
                        .parse()
                        .context("Destino do diagnóstico inválido")?,
                    allow_fallback: payload.get("allowFallback").and_then(|v| v.as_bool())
                        == Some(true),
                })
            } else {
                None
            };
            hub.share(
                id,
                u16::try_from(
                    payload
                        .get("port")
                        .and_then(|v| v.as_u64())
                        .context("Indica a porta externa")?,
                )?,
                payload
                    .get("consent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                action != "share_assisted_start"
                    && payload
                        .get("temporary")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                payload
                    .get("recover")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                assisted.as_ref(),
            )
        }
        "share_cancel_check" => {
            hub.entry(id)?;
            let operation = payload
                .get("expectedOperation")
                .and_then(|v| v.as_u64())
                .context("Aguarda o início da preparação antes de cancelar")?;
            let mut sharing = hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            let cancelled = sharing.check_matches(id, operation);
            if cancelled {
                sharing.stop(id);
            }
            let mut view = sharing.status(id);
            view["cancelled"] = serde_json::json!(cancelled);
            Ok(view)
        }
        "share_stop" => {
            hub.entry(id)?;
            // A direct connection ends only when the Minecraft listener is stopped.
            let direct = hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id)["state"] == "direct";
            if direct {
                hub.stop(id)?;
            }
            hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.stop(id);
            Ok(hub.sharing.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(id))
        }
        "internet_ip" => {
            hub.entry(id)?;
            hub.internet_operation(|internet| {
                internet.lookup(
                    id,
                    payload
                        .get("consent")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            })
        }
        "internet_test" => {
            hub.entry(id)?;
            let port = u16::try_from(
                payload
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .context("Indica a porta externa")?,
            )?;
            hub.internet_operation(|internet| {
                internet.test(
                    id,
                    payload
                        .get("expectedIp")
                        .and_then(|v| v.as_str())
                        .context("Consulta primeiro o IP público")?,
                    port,
                    payload
                        .get("consent")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            })
        }
        "internet_wan" => {
            hub.entry(id)?;
            hub.internet_operation(|internet| {
                internet.compare_wan(
                    id,
                    payload
                        .get("wan")
                        .and_then(|v| v.as_str())
                        .context("Indica o IPv4 WAN")?,
                )
            })
        }
        "inspect_firewall" => hub.firewall(id, false, None),
        "public_access" => hub.public_access(id, "inspect", None),
        "set_public_access" => hub.public_access(
            id,
            payload
                .get("operation")
                .and_then(|v| v.as_str())
                .context("Falta a operação de acesso público")?,
            payload.get("expected"),
        ),
        "repair_firewall" => hub.firewall(id, true, payload.get("expected")),
        "diagnose_connection" => hub.connection(id, true),
        "set_connection" => {
            hub.set_connection(
                id,
                payload
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .context("Escolhe um modo de ligação")?,
                payload
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )?;
            Ok(serde_json::json!({"ok": true}))
        }
        "create" => Ok(serde_json::json!({"id": hub.create(serde_json::from_value(payload)?)?})),
        "install" => {
            hub.install(id)?;
            Ok(serde_json::json!({"ok": true}))
        }
        "java_status" => hub.java_status(id),
        "ddns_status" => {
            hub.entry(id)?;
            hub.ddns.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(&hub.root, id)
        }
        "ddns_save" => {
            hub.entry(id)?;
            let settings = serde_json::from_value(
                payload
                    .get("settings")
                    .cloned()
                    .context("Falta a configuração DDNS")?,
            )?;
            hub.ddns.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.save(&hub.root, id, settings)
        }
        "ddns_test" => {
            hub.entry(id)?;
            let network_revision = hub.network.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.revision();
            if payload.get("consent").and_then(|v| v.as_bool()) != Some(true) {
                bail!("Confirma o envio do nome DuckDNS ao serviço mcsrvstat.us");
            }
            if hub.runtime(id)?.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.state != "online" {
                bail!("Inicia o servidor antes de testar o nome DuckDNS");
            }
            let before = hub.ddns.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(&hub.root, id)?;
            let hostname = before["hostname"]
                .as_str()
                .context("Configura primeiro o nome DuckDNS")?;
            if payload.get("expectedHostname").and_then(|v| v.as_str()) != Some(hostname) {
                bail!("O nome DuckDNS mudou. Atualiza o painel antes de testar");
            }
            let port = before["externalPort"]
                .as_u64()
                .context("Falta a porta externa")?;
            // An explicit port tests the router mapping rather than an unrelated SRV record.
            let address = format!("{hostname}:{port}");
            let result = hub.internet_operation(|internet| {
                internet.observe_publication(id, before["lastPublishedIp"].as_str().unwrap_or(""));
                internet.test_shared(id, &address, true)
            })?;
            let after = hub.ddns.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.status(&hub.root, id)?;
            if before["hostname"] != after["hostname"]
                || before["externalPort"] != after["externalPort"]
                || before["lastPublishedIp"] != after["lastPublishedIp"]
                || network_revision != hub.network.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.revision()
                || hub.runtime(id)?.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.state != "online"
            {
                bail!("O servidor ou o destino DuckDNS mudou durante o teste. Verifica novamente");
            }
            Ok(result)
        }
        "ddns_pause" => {
            hub.entry(id)?;
            hub.ddns.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.pause(&hub.root, id)
        }
        "ddns_update" => {
            hub.entry(id)?;
            let hostname = payload
                .get("expectedHostname")
                .and_then(|v| v.as_str())
                .context("Confirma o nome que queres publicar")?;
            hub.ddns
                .lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
                .update(&hub.root, id, Some(hostname))
        }
        "prepare_java" => {
            hub.prepare_java(id)?;
            Ok(serde_json::json!({"ok":true}))
        }
        "start" => {
            hub.start(id)?;
            Ok(serde_json::json!({"ok": true}))
        }
        "stop" => {
            hub.stop(id)?;
            Ok(serde_json::json!({"ok": true}))
        }
        "command" => {
            hub.command(
                id,
                payload
                    .get("command")
                    .and_then(|v| v.as_str())
                    .context("Falta o comando")?,
            )?;
            Ok(serde_json::json!({"ok": true}))
        }
        "logs" => Ok(serde_json::to_value(hub.logs(
            id,
            payload.get("after").and_then(|v| v.as_u64()).unwrap_or(0),
        )?)?),
        "backup_destination" => {
            hub.entry(id)?;
            crate::backup_destination::status(&hub.root, id)
        }
        "save_backup_destination" => {
            let entry = hub.entry(id)?;
            anyhow::ensure!(
                !hub.cloud_jobs.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.contains(id),
                "Aguarda a transferência cloud antes de mudar o destino local"
            );
            let runtime = hub.runtime(id)?;
            let rt = runtime.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            anyhow::ensure!(
                matches!(rt.state.as_str(), "offline" | "error")
                    && rt.child.is_none()
                    && !rt.firewall_busy,
                "Para o servidor e aguarda as operações antes de mudar o destino"
            );
            let _guard = config::lock(&entry.directory)?;
            let _policy = hub.backup_policy.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            let base = match payload.get("path") {
                Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(s)) => Some(Path::new(s)),
                _ => bail!("Indica uma pasta ou null para armazenamento local"),
            };
            crate::backup_destination::save(&hub.root, id, base, &entry.directory)?;
            crate::backup_destination::status(&hub.root, id)
        }
        "backups" => Ok(serde_json::to_value(hub.backups(id)?)?),
        "plugins" => crate::plugins::inventory(&hub.entry(id)?.directory),
        "manual_jars" => crate::plugins::manual::list(&hub.entry(id)?.directory),
        "manual_jar_file_preview" => crate::plugins::manual::file_preview(Path::new(
            payload["path"].as_str().context("Escolhe o JAR")?,
        )),
        "manual_jar_preview" => Ok(serde_json::to_value(crate::plugins::manual::preview(
            &hub.entry(id)?.directory,
            payload["filename"].as_str().context("Escolhe o JAR")?,
            payload["consent"].as_bool().unwrap_or(false),
        )?)?),
        "manual_jar_add" | "manual_jar_adopt" | "manual_jar_disable" | "manual_jar_enable" => hub
            .world_operation(
                id,
                ServerState::ManagingPlugins,
                "A verificar e gerir o JAR manual…",
                |_, directory, _| match action {
                    "manual_jar_add" => crate::plugins::manual::add(
                        directory,
                        Path::new(payload["path"].as_str().context("Escolhe o JAR")?),
                        payload["sha512"].as_str().context("Revê o ficheiro")?,
                    ),
                    "manual_jar_adopt" => crate::plugins::manual::adopt(
                        directory,
                        payload["filename"].as_str().context("Escolhe o JAR")?,
                        payload["fingerprint"].as_str().context("Revê o JAR")?,
                        payload["consent"].as_bool().unwrap_or(false),
                    ),
                    "manual_jar_disable" => crate::plugins::manual::disable(
                        directory,
                        payload["filename"].as_str().context("Escolhe o JAR")?,
                        payload["sha512"].as_str().context("Revê o JAR")?,
                    ),
                    _ => crate::plugins::manual::enable(
                        directory,
                        payload["disabledId"]
                            .as_str()
                            .context("Escolhe o JAR desativado")?,
                    ),
                },
            ),
        "addon_set_preview" => Ok(serde_json::to_value(crate::plugins::solver::preview(
            &hub.entry(id)?.directory,
            &serde_json::from_value::<Vec<String>>(
                payload
                    .get("projects")
                    .cloned()
                    .unwrap_or(serde_json::json!([])),
            )?,
        )?)?),
        "addon_set_install" => hub.install_addon_set(
            id,
            &serde_json::from_value::<Vec<String>>(
                payload
                    .get("projects")
                    .cloned()
                    .context("Indica os projetos revistos")?,
            )?,
            payload["fingerprint"]
                .as_str()
                .context("Revê o plano antes de instalar")?,
        ),
        "export_modpack" => hub.export_modpack(id),
        "plugin_search" => crate::plugins::search(
            &hub.entry(id)?.directory,
            payload.get("query").and_then(|v| v.as_str()).unwrap_or(""),
        ),
        "plugin_preview" => Ok(serde_json::to_value(crate::plugins::preview(
            &hub.entry(id)?.directory,
            payload
                .get("projectId")
                .and_then(|v| v.as_str())
                .context("Escolhe um plugin")?,
            None,
        )?)?),
        "plugin_install" => hub.install_plugin(
            id,
            payload
                .get("projectId")
                .and_then(|v| v.as_str())
                .context("Escolhe um plugin")?,
            payload
                .get("versionId")
                .and_then(|v| v.as_str())
                .context("Escolhe uma versão")?,
            payload
                .get("fingerprint")
                .and_then(|v| v.as_str())
                .context("Revê o plano antes de instalar")?,
        ),
        "backup_schedule" => hub.backup_schedule(id),
        "save_backup_schedule" => {
            hub.save_backup_schedule(
                id,
                serde_json::from_value(
                    payload
                        .get("policy")
                        .cloned()
                        .context("Falta a política de backups")?,
                )?,
            )?;
            hub.backup_schedule(id)
        }
        "create_backup" => Ok(serde_json::to_value(hub.create_backup(id)?)?),
        "restore_recovery_pending" => Ok(serde_json::json!(backups::recovery_pending(&hub.entry(id)?.directory)?)),
        "restore_recovery_plan" => hub.world_operation(id, ServerState::Restoring, "A verificar a cópia de recuperação…", |store, directory, server_id| Ok(serde_json::to_value(backups::recovery_plan(store, server_id, directory)?)?)),
        "recover_restore" => hub.world_operation(id, ServerState::Restoring, "A recuperar o estado anterior ao restauro…", |store, directory, server_id| {
            anyhow::ensure!(payload["confirmed"].as_bool() == Some(true), "Confirma a recuperação");
            Ok(serde_json::json!({"message": backups::recover(store, server_id, directory, payload["fingerprint"].as_str().context("Revê o plano de recuperação")?)?}))
        }),
        "restore_backup" => Ok(serde_json::to_value(
            hub.restore_backup(
                id,
                payload
                    .get("backupId")
                    .and_then(|v| v.as_str())
                    .context("Escolhe um backup")?,
            )?,
        )?),
        "players" => Ok(serde_json::to_value(hub.player_access(id)?)?),
        "online_players" => Ok(serde_json::to_value(hub.online_players(id)?)?),
        "player_operation" => Ok(serde_json::to_value(
            hub.player_operation(
                id,
                payload
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .context("Falta a operação")?,
                payload.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            )?,
        )?),
        "gameplay" => Ok(serde_json::to_value(hub.gameplay(id, None)?)?),
        "save_gameplay" => Ok(serde_json::to_value(
            hub.gameplay(
                id,
                Some(serde_json::from_value(
                    payload
                        .get("settings")
                        .cloned()
                        .context("Faltam as definições de jogo")?,
                )?),
            )?,
        )?),
        "settings" => {
            hub.settings(
                id,
                serde_json::from_value(
                    payload
                        .get("settings")
                        .cloned()
                        .context("Faltam as definições")?,
                )?,
            )?;
            Ok(serde_json::json!({"ok": true}))
        }
        "versions" if payload.get("software").and_then(|v| v.as_str()) == Some("fabric") => {
            Ok(serde_json::to_value(crate::fabric::versions()?)?)
        }
        "versions" => Ok(serde_json::to_value(paper::versions(
            payload
                .get("contact")
                .and_then(|v| v.as_str())
                .context("Indica um contacto para a API PaperMC")?,
        )?)?),
        _ => bail!("Operação desconhecida"),
    }
}
