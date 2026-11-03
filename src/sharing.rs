//! Application-owned playit sessions. Allocations persist; agent processes do not.
use crate::tunnel_process::OwnedChild;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    net::SocketAddr,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Default)]
pub struct Sharing {
    sessions: HashMap<String, Session>,
    generation: u64,
}
#[derive(Clone)]
pub(crate) struct RetryRequest {
    pub id: String,
    pub target: SocketAddr,
    generation: u64,
    address: String,
}
#[derive(Default)]
struct Retry {
    enabled: bool,
    attempts: u8,
    due: Option<Instant>,
    in_flight: bool,
    healthy_since: Option<Instant>,
}
struct Session {
    child: Option<OwnedChild>,
    output: Option<Receiver<Vec<u8>>>,
    buffer: String,
    started: Instant,
    address: Option<String>,
    view: Value,
    target: Option<SocketAddr>,
    generation: u64,
    retry: Retry,
}
impl Session {
    fn clear_process(&mut self) {
        self.child = None;
        self.output = None;
        self.buffer.clear();
        self.view["address"] = Value::Null;
    }
    fn end(&mut self, state: &str, reason: &str) {
        self.clear_process();
        self.retry.due = None;
        self.retry.in_flight = false;
        self.retry.enabled = false;
        self.view["state"] = json!(state);
        self.view["detail"] = json!(reason);
    }
    fn failed(&mut self, reason: &str, now: Instant) {
        self.clear_process();
        self.retry.in_flight = false;
        if self
            .retry
            .healthy_since
            .take()
            .is_some_and(|ready| now.saturating_duration_since(ready) >= Duration::from_secs(600))
        {
            self.retry.attempts = 0;
        }
        self.retry.due = None;
        if self.retry.enabled && self.retry.attempts < 3 {
            let delay = Duration::from_secs(5 * 3u64.pow(self.retry.attempts.into()));
            self.retry.attempts += 1;
            self.retry.due = Some(now + delay);
            self.view["state"] = json!("recovering");
            self.view["detail"] = json!(format!(
                "{reason} Recuperação automática agendada, usando o mesmo túnel."
            ));
        } else {
            self.view["state"] = json!("error");
            self.view["detail"] = json!(if self.retry.enabled {
                format!("{reason} As três tentativas automáticas terminaram. Revê a ligação e volta a partilhar manualmente.")
            } else {
                reason.to_owned()
            });
        }
    }
    fn status(&self) -> Value {
        let mut view = self.view.clone();
        view["recoveryEnabled"] = json!(self.retry.enabled);
        view["attempt"] = json!(self.retry.attempts);
        view["maximumAttempts"] = json!(3);
        view["remainingSeconds"] = json!(self
            .retry
            .due
            .map(|due| due.saturating_duration_since(Instant::now()).as_secs()));
        view
    }
    fn tick(&mut self) {
        if self.child.is_none() {
            return;
        }
        if let Some(output) = &self.output {
            for bytes in output.try_iter().take(64) {
                self.buffer.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        // The address is obtained from the authenticated API, never parsed from logs.
        if self.buffer.contains("tunnel running") && self.view["state"] == "connecting" {
            self.view["state"] = json!("tunnel");
            self.view["address"] = json!(self.address);
            self.retry.healthy_since = Some(Instant::now());
            self.view["detail"]=json!("Agente playit ligado. O endereço é reutilizado nas próximas sessões; confirma a entrada no Minecraft.");
        }
        if self.buffer.len() > 16384 {
            let mut at = self.buffer.len() - 8192;
            while !self.buffer.is_char_boundary(at) {
                at += 1;
            }
            self.buffer.drain(..at);
        }
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match child.child.try_wait() {
            Ok(Some(_)) | Err(_) => self.failed("O agente playit terminou.", Instant::now()),
            Ok(None)
                if self.view["state"] == "connecting"
                    && self.started.elapsed() > Duration::from_secs(90) =>
            {
                self.failed(
                    "O agente playit não confirmou a ligação em 90 segundos.",
                    Instant::now(),
                )
            }
            _ => {}
        }
    }
}
impl Sharing {
    pub(crate) fn begin_check(&mut self, id: &str) -> Result<u64> {
        let state = self.status(id);
        if matches!(
            state["state"].as_str(),
            Some("checking" | "direct" | "tunnel" | "connecting" | "recovering")
        ) {
            bail!("Termina a partilha ou verificação atual antes de iniciar outra");
        }
        self.generation += 1;
        let generation = self.generation;
        self.sessions.insert(id.into(), Session {
            child:None, output:None, buffer:String::new(), started:Instant::now(), address:None,
            view:json!({"state":"checking","operation":generation,"address":null,"detail":"A diagnosticar e preparar a ligação autorizada…"}),
            target:None, generation, retry:Retry::default(),
        });
        Ok(generation)
    }
    pub(crate) fn check_matches(&self, id: &str, generation: u64) -> bool {
        self.sessions
            .get(id)
            .is_some_and(|s| s.generation == generation && s.view["state"] == "checking")
    }
    pub(crate) fn fail_check(&mut self, id: &str, generation: u64, reason: &str) {
        if self.check_matches(id, generation) {
            if let Some(session) = self.sessions.get_mut(id) {
                session.end("error", reason);
            }
        }
    }
    pub(crate) fn decision(&mut self, id: &str, test: Option<Value>, fallback: bool) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.view["directTest"] = json!(test);
            session.view["fallbackUsed"] = json!(fallback);
        }
    }
    pub fn tick(&mut self) {
        for s in self.sessions.values_mut() {
            s.tick();
        }
    }
    pub fn status(&mut self, id: &str) -> Value {
        self.tick();
        self.sessions
            .get(id)
            .map(Session::status)
            .unwrap_or(json!({"state":"idle","address":null}))
    }
    pub fn stop(&mut self, id: &str) {
        if let Some(s) = self.sessions.get_mut(id) {
            s.end(
                "idle",
                "Partilha terminada. A atribuição do endereço playit foi preservada.",
            );
        }
    }
    pub fn stop_all(&mut self) {
        self.sessions.clear();
    }
    pub fn direct(&mut self, id: &str, test: Value) -> Value {
        self.stop(id);
        let view = json!({"state":"direct","address":test["address"],"test":test,
            "startedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "detail":"O serviço externo reportou o servidor online. Confirma a entrada no Minecraft; o teste pode estar em cache."});
        self.sessions.insert(
            id.into(),
            Session {
                child: None,
                output: None,
                buffer: String::new(),
                started: Instant::now(),
                address: None,
                view: view.clone(),
                target: None,
                generation: 0,
                retry: Retry::default(),
            },
        );
        view
    }
    pub fn tunnel(&mut self, root: &Path, id: &str, target: SocketAddr) -> Result<Value> {
        self.tunnel_with_recovery(root, id, target, false)
    }
    pub fn tunnel_with_recovery(
        &mut self,
        root: &Path,
        id: &str,
        target: SocketAddr,
        enabled: bool,
    ) -> Result<Value> {
        self.tick();
        if self
            .sessions
            .get(id)
            .is_some_and(|s| s.child.is_some() || s.view["state"] == "recovering")
        {
            bail!("Este servidor já está a ser partilhado");
        }
        self.generation += 1;
        let session = Self::launch(
            root,
            id,
            target,
            self.generation,
            Retry {
                enabled,
                ..Retry::default()
            },
            None,
        )?;
        let view = session.status();
        self.sessions.insert(id.into(), session);
        Ok(view)
    }
    fn launch(
        root: &Path,
        id: &str,
        target: SocketAddr,
        generation: u64,
        retry: Retry,
        expected_address: Option<&str>,
    ) -> Result<Session> {
        let (secret, tunnel) = crate::playit::credentials(root, id, target)?;
        if expected_address.is_some_and(|expected| expected != tunnel.address) {
            bail!("O endereço atribuído mudou. Verifica o playit antes de voltar a partilhar");
        }
        let mut command = Command::new(crate::playit::binary(root));
        command
            .args(["--stdout", "start"])
            .current_dir(root.join("hub-data/playit"))
            .env("PLAYIT_SECRET", secret)
            .env_remove("PLAYIT_SECRET_PATH")
            .env_remove("API_BASE")
            .env_remove("PLAYIT_LOG")
            .env("PLAYIT_LOG", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::process::hide_window(&mut command);
        let mut child = OwnedChild::new(
            command
                .spawn()
                .context("Não foi possível iniciar o agente playit")?,
        )?;
        let (send, receive) = mpsc::sync_channel(64);
        for mut reader in [
            Box::new(
                child
                    .child
                    .stdout
                    .take()
                    .context("Não foi possível abrir a saída do agente playit")?,
            ) as Box<dyn Read + Send>,
            Box::new(
                child
                    .child
                    .stderr
                    .take()
                    .context("Não foi possível abrir a saída de erros do agente playit")?,
            ) as Box<dyn Read + Send>,
        ] {
            let send = send.clone();
            thread::spawn(move || {
                let mut bytes = [0; 2048];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if send.try_send(bytes[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        let view = json!({"state":"connecting","address":null,"provider":"playit","loginConfirmed":false,
            "startedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "detail":"A ligar o agente playit ao túnel guardado…"});
        Ok(Session {
            child: Some(child),
            output: Some(receive),
            buffer: String::new(),
            started: Instant::now(),
            address: Some(tunnel.address),
            view,
            target: Some(target),
            generation,
            retry,
        })
    }
    pub(crate) fn network_changed(&mut self) {
        for session in self.sessions.values_mut() {
            if session.child.is_some() && session.retry.enabled {
                session.failed("Foi confirmada uma mudança de rede; a ligação do túnel precisa de ser restabelecida.", Instant::now());
            }
        }
    }
    pub(crate) fn due_retries(&mut self, now: Instant) -> Vec<RetryRequest> {
        self.sessions
            .iter_mut()
            .filter_map(|(id, s)| {
                if s.view["state"] == "recovering" && s.retry.due.is_some_and(|due| now >= due) {
                    s.retry.due = None;
                    s.retry.in_flight = true;
                    s.view["detail"] =
                        json!("A verificar a configuração antes de recuperar o túnel…");
                    Some(RetryRequest {
                        id: id.clone(),
                        target: s.target?,
                        generation: s.generation,
                        address: s.address.clone()?,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
    pub(crate) fn retry_matches(&self, request: &RetryRequest) -> bool {
        self.sessions.get(&request.id).is_some_and(|s| {
            s.generation == request.generation
                && s.view["state"] == "recovering"
                && s.retry.in_flight
                && s.retry.enabled
        })
    }
    pub(crate) fn abort_retry(&mut self, request: &RetryRequest, reason: &str) {
        if self.retry_matches(request) {
            if let Some(session) = self.sessions.get_mut(&request.id) {
                session.end("error", reason);
            }
        }
    }
    pub(crate) fn finish_retry(
        &mut self,
        root: &Path,
        request: &RetryRequest,
        validation: Result<()>,
    ) {
        self.finish_retry_with(request, validation, |retry| {
            Self::launch(
                root,
                &request.id,
                request.target,
                request.generation,
                retry,
                Some(&request.address),
            )
        });
    }
    fn finish_retry_with(
        &mut self,
        request: &RetryRequest,
        validation: Result<()>,
        launch: impl FnOnce(Retry) -> Result<Session>,
    ) {
        if !self.retry_matches(request) {
            return;
        }
        let Some(current) = self.sessions.get_mut(&request.id) else {
            return;
        };
        if let Err(error) = validation {
            current.failed(
                &format!("Não foi possível recuperar: {error:#}"),
                Instant::now(),
            );
            return;
        }
        let retry = std::mem::take(&mut current.retry);
        // Restore the budget on spawn failure; otherwise launch consumes it.
        let attempts = retry.attempts;
        match launch(Retry {
            in_flight: false,
            ..retry
        }) {
            Ok(mut session) => {
                if let Some(previous) = self.sessions.get(&request.id) {
                    for key in ["fallbackUsed", "directTest"] {
                        if let Some(value) = previous.view.get(key) {
                            session.view[key] = value.clone();
                        }
                    }
                }
                self.sessions.insert(request.id.clone(), session);
            }
            Err(error) => {
                let Some(s) = self.sessions.get_mut(&request.id) else {
                    return;
                };
                s.retry = Retry {
                    enabled: true,
                    attempts,
                    ..Retry::default()
                };
                s.failed(
                    &format!("Falha ao iniciar o agente: {error:#}"),
                    Instant::now(),
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_tickets_prevent_late_completion_and_duplicate_starts() {
        let mut sharing = Sharing::default();
        let first = sharing.begin_check("fixture").unwrap();
        assert_eq!(sharing.status("fixture")["state"], "checking");
        assert!(sharing.begin_check("fixture").is_err());
        sharing.stop("fixture");
        assert!(!sharing.check_matches("fixture", first));
        let second = sharing.begin_check("fixture").unwrap();
        assert_ne!(first, second);
        sharing.fail_check("fixture", first, "old failure");
        assert!(sharing.check_matches("fixture", second));
        sharing.fail_check("fixture", second, "current failure");
        assert_eq!(sharing.status("fixture")["detail"], "current failure");
        sharing.direct("fixture", json!({"address":"1.1.1.1:25565"}));
        assert!(sharing.begin_check("fixture").is_err());
    }
    fn fixture(enabled: bool) -> Session {
        Session {
            child: None,
            output: None,
            buffer: String::new(),
            started: Instant::now(),
            address: Some("fixture.joinmc.link".into()),
            view: json!({"state":"connecting","address":null,"startedAt":1}),
            target: Some("127.0.0.1:25565".parse().unwrap()),
            generation: 7,
            retry: Retry {
                enabled,
                ..Retry::default()
            },
        }
    }
    #[test]
    fn retry_delays_are_bounded_and_validation_failures_consume_budget() {
        let mut sharing = Sharing::default();
        let now = Instant::now();
        let mut session = fixture(true);
        session.failed("exit", now);
        sharing.sessions.insert("fixture".into(), session);
        for (attempt, seconds) in [(1, 5), (2, 15), (3, 45)] {
            let s = &sharing.sessions["fixture"];
            assert_eq!(s.retry.attempts, attempt);
            let due = s.retry.due.unwrap();
            // Exact initial delay and bounded subsequent delays without sleeping.
            if attempt == 1 {
                assert_eq!(due, now + Duration::from_secs(seconds));
            }
            assert!(due <= Instant::now() + Duration::from_secs(seconds));
            assert!(sharing
                .due_retries(due - Duration::from_millis(1))
                .is_empty());
            let requests = sharing.due_retries(due);
            assert_eq!(requests.len(), 1);
            assert!(
                sharing.due_retries(due).is_empty(),
                "Never dispatch duplicate workers"
            );
            sharing.finish_retry_with(&requests[0], Err(anyhow::anyhow!("offline")), |_| {
                panic!("Must validate before spawning")
            });
        }
        assert_eq!(sharing.status("fixture")["state"], "error");
        assert!(sharing.status("fixture")["address"].is_null());
        assert!(sharing
            .due_retries(Instant::now() + Duration::from_secs(999))
            .is_empty());
    }
    #[test]
    fn stop_replacement_and_abort_cancel_inflight_recovery() {
        for replacement in [false, true] {
            let mut sharing = Sharing::default();
            let mut s = fixture(true);
            s.failed("exit", Instant::now());
            let due = s.retry.due.unwrap();
            sharing.sessions.insert("fixture".into(), s);
            let request = sharing.due_retries(due).remove(0);
            if replacement {
                sharing.direct("fixture", json!({"address":"1.1.1.1:25565"}));
            } else {
                sharing.stop("fixture");
            }
            sharing.finish_retry_with(&request, Ok(()), |_| {
                panic!("Cancelled work must not spawn")
            });
            assert!(!sharing.retry_matches(&request));
            assert_eq!(
                sharing.status("fixture")["state"],
                if replacement { "direct" } else { "idle" }
            );
        }
        let mut sharing = Sharing::default();
        let mut s = fixture(true);
        s.failed("exit", Instant::now());
        let due = s.retry.due.unwrap();
        sharing.sessions.insert("fixture".into(), s);
        let request = sharing.due_retries(due).remove(0);
        sharing.abort_retry(&request, "IP mudou");
        sharing.finish_retry_with(&request, Ok(()), |_| panic!("Aborted work must not spawn"));
        assert_eq!(sharing.status("fixture")["recoveryEnabled"], false);
    }
    #[test]
    fn retries_keep_budget_after_success_and_reset_only_after_healthy_interval() {
        let mut sharing = Sharing::default();
        let mut s = fixture(true);
        s.failed("exit", Instant::now());
        let due = s.retry.due.unwrap();
        sharing.sessions.insert("fixture".into(), s);
        let request = sharing.due_retries(due).remove(0);
        sharing.finish_retry_with(&request, Ok(()), |retry| {
            let mut next = fixture(true);
            next.retry = retry;
            Ok(next)
        });
        let s = sharing.sessions.get_mut("fixture").unwrap();
        assert_eq!(s.retry.attempts, 1);
        assert_eq!(s.address.as_deref(), Some(request.address.as_str()));
        assert_eq!(s.target, Some(request.target));
        s.failed("another exit before ready", Instant::now());
        assert_eq!(s.retry.attempts, 2);
        s.retry.healthy_since = Some(Instant::now() - Duration::from_secs(601));
        s.failed("after stable session", Instant::now());
        assert_eq!(s.retry.attempts, 1);
        let mut disabled = fixture(false);
        disabled.failed("exit", Instant::now());
        assert_eq!(disabled.view["state"], "error");
        assert!(disabled.retry.due.is_none());
    }
    #[test]
    fn spawn_errors_keep_budget_and_stop_all_invalidates_workers() {
        let mut sharing = Sharing::default();
        let mut s = fixture(true);
        s.failed("exit", Instant::now());
        let due = s.retry.due.unwrap();
        sharing.sessions.insert("fixture".into(), s);
        let request = sharing.due_retries(due).remove(0);
        sharing.finish_retry_with(&request, Ok(()), |_| Err(anyhow::anyhow!("spawn failed")));
        assert_eq!(sharing.status("fixture")["attempt"], 2);
        let due = sharing.sessions["fixture"].retry.due.unwrap();
        let next = sharing.due_retries(due).remove(0);
        sharing.stop_all();
        sharing.finish_retry_with(&next, Ok(()), |_| panic!("Shutdown cancels workers"));
        assert_eq!(sharing.status("fixture")["state"], "idle");
    }
    #[test]
    fn stopping_clears_address_without_deleting_provider_allocation() {
        let mut sharing = Sharing::default();
        sharing.direct("test", json!({"address":"example:25565"}));
        sharing.stop("test");
        assert!(sharing.status("test")["address"].is_null());
        assert_eq!(sharing.status("test")["state"], "idle");
    }
    #[test]
    #[cfg(windows)]
    fn native_agent_exit_restarts_and_network_change_cancels_old_process() {
        fn native(retry: Retry) -> Session {
            let mut command =
                Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 60",
            ]);
            crate::process::hide_window(&mut command);
            let mut s = fixture(true);
            s.child = Some(OwnedChild::new(command.spawn().unwrap()).unwrap());
            let (send, receive) = mpsc::sync_channel(2);
            send.send(b"tunnel running\n".to_vec()).unwrap();
            s.output = Some(receive);
            s.retry = retry;
            s
        }
        let mut sharing = Sharing::default();
        sharing.sessions.insert(
            "fixture".into(),
            native(Retry {
                enabled: true,
                ..Retry::default()
            }),
        );
        assert_eq!(sharing.status("fixture")["state"], "tunnel");
        let s = sharing.sessions.get_mut("fixture").unwrap();
        let original_pid = s.child.as_ref().unwrap().child.id();
        s.child.as_mut().unwrap().child.kill().unwrap();
        s.child.as_mut().unwrap().child.wait().unwrap();
        sharing.tick();
        assert_eq!(sharing.status("fixture")["state"], "recovering");
        let due = sharing.sessions["fixture"].retry.due.unwrap();
        let request = sharing.due_retries(due).remove(0);
        sharing.finish_retry_with(&request, Ok(()), |retry| Ok(native(retry)));
        assert_eq!(sharing.status("fixture")["state"], "tunnel");
        assert_ne!(
            sharing.sessions["fixture"]
                .child
                .as_ref()
                .unwrap()
                .child
                .id(),
            original_pid
        );
        assert_eq!(sharing.status("fixture")["attempt"], 1);
        sharing.network_changed();
        assert!(sharing.sessions["fixture"].child.is_none());
        assert_eq!(sharing.status("fixture")["state"], "recovering");
        sharing.stop("fixture");
        assert!(sharing
            .due_retries(Instant::now() + Duration::from_secs(100))
            .is_empty());

        let mut timeout = native(Retry {
            enabled: true,
            ..Retry::default()
        });
        timeout.output = None;
        timeout.started = Instant::now() - Duration::from_secs(91);
        timeout.tick();
        assert_eq!(timeout.view["state"], "recovering");
        assert!(timeout.child.is_none());
    }
    #[test]
    #[cfg(windows)]
    fn agent_readiness_has_no_hourly_expiry_and_drop_reaps_process() {
        let mut command =
            Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ]);
        crate::process::hide_window(&mut command);
        let child = OwnedChild::new(command.spawn().unwrap()).unwrap();
        let (send, receive) = mpsc::sync_channel(4);
        let mut s = Session {
            child: Some(child),
            output: Some(receive),
            buffer: String::new(),
            started: Instant::now(),
            address: Some("test.joinmc.link".into()),
            view: json!({"state":"connecting","address":null}),
            target: Some("127.0.0.1:25565".parse().unwrap()),
            generation: 1,
            retry: Retry::default(),
        };
        send.send(b"tunnel run".to_vec()).unwrap();
        s.tick();
        assert_eq!(s.view["state"], "connecting");
        send.send(b"ning\n".to_vec()).unwrap();
        s.tick();
        assert_eq!(s.view["state"], "tunnel");
        s.started = Instant::now() - Duration::from_secs(7200);
        s.tick();
        assert_eq!(s.view["state"], "tunnel");
        s.end("idle", "stopped");
        assert!(s.child.is_none());
        assert!(s.view["address"].is_null());
    }
}
