//! Java preparation, supervised process lifecycle and graceful shutdown.
use super::*;

impl Hub {
    pub fn install(self: &Arc<Self>, id: &str) -> Result<()> {
        self.prepare_install(id, false)
    }

    pub fn prepare_java(self: &Arc<Self>, id: &str) -> Result<()> {
        self.prepare_install(id, true)
    }

    pub fn java_status(&self, id: &str) -> Result<serde_json::Value> {
        let entry = self.entry(id)?;
        let cfg = Config::load(&entry.directory)?;
        let status = if cfg.java == Path::new("auto") {
            Err(anyhow::anyhow!(
                "O Java será descarregado quando preparares este servidor"
            ))
        } else {
            process::check_java(&cfg)
        };
        Ok(
            serde_json::json!({"compatible":status.is_ok(),"minimum":cfg.java_major,"path":cfg.java,"detail":status.err().map(|e|format!("{e:#}"))}),
        )
    }

    pub(super) fn ensure_java(
        &self,
        directory: &Path,
        id: &str,
        runtime: &Arc<Mutex<Runtime>>,
    ) -> Result<Config> {
        let mut cfg = Config::load(directory)?;
        if cfg.java != Path::new("auto") {
            match process::check_java(&cfg) {
                Ok(()) => {
                    runtime.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.log("[Nethergate] O Java configurado é compatível.".into());
                    return Ok(cfg);
                }
                Err(error) if cfg.java != Path::new("java") => return Err(error.context("O caminho Java personalizado não é compatível. Corrige-o nas definições ou escolhe auto")),
                Err(_) => {}
            }
        }
        crate::public_access::guard_change(&self.root, directory, id)?;
        let executable = crate::java::managed(&self.root, &cfg, |line| {
            runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .log(line)
        })?;
        cfg.java = executable;
        replace_json(&directory.join("nethergate.json"), &cfg)?;
        Ok(cfg)
    }

    pub(super) fn prepare_install(self: &Arc<Self>, id: &str, only_java: bool) -> Result<()> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.firewall_busy || !matches!(rt.state.as_str(), "offline" | "error") {
                bail!("O servidor está ocupado");
            }
            rt.guard = Some(config::lock(&entry.directory)?);
            rt.state = ServerState::Installing;
            rt.error = None;
            rt.log("[Nethergate] A preparar o Java e a verificar os ficheiros…".into());
        }
        let hub = Arc::clone(self);
        let id = id.to_string();
        thread::spawn(move || {
            let result = hub
                .ensure_java(&entry.directory, &id, &runtime)
                .and_then(|cfg| {
                    if only_java {
                        Ok(())
                    } else {
                        runtime.lock().map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?.log(match cfg.software {crate::config::Software::Paper=>"[Nethergate] A descarregar o PaperMC e a verificar SHA-256…",crate::config::Software::Fabric=>"[Nethergate] A preparar o launcher oficial Fabric; os ficheiros Minecraft serão obtidos no primeiro arranque…"}.into());
                        match cfg.software {
                            crate::config::Software::Paper => {
                                paper::install(&entry.directory, &cfg)
                            }
                            crate::config::Software::Fabric => {
                                crate::fabric::install(&entry.directory, &cfg)
                            }
                        }
                    }
                });
            let mut rt = runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match result {
                Ok(()) => {
                    rt.state = ServerState::Offline;
                    rt.guard = None;
                    rt.log(
                        if only_java {
                            "[Nethergate] Java preparado e verificado."
                        } else {
                            "[Nethergate] Instalação concluída. Já podes iniciar."
                        }
                        .into(),
                    );
                }
                Err(error) => rt.fail(&error),
            }
        });
        Ok(())
    }

    pub fn start(self: &Arc<Self>, id: &str) -> Result<()> {
        self.router
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .guard_change(id, crate::backup_schedule::now()?)?;
        self.start_inner(id, false)
    }

    pub(super) fn recovery_tick(self: &Arc<Self>, id: &str) -> Result<()> {
        let runtime = self.runtime(id)?;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state != "restart_waiting" || !rt.recovery.is_due() {
                return Ok(());
            }
            match crate::recovery::load(&self.root, id) {
                Ok(policy) if policy.enabled => {}
                _ => {
                    rt.recovery.cancel();
                    rt.state = ServerState::Offline;
                    return Ok(());
                }
            }
        }
        if let Err(error) = self.start_inner(id, true) {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.state == "restart_waiting" {
                rt.recovery.cancel();
                rt.fail(&error);
            }
        }
        Ok(())
    }

    pub(super) fn start_inner(self: &Arc<Self>, id: &str, automatic: bool) -> Result<()> {
        let entry = self.entry(id)?;
        let runtime = self.runtime(id)?;
        {
            let mut rt = runtime
                .lock()
                .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
            if rt.firewall_busy
                || if automatic {
                    rt.state != "restart_waiting"
                } else {
                    !matches!(rt.state.as_str(), "offline" | "error")
                }
            {
                bail!("O servidor já está ativo ou ocupado");
            }
            rt.guard = Some(config::lock(&entry.directory)?);
            rt.state = ServerState::Starting;
            rt.recovery.start(automatic);
            rt.error = None;
            rt.log("[Nethergate] A verificar o Java, a instalação e a porta…".into());
        }
        let hub = Arc::clone(self);
        let id = id.to_string();
        thread::spawn(move || {
            let result = (|| -> Result<Child> {
                let cfg = hub.ensure_java(&entry.directory, &id, &runtime)?;
                let bind =
                    std::net::SocketAddr::new(connection::bind_ip(&entry.directory)?, cfg.port);
                let probe = TcpListener::bind(bind)
                    .with_context(|| format!("Não foi possível reservar {bind}. Confirma se o IP ainda pertence ao PC e se a porta está livre"))?;
                process::prepare(&entry.directory, &cfg, false)?;
                drop(probe);
                let mut command = process::java_command(&entry.directory, &cfg);
                process::hide_window(&mut command);
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                command.spawn().context("Não foi possível iniciar o Java")
            })();
            let mut child = match result {
                Ok(child) => child,
                Err(error) => {
                    runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .fail(&error);
                    return;
                }
            };
            // Keep this guard on the Java supervisor thread, through shutdown.
            let sleep_guard = match SleepGuard::acquire() {
                Ok(guard) => Some(guard),
                Err(error) => {
                    runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .log(format!(
                            "[Nethergate] Proteção de energia indisponível: {error}"
                        ));
                    None
                }
            };
            let pipes = (|| -> Result<_> {
                Ok((
                    child
                        .stdout
                        .take()
                        .context("Não foi possível abrir a saída do Java")?,
                    child
                        .stderr
                        .take()
                        .context("Não foi possível abrir a saída de erros do Java")?,
                ))
            })();
            let (stdout, stderr) = match pipes {
                Ok(pipes) => pipes,
                Err(error) => {
                    // Do not leave an unobserved Java process behind on setup failure.
                    let cleanup = child.kill().and_then(|_| child.wait());
                    let mut rt = runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Err(cleanup_error) = cleanup {
                        rt.input = child.stdin.take();
                        rt.child = Some(child);
                        rt.state = ServerState::Error;
                        let message = format!("{error:#}. Não foi possível confirmar a paragem do Java: {cleanup_error}");
                        rt.error = Some(message.clone());
                        rt.log(format!("[Nethergate] {message}"));
                        // Keep the ownership lock until the process is confirmed stopped.
                    } else {
                        rt.fail(&error);
                    }
                    return;
                }
            };
            {
                let mut rt = runtime
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                rt.input = child.stdin.take();
                rt.child = Some(child);
                rt.sleep_protected = sleep_guard.is_some();
                if rt.sleep_protected {
                    rt.log("[Nethergate] Suspensão automática impedida enquanto este servidor estiver ativo.".into());
                }
                if rt.state == "stopping" {
                    if let Some(input) = &mut rt.input {
                        let _ = writeln!(input, "stop");
                        let _ = input.flush();
                    }
                }
            }
            let out_thread = read_logs(stdout, runtime.clone());
            let err_thread = read_logs(stderr, runtime.clone());
            let mut sleep_guard = sleep_guard;
            loop {
                let status = {
                    runtime
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .child
                        .as_mut()
                        .ok_or_else(|| {
                            std::io::Error::other(
                                "O processo Java deixou de estar disponível no supervisor",
                            )
                        })
                        .and_then(|child| child.try_wait())
                };
                match status {
                    Ok(Some(status)) => {
                        let _ = out_thread.join();
                        let _ = err_thread.join();
                        hub.finish_java_exit(
                            &entry.directory,
                            &id,
                            &runtime,
                            status.success(),
                            &status.to_string(),
                        );
                        drop(sleep_guard.take());
                        runtime
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .sleep_protected = false;
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(300)),
                    Err(error) => {
                        // Keep ownership/lock: do not permit a second start while the child may still be alive.
                        runtime
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .log(format!("[Nethergate] Falha ao consultar Java: {error}"));
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        });
        Ok(())
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        let runtime = self.runtime(id)?;
        let mut rt = runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        if rt.state == "restart_waiting" {
            rt.recovery.cancel();
            rt.state = ServerState::Offline;
            rt.log("[Nethergate] Reinício automático cancelado.".into());
            return Ok(());
        }
        if !matches!(rt.state.as_str(), "starting" | "online") {
            bail!("O servidor não está a correr");
        }
        if let Some(input) = &mut rt.input {
            writeln!(input, "stop")?;
            input.flush()?;
        }
        rt.state = ServerState::Stopping;
        self.sharing
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .stop(id);
        rt.log("[Nethergate] A guardar o mundo e a parar…".into());
        drop(rt);
        self.cleanup_router(id);
        Ok(())
    }

    pub fn command(&self, id: &str, command: &str) -> Result<()> {
        let command = command.trim().trim_start_matches('/');
        if command.is_empty() || command.len() > 1000 || command.chars().any(char::is_control) {
            bail!("Escreve um comando numa única linha");
        }
        if command.eq_ignore_ascii_case("stop") {
            return self.stop(id);
        }
        let runtime = self.runtime(id)?;
        let mut rt = runtime
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?;
        if rt.state != "online" {
            bail!("Aguarda até o servidor estar online");
        }
        let input = rt.input.as_mut().context("A consola não está disponível")?;
        writeln!(input, "{command}")?;
        input.flush()?;
        rt.log(format!("> {command}"));
        Ok(())
    }

    pub fn energy(&self) -> serde_json::Value {
        let runtimes: Vec<_> = self
            .runtimes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        let mut active = 0;
        let mut protected = 0;
        for runtime in runtimes {
            let rt = runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if rt.child.is_some() {
                active += 1;
            }
            if rt.sleep_protected {
                protected += 1;
            }
        }
        let (supply, error) = match energy::supply() {
            Ok(supply) => (Some(supply), None),
            Err(error) => (None, Some(error.to_string())),
        };
        serde_json::json!({"supported": cfg!(windows), "activeServers": active, "protectedServers": protected, "supply": supply, "error": error})
    }

    pub fn logs(&self, id: &str, after: u64) -> Result<Vec<LogLine>> {
        Ok(self
            .runtime(id)?
            .lock()
            .map_err(|error| anyhow::anyhow!("Não foi possível obter o lock: {error}"))?
            .logs
            .iter()
            .filter(|l| l.seq > after)
            .cloned()
            .collect())
    }

    pub(super) fn finish_java_exit(
        &self,
        directory: &Path,
        id: &str,
        runtime: &Arc<Mutex<Runtime>>,
        success: bool,
        status: &str,
    ) {
        self.cleanup_router(id);
        self.sharing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stop(id);
        let automatic;
        {
            let mut rt = runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            rt.child = None;
            rt.input = None;
            if !success {
                let enabled = crate::recovery::load(&self.root, id)
                    .map(|policy| policy.enabled)
                    .unwrap_or(false);
                let unexpected = rt.state != "stopping";
                let retry = rt
                    .recovery
                    .failure(unexpected, enabled, std::time::Instant::now());
                rt.fail(&anyhow::anyhow!(
                    "O Java terminou com {status}. Consulta a consola."
                ));
                if let Some(delay) = retry {
                    rt.state = ServerState::RestartWaiting;
                    rt.log(format!("[Nethergate] Nova tentativa de arranque em cerca de {} segundos. Podes cancelar o reinício.",delay.as_secs()));
                }
                return;
            }
            rt.log("[Nethergate] Servidor parado. O processo terminou sem erros.".into());
            let policy = {
                let _lock = self
                    .backup_policy
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                crate::backup_schedule::load(&self.root, id)
            };
            automatic = match policy {
                Ok(value) => value.policy.enabled,
                Err(error) => {
                    rt.log(format!(
                        "[Nethergate] Não foi possível ler o agendamento: {error:#}"
                    ));
                    false
                }
            };
            if automatic {
                rt.state = ServerState::BackingUp;
            } else {
                rt.state = ServerState::Offline;
                rt.guard = None;
            }
        }
        if automatic {
            self.finish_automatic_backup(directory, id, runtime);
        }
    }

    pub fn busy(&self) -> bool {
        !self
            .cloud_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
            || self
                .runtimes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .any(|rt| {
                    let rt = rt.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    rt.firewall_busy || !matches!(rt.state.as_str(), "offline" | "error")
                })
    }

    pub fn stop_all(&self) {
        self.cloud_cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.sharing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stop_all();
        let ids: Vec<_> = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|e| e.id.clone())
            .collect();
        for id in ids {
            let _ = self.stop(&id);
            self.cleanup_router(&id);
        }
    }
}
