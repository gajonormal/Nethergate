//! Exclusive world operations, backup scheduling and portable profiles.
use super::*;

impl Hub {
    pub fn backups(&self, id: &str) -> Result<Vec<BackupInfo>> {
        self.entry(id)?;
        backups::list(&crate::backup_destination::resolve(&self.root, id)?, id)
    }

    pub fn backup_schedule(&self, id: &str) -> Result<serde_json::Value> {
        let entry = self.entry(id)?;
        let state = self
            .runtime(id)?
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .state;
        let _lock = self
            .backup_policy
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        let schedule = crate::backup_schedule::load(&self.root, id)?;
        let now = crate::backup_schedule::now()?;
        let phase = if !schedule.policy.enabled {
            "disabled"
        } else if state == "backing_up" {
            "running"
        } else if state == "error" {
            "blocked"
        } else if !backups::world_ready(&entry.directory)? {
            "waiting_world"
        } else if schedule.due(now) && state != "offline" {
            "waiting_stop"
        } else if schedule.due(now) {
            "due"
        } else {
            "scheduled"
        };
        Ok(serde_json::json!({"schedule":schedule,"phase":phase,"serverState":state}))
    }

    pub fn save_backup_schedule(
        &self,
        id: &str,
        policy: crate::backup_schedule::Policy,
    ) -> Result<()> {
        self.entry(id)?;
        let _lock = self
            .backup_policy
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        crate::backup_schedule::save(&self.root, id, policy, crate::backup_schedule::now()?)?;
        Ok(())
    }

    pub(super) fn finish_automatic_backup(
        &self,
        directory: &Path,
        id: &str,
        runtime: &Arc<Mutex<Runtime>>,
    ) {
        runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .log("[Nethergate] A criar o backup automático com o mundo parado…".into());
        let result = (|| -> Result<(bool, Option<String>)> {
            let previous = {
                let _lock = self
                    .backup_policy
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
                crate::backup_schedule::load(&self.root, id)?.last_backup_id
            };
            let copy = (|| -> Result<Option<(BackupInfo, PathBuf)>> {
                let store = crate::backup_destination::resolve(&self.root, id)?;
                if let Some(previous) = previous {
                    if backups::unchanged_since(&store, id, directory, &previous)? {
                        return Ok(None);
                    }
                }
                Ok(Some((
                    backups::create(&store, id, directory, "automatic")?,
                    store,
                )))
            })();
            let _lock = self
                .backup_policy
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            let mut schedule = crate::backup_schedule::load(&self.root, id)?;
            let timestamp = crate::backup_schedule::now()?;
            match copy {
                Ok(None) => {
                    schedule.error = None;
                    schedule.next_due = schedule.policy.enabled.then_some(
                        timestamp.saturating_add(schedule.policy.interval_minutes as u64 * 60000),
                    );
                    crate::backup_schedule::persist(&self.root, id, &schedule)?;
                    Ok((false, None))
                }
                Ok(Some((copy, store))) => {
                    let warning = if schedule.policy.enabled {
                        backups::retain_automatic(&store, id, &copy.id, schedule.policy.keep)
                            .err()
                            .map(|e| {
                                format!("A cópia foi criada, mas a retenção não terminou: {e:#}")
                            })
                    } else {
                        None
                    };
                    schedule.succeeded(copy.id, timestamp, warning.clone());
                    crate::backup_schedule::persist(&self.root, id, &schedule)?;
                    Ok((true, warning))
                }
                Err(error) => {
                    schedule.failed(format!("{error:#}"), timestamp);
                    crate::backup_schedule::persist(&self.root, id, &schedule)?;
                    Err(error)
                }
            }
        })();
        let mut rt = runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rt.guard = None;
        rt.state = ServerState::Offline;
        match result {
            Ok((created, warning)) => {
                rt.backup_retry = None;
                rt.error = None;
                if let Some(warning) = warning {
                    rt.log(format!("[Nethergate] {warning}"));
                }
                rt.log(if created {"[Nethergate] Backup automático concluído e verificado."}else{"[Nethergate] O mundo não mudou e a cópia existente foi verificada; o histórico foi preservado."}.into());
            }
            Err(error) => {
                rt.backup_retry = Some(std::time::Instant::now());
                rt.error = Some(format!("Backup automático: {error:#}"));
                rt.log(format!("[Nethergate] Falha no backup automático: {error:#}. As cópias anteriores foram preservadas."));
            }
        }
    }

    pub(super) fn automatic_backup_tick(&self, id: &str) -> Result<()> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state != "offline"
                || rt.child.is_some()
                || rt.firewall_busy
                || rt.guard.is_some()
                || rt
                    .backup_retry
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(300))
            {
                return Ok(());
            }
            let schedule = {
                let _lock = self
                    .backup_policy
                    .lock()
                    .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
                crate::backup_schedule::load(&self.root, id)?
            };
            if !schedule.due(crate::backup_schedule::now()?)
                || !backups::world_ready(&entry.directory)?
            {
                return Ok(());
            }
            rt.guard = Some(config::lock(&entry.directory)?);
            rt.state = ServerState::BackingUp;
        }
        self.finish_automatic_backup(&entry.directory, id, &runtime);
        Ok(())
    }

    pub(super) fn world_operation<T>(
        &self,
        id: &str,
        state: ServerState,
        description: &str,
        task: impl FnOnce(&Path, &Path, &str) -> Result<T>,
    ) -> Result<T> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        let guard;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.firewall_busy
                || !matches!(rt.state.as_str(), "offline" | "error")
                || rt.child.is_some()
            {
                bail!(
                    "Para o servidor e aguarda as operações em curso antes de alterar os ficheiros"
                );
            }
            guard = config::lock(&entry.directory)?;
            rt.state = state;
            rt.error = None;
            rt.log(format!("[Nethergate] {description}"));
        }
        let store = if matches!(state, ServerState::BackingUp | ServerState::Restoring) {
            crate::backup_destination::resolve(&self.root, id)
        } else {
            Ok(self.root.join("hub-data/backups").join(id))
        };
        let result = store.and_then(|store| task(&store, &entry.directory, id));
        drop(guard);
        let mut rt = runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        match &result {
            Ok(_) => {
                rt.state = ServerState::Offline;
                rt.log(if state=="managing_plugins" {"[Nethergate] Plugin instalado e verificado. Será carregado no próximo arranque."} else {"[Nethergate] Operação do mundo concluída e ficheiros verificados."}.into());
            }
            Err(error) => rt.fail(error),
        }
        result
    }

    pub fn create_backup(&self, id: &str) -> Result<BackupInfo> {
        self.world_operation(
            id,
            ServerState::BackingUp,
            "A criar uma cópia completa do mundo e a verificar SHA-256…",
            |store, directory, server_id| backups::create(store, server_id, directory, "manual"),
        )
    }

    pub fn export_server(&self, id: &str, parent: &Path) -> Result<serde_json::Value> {
        let entry = self.entry(id)?;
        self.world_operation(
            id,
            ServerState::ExportingServer,
            "A preparar uma transferência verificada…",
            |_, directory, _| crate::transfer::export(directory, &entry.name, parent),
        )
    }

    pub fn import_server(&self, input: ImportServer) -> Result<String> {
        let _ports = self
            .port_changes
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        anyhow::ensure!(input.eula_accepted, "Lê e aceita a EULA antes de importar");
        let preview = crate::transfer::inspect(&input.path)?;
        anyhow::ensure!(
            preview["fingerprint"].as_str() == Some(&input.fingerprint),
            "O pacote mudou; revê novamente a importação"
        );
        let profile: crate::transfer::Profile = serde_json::from_value(preview["profile"].clone())?;
        let name = valid_name(&input.name)?;
        let mut cfg = Config::new(profile.minecraft, input.contact, profile.java_major)?;
        cfg.software = profile.software;
        cfg.port = input.port;
        cfg.memory_mb = input.memory_mb;
        cfg.java = input.java_path.into();
        cfg.validate()?;
        anyhow::ensure!(cfg.memory_mb <= 65536, "Escolhe no máximo 65536 MB de RAM");
        let mut entries = self
            .entries
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        check_port(&entries, cfg.port, None)?;
        let id = crate::transfer::key()?;
        let parent = self.root.join("servers");
        fs::create_dir_all(&parent)?;
        crate::backup_destination::directory(&parent)?;
        let staging = parent.join(format!(".import-{id}.partial"));
        crate::transfer::unpack(&input.path, &input.fingerprint, &staging, &cfg).with_context(
            || {
                format!(
                    "A importação não foi publicada. Ficheiros parciais preservados em {}",
                    staging.display()
                )
            },
        )?;
        let directory = parent.join(&id);
        fs::rename(&staging, &directory)?;
        let mut next = entries.clone();
        next.push(Entry {
            id: id.clone(),
            name,
            directory,
        });
        replace_json(&self.root.join("hub-data/registry.json"), &next)?;
        *entries = next;
        Ok(id)
    }

    pub fn install_plugin(
        &self,
        id: &str,
        project: &str,
        version: &str,
        expected: &str,
    ) -> Result<serde_json::Value> {
        self.world_operation(
            id,
            ServerState::ManagingPlugins,
            "A verificar o plugin, criar cópias de segurança e instalar…",
            |_, directory, server_id| {
                crate::plugins::install(
                    &self.root, server_id, directory, project, version, expected,
                )
            },
        )
    }

    pub fn migrate_server(
        &self,
        id: &str,
        input: crate::migration::Input,
    ) -> Result<serde_json::Value> {
        let name = valid_name(&input.name)?;
        anyhow::ensure!(input.port > 0, "Escolhe uma porta válida");
        let entry = self.entry(id)?;
        self.world_operation(id,ServerState::Migrating,"A preparar uma cópia para a nova versão do Minecraft…",|_,directory,_|{
            let _ports=self.port_changes.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;check_port(&self.entries.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?,input.port,None)?;
            let (new_id,path,details)=crate::migration::prepare(&self.root,id,directory,&entry.name,&input)?;
            let mut entries=self.entries.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;let mut next=entries.clone();next.push(Entry{id:new_id.clone(),name,directory:path.clone()});
            replace_json(&self.root.join("hub-data/registry.json"),&next).with_context(||format!("Cópia preparada mas não registada em {}",path.display()))?;*entries=next;
            Ok(serde_json::json!({"id":new_id,"path":path,"details":details,"firstStartRequired":true}))
        })
    }

    pub fn install_addon_set(
        &self,
        id: &str,
        projects: &[String],
        expected: &str,
    ) -> Result<serde_json::Value> {
        self.world_operation(
            id,
            ServerState::ManagingPlugins,
            "A resolver, verificar e preparar o conjunto de mods/plugins…",
            |_, directory, server_id| {
                crate::plugins::solver::install(
                    &self.root, server_id, directory, projects, expected,
                )
            },
        )
    }

    pub fn export_modpack(&self, id: &str) -> Result<serde_json::Value> {
        let name = self.entry(id)?.name;
        self.world_operation(
            id,
            ServerState::ExportingModpack,
            "A verificar os mods e preparar o pacote para os jogadores…",
            |_, directory, server_id| {
                crate::plugins::export_pack(&self.root, server_id, directory, &name)
            },
        )
    }

    pub fn restore_backup(&self, id: &str, backup_id: &str) -> Result<RestoreResult> {
        self.world_operation(
            id,
            ServerState::Restoring,
            "A verificar o backup, guardar o estado atual e restaurar o mundo…",
            |store, directory, server_id| backups::restore(store, server_id, directory, backup_id),
        )
    }
}
