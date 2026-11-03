use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
fn test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "nethergate-hub-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}
fn input(name: &str, port: u16) -> CreateServer {
    CreateServer {
        software: crate::config::Software::Paper,
        name: name.into(),
        version: "1.21.11".into(),
        memory_mb: 2048,
        port,
        contact: "https://example.org".into(),
        java_major: 21,
        java_path: "java".into(),
        eula_accepted: true,
    }
}
#[test]
fn internet_requests_reject_a_network_change_during_an_operation() {
    let hub = Hub::new_paused(test_root()).unwrap();
    let result = hub.internet_operation(|_| {
        let mut monitor = hub.network.lock().unwrap();
        let mut observe = |ip: &str| {
            monitor.observe(
                Ok(vec![connection::Adapter {
                    name: "Network fixture".into(),
                    ip: ip.parse().unwrap(),
                    gateway: Some("192.168.250.1".parse().unwrap()),
                }]),
                123,
            )
        };
        observe("192.168.250.2");
        observe("192.168.250.2");
        observe("192.168.250.3");
        observe("192.168.250.3");
        Ok(serde_json::json!({"state":"reported_online"}))
    });
    assert!(result.unwrap_err().to_string().contains("A rede mudou"));
    assert_eq!(
        hub.internet_operation(|_| Ok(serde_json::json!({"fresh":true})))
            .unwrap()["fresh"],
        true
    );
    // Busy diagnostics are rejected without blocking a second request.
    let _guard = hub.internet.lock().unwrap();
    assert!(hub
        .internet_operation(|_| panic!("must not execute"))
        .unwrap_err()
        .to_string()
        .contains("Aguarda"));
}
#[test]
fn library_switch_never_abandons_a_running_server() {
    let first = test_root();
    let target = test_root();
    fs::create_dir_all(target.join("hub-data")).unwrap();
    fs::write(target.join("hub-data/registry.json"), "[]").unwrap();
    let prefs = first.join("preferences/library.json");
    let session = crate::library::Session::new(first.clone(), prefs.clone(), None).unwrap();
    let hub = session.current();
    let id = hub.create(input("Em execução", 25598)).unwrap();
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    let candidate = session.preview(&target).unwrap();
    assert!(session
        .open(&candidate.root, &candidate.fingerprint)
        .is_err());
    assert!(!prefs.exists());
    assert_eq!(session.current().root(), first.canonicalize().unwrap());
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Offline;
}
#[test]
fn ready_crashes_queue_recovery_but_stop_and_disable_cancel_it() {
    let root = test_root();
    let hub = Hub::new(root.clone()).unwrap();
    let id = hub.create(input("Recuperação", 25597)).unwrap();
    crate::recovery::save(&root, &id, true).unwrap();
    let runtime = hub.runtime(&id).unwrap();
    {
        let mut rt = runtime.lock().unwrap();
        rt.state = ServerState::Online;
        rt.recovery.ready();
    }
    hub.finish_java_exit(
        &hub.entry(&id).unwrap().directory,
        &id,
        &runtime,
        false,
        "crash de teste",
    );
    assert_eq!(runtime.lock().unwrap().state, "restart_waiting");
    hub.stop(&id).unwrap();
    assert_eq!(runtime.lock().unwrap().state, "offline");
    {
        let mut rt = runtime.lock().unwrap();
        rt.state = ServerState::Stopping;
        rt.recovery.ready();
    }
    hub.finish_java_exit(
        &hub.entry(&id).unwrap().directory,
        &id,
        &runtime,
        false,
        "paragem de teste",
    );
    assert_eq!(runtime.lock().unwrap().state, "error");
}
#[test]
fn assisted_diagnosis_and_fallback_require_current_evidence_and_permission() {
    let hub = Hub::new_paused(test_root()).unwrap();
    let id = hub.create(input("Assistido", 25591)).unwrap();
    let advice = hub.sharing_advice(&id, 25591, false, "").unwrap();
    assert_eq!(advice["recommendation"], "review");
    assert!(advice["directTest"].is_null());
    assert_eq!(hub.sharing.lock().unwrap().status(&id)["state"], "idle");
    assert!(hub.sharing_advice(&id, 0, false, "").is_err());
    assert!(hub
        .sharing_advice(&id, 25591, false, "https://router.local")
        .is_err());
    let directory = hub.entry(&id).unwrap().directory;
    fs::write(
        directory.join("server.properties"),
        "server-ip=127.0.0.1\nonline-mode=true\nwhite-list=true\n",
    )
    .unwrap();
    fs::write(
        directory.join("whitelist.json"),
        r#"[{"uuid":"fixture","name":"PlayerOne"}]"#,
    )
    .unwrap();
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    let advice = hub.sharing_advice(&id, 25591, false, "").unwrap();
    assert_eq!(advice["recommendation"], "playit");
    assert!(advice["directTest"].is_null());
    let mut plan = AssistedStart {
        revision: advice["networkRevision"].as_u64().unwrap(),
        target: advice["target"].as_str().unwrap().parse().unwrap(),
        allow_fallback: false,
    };
    let error = hub
        .share(&id, 25591, true, false, false, Some(&plan))
        .unwrap_err();
    assert!(error.to_string().contains("não está autorizado"));
    assert_eq!(hub.sharing.lock().unwrap().status(&id)["state"], "error");
    plan.revision += 1;
    assert!(hub
        .share(&id, 25591, true, false, false, Some(&plan))
        .unwrap_err()
        .to_string()
        .contains("desatualizado"));
    assert!(
        !hub.root.join("hub-data/playit").exists(),
        "Diagnosis and denied fallback never prepare credentials or an agent"
    );
    plan.revision -= 1;
    plan.allow_fallback = true;
    assert!(hub
        .share(&id, 25591, false, false, false, Some(&plan))
        .unwrap_err()
        .to_string()
        .contains("Confirma"));
    let allowed = hub
        .share(&id, 25591, true, false, false, Some(&plan))
        .unwrap_err()
        .to_string();
    assert!(allowed.contains("playit"), "An authorized fallback reaches local playit verification, without downloading or associating an agent");
    assert!(!allowed.contains("não está autorizado"));
}
#[test]
fn cancelling_a_completed_check_never_stops_a_direct_server_or_a_new_check() {
    let hub = Hub::new_paused(test_root()).unwrap();
    let id = hub.create(input("Cancelar", 25590)).unwrap();
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    let operation = hub.sharing.lock().unwrap().begin_check(&id).unwrap();
    let payload = serde_json::json!({"id":id,"expectedOperation":operation});
    let cancelled = dispatch(&hub, "share_cancel_check", payload.clone()).unwrap();
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(hub.runtime(&id).unwrap().lock().unwrap().state, "online");
    let next = hub.sharing.lock().unwrap().begin_check(&id).unwrap();
    assert_eq!(
        dispatch(&hub, "share_cancel_check", payload.clone()).unwrap()["cancelled"],
        false
    );
    assert!(hub.sharing.lock().unwrap().check_matches(&id, next));
    hub.sharing
        .lock()
        .unwrap()
        .direct(&id, serde_json::json!({"address":"1.1.1.1:25590"}));
    let late = dispatch(&hub, "share_cancel_check", payload).unwrap();
    assert_eq!(late["state"], "direct");
    assert_eq!(late["cancelled"], false);
    assert_eq!(hub.runtime(&id).unwrap().lock().unwrap().state, "online");
}
#[test]
fn sharing_requires_consent_online_authentication_and_nonempty_whitelist() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Partilha", 25589)).unwrap();
    assert!(hub
        .share(&id, 25589, false, true, false, None)
        .unwrap_err()
        .to_string()
        .contains("Confirma"));
    assert!(hub
        .share(&id, 25589, true, true, false, None)
        .unwrap_err()
        .to_string()
        .contains("Inicia"));
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    let directory = hub.entry(&id).unwrap().directory;
    fs::write(
        directory.join("server.properties"),
        "online-mode=false\nwhite-list=true\n",
    )
    .unwrap();
    assert!(hub
        .share(&id, 25589, true, true, false, None)
        .unwrap_err()
        .to_string()
        .contains("autenticação"));
    fs::write(
        directory.join("server.properties"),
        "online-mode=true\nwhite-list=true\n",
    )
    .unwrap();
    assert!(hub
        .share(&id, 25589, true, true, false, None)
        .unwrap_err()
        .to_string()
        .contains("whitelist"));
    hub.sharing
        .lock()
        .unwrap()
        .direct(&id, serde_json::json!({"address":"1.1.1.1:25589"}));
    hub.stop(&id).unwrap();
    assert!(hub.sharing.lock().unwrap().status(&id)["address"].is_null());
}
#[test]
#[cfg(windows)]
fn ddns_test_requires_consent_online_and_current_configured_name() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Nome estavel", 25592)).unwrap();
    let payload = |consent, hostname| {
        serde_json::json!({
            "id":id,"consent":consent,"expectedHostname":hostname,
            "address":"attacker.example.org"
        })
    };
    assert!(
        dispatch(&hub, "ddns_test", payload(false, "test-only.duckdns.org"))
            .unwrap_err()
            .to_string()
            .contains("Confirma")
    );
    assert!(
        dispatch(&hub, "ddns_test", payload(true, "test-only.duckdns.org"))
            .unwrap_err()
            .to_string()
            .contains("Inicia")
    );
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    assert!(
        dispatch(&hub, "ddns_test", payload(true, "attacker.example.org"))
            .unwrap_err()
            .to_string()
            .contains("Configura")
    );
    hub.ddns
        .lock()
        .unwrap()
        .save(
            &hub.root,
            &id,
            crate::ddns::SaveDdns {
                hostname: "test-only.duckdns.org".into(),
                token: "01234567-89ab-cdef-0123-456789abcdef".into(),
                external_port: 25592,
                automatic: false,
                consent: true,
            },
        )
        .unwrap();
    assert!(
        dispatch(&hub, "ddns_test", payload(true, "attacker.example.org"))
            .unwrap_err()
            .to_string()
            .contains("mudou")
    );
    assert!(
        dispatch(&hub, "ddns_test", payload(false, "test-only.duckdns.org"))
            .unwrap_err()
            .to_string()
            .contains("Confirma")
    );
}
#[test]
fn shared_test_uses_active_share_address_and_requires_consent() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Teste externo", 25593)).unwrap();
    assert!(dispatch(
        &hub,
        "share_test",
        serde_json::json!({"id":id,"consent":true})
    )
    .unwrap_err()
    .to_string()
    .contains("Inicia o servidor"));
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    let payload = serde_json::json!({"id":id,"address":"attacker.example.org","consent":true});
    assert!(dispatch(&hub, "share_test", payload)
        .unwrap_err()
        .to_string()
        .contains("Inicia a partilha"));
    hub.sharing
        .lock()
        .unwrap()
        .direct(&id, serde_json::json!({"address":"1.1.1.1:25593"}));
    assert!(dispatch(
        &hub,
        "share_test",
        serde_json::json!({"id":id,"consent":false})
    )
    .unwrap_err()
    .to_string()
    .contains("Confirma"));
    hub.sharing.lock().unwrap().stop(&id);
    assert!(dispatch(
        &hub,
        "share_test",
        serde_json::json!({"id":id,"consent":true})
    )
    .is_err());
}
#[test]
fn automatic_backups_wait_for_stop_persist_and_do_not_run_after_crashes() {
    let root = test_root();
    let hub = Hub::new(root.clone()).unwrap();
    let id = hub.create(input("Agendamento", 25587)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    fs::create_dir_all(directory.join("world")).unwrap();
    fs::write(directory.join("world/level.dat"), "world data").unwrap();
    hub.save_backup_schedule(
        &id,
        crate::backup_schedule::Policy {
            enabled: true,
            interval_minutes: 60,
            keep: 2,
        },
    )
    .unwrap();
    let runtime = hub.runtime(&id).unwrap();
    runtime.lock().unwrap().state = ServerState::Online;
    hub.automatic_backup_tick(&id).unwrap();
    assert!(hub.backups(&id).unwrap().is_empty());
    assert_eq!(hub.backup_schedule(&id).unwrap()["phase"], "waiting_stop");
    runtime.lock().unwrap().state = ServerState::Offline;
    hub.automatic_backup_tick(&id).unwrap();
    assert_eq!(hub.backups(&id).unwrap().len(), 1);
    {
        let mut schedule = crate::backup_schedule::load(&root, &id).unwrap();
        schedule.next_due = Some(0);
        crate::backup_schedule::persist(&root, &id, &schedule).unwrap();
    }
    hub.automatic_backup_tick(&id).unwrap();
    assert_eq!(hub.backups(&id).unwrap().len(), 1);
    fs::write(directory.join("world/level.dat"), "world data after play").unwrap();
    assert!(!hub.busy());
    let schedule = crate::backup_schedule::load(&root, &id).unwrap();
    assert!(schedule.last_success.is_some());
    hub.automatic_backup_tick(&id).unwrap();
    assert_eq!(hub.backups(&id).unwrap().len(), 1);
    {
        let mut rt = runtime.lock().unwrap();
        rt.state = ServerState::Stopping;
        rt.guard = Some(config::lock(&directory).unwrap());
    }
    hub.finish_java_exit(&directory, &id, &runtime, true, "0");
    assert_eq!(hub.backups(&id).unwrap().len(), 2);
    assert!(runtime.lock().unwrap().guard.is_none());
    hub.finish_java_exit(&directory, &id, &runtime, false, "1");
    hub.automatic_backup_tick(&id).unwrap();
    assert_eq!(hub.backups(&id).unwrap().len(), 2);
    assert_eq!(runtime.lock().unwrap().state, "error");
}

#[test]
#[ignore = "Starts an isolated Paper server from existing immutable cache; opt-in runtime integration"]
fn real_paper_shutdown_finishes_automatic_backup_before_releasing_lock() {
    fn copy_tree(from: &Path, to: &Path) -> Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Unexpected cache symlink"
            );
            if metadata.is_dir() {
                copy_tree(&entry.path(), &to.join(entry.file_name()))?;
            } else {
                fs::copy(entry.path(), to.join(entry.file_name()))?;
            }
        }
        Ok(())
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("server");
    let root = test_root();
    let server = root.join("server");
    fs::create_dir(&server).unwrap();
    for folder in ["libraries", "cache", "versions"] {
        copy_tree(&source.join(folder), &server.join(folder)).unwrap();
    }
    for name in ["paper.jar", "paper.lock.json"] {
        fs::copy(source.join(name), server.join(name)).unwrap();
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
    cfg.port = port;
    cfg.memory_mb = 1024;
    config::write_json(&server.join("nethergate.json"), &cfg).unwrap();
    fs::write(server.join("eula.txt"), "eula=true\n").unwrap();
    fs::write(server.join("server.properties"),format!("server-ip=127.0.0.1\nserver-port={port}\nonline-mode=true\nlevel-type=minecraft:flat\nview-distance=2\nsimulation-distance=2\nmax-players=1\n")).unwrap();
    let hub = Hub::new(root.clone()).unwrap();
    hub.save_backup_schedule(
        "existing",
        crate::backup_schedule::Policy {
            enabled: true,
            interval_minutes: 60,
            keep: 2,
        },
    )
    .unwrap();
    hub.start("existing").unwrap();
    let runtime = hub.runtime("existing").unwrap();
    for _ in 0..900 {
        let state = runtime.lock().unwrap().state;
        if state == "online" || state == "error" {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let was_online = runtime.lock().unwrap().state == "online";
    let mut recovered = false;
    let mut first_pid = None;
    let mut second_pid = None;
    let mut memory = serde_json::Value::Null;
    if was_online {
        crate::recovery::save(&root, "existing", true).unwrap();
        first_pid = runtime
            .lock()
            .unwrap()
            .child
            .as_ref()
            .map(|child| child.id());
        memory = serde_json::to_value(hub.metrics.lock().unwrap().measure("existing", first_pid))
            .unwrap();
        // Intentional crash ONLY in the newly-created, loopback-only fixture.
        runtime
            .lock()
            .unwrap()
            .child
            .as_mut()
            .unwrap()
            .kill()
            .unwrap();
        for _ in 0..900 {
            let rt = runtime.lock().unwrap();
            if rt.state == "online" && rt.child.as_ref().map(|child| child.id()) != first_pid {
                second_pid = rt.child.as_ref().map(|child| child.id());
                recovered = true;
                break;
            }
            drop(rt);
            thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = hub.stop("existing");
    for _ in 0..1200 {
        if !hub.busy() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let error = runtime.lock().unwrap().error.clone();
    assert!(was_online, "Paper failed to become ready: {error:?}");
    assert!(
        recovered,
        "Paper did not recover from the isolated crash: {error:?}"
    );
    assert!(
        memory["memoryBytes"].as_u64().unwrap_or(0) > 0,
        "Java metrics unavailable: {memory}"
    );
    assert!(!hub.busy(), "Paper did not finish stopping");
    assert!(runtime.lock().unwrap().child.is_none());
    assert!(runtime.lock().unwrap().guard.is_none());
    let schedule = crate::backup_schedule::load(&root, "existing").unwrap();
    assert!(schedule.last_success.is_some(), "{error:?}");
    let copies = hub.backups("existing").unwrap();
    assert!(!copies.is_empty());
    assert!(copies.iter().all(|b| b.reason == "automatic"));
    config::lock(&server).unwrap();
    let proof = Path::new(env!("CARGO_MANIFEST_DIR")).join("hub-data/recovery-validation.json");
    fs::write(proof,serde_json::to_vec_pretty(&serde_json::json!({"root":root,"firstPid":first_pid,"recoveredPid":second_pid,"metrics":memory,"recovered":recovered,"saved":true})).unwrap()).unwrap();
    println!(
        "Verified real Paper shutdown + automatic snapshot: {}",
        root.display()
    );
}

#[test]
#[ignore = "Downloads Fabric/Minecraft and Fabric API, then starts an isolated loopback server"]
fn real_fabric_mods_players_export_and_shutdown() {
    let root = test_root();
    let directory = root.join("server");
    fs::create_dir(&directory).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut cfg = Config::new("1.21.11".into(), "https://example.org".into(), 21).unwrap();
    cfg.software = crate::config::Software::Fabric;
    cfg.port = port;
    cfg.memory_mb = 1024;
    config::write_json(&directory.join("nethergate.json"), &cfg).unwrap();
    fs::write(directory.join("eula.txt"), "eula=true\n").unwrap();
    fs::write(directory.join("server.properties"),format!("server-ip=127.0.0.1\nserver-port={port}\nonline-mode=true\nlevel-type=minecraft:flat\nview-distance=2\nsimulation-distance=2\nmax-players=1\n")).unwrap();
    crate::fabric::install(&directory, &cfg).unwrap();
    let plan = crate::plugins::preview(&directory, "fabric-api", None).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    crate::plugins::install(
        &root,
        "existing",
        &directory,
        &plan.project_id,
        &plan.version_id,
        &plan.fingerprint,
    )
    .unwrap();
    assert!(directory.join("nethergate-mods.json").exists());
    assert!(!directory.join("nethergate-plugins.json").exists());
    let client_only = crate::plugins::preview(&directory, "sodium", None).unwrap();
    assert!(!client_only.blockers.is_empty());
    let pack =
        crate::plugins::export_pack(&root, "existing", &directory, "Fabric validation").unwrap();
    let artifact = Path::new(env!("CARGO_MANIFEST_DIR")).join("hub-data/fabric-validation.json");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(
        &artifact,
        serde_json::to_vec_pretty(&serde_json::json!({"root":root,"pack":pack,"stage":"starting"}))
            .unwrap(),
    )
    .unwrap();
    let hub = Hub::new(root.clone()).unwrap();
    hub.save_backup_schedule(
        "existing",
        crate::backup_schedule::Policy {
            enabled: true,
            interval_minutes: 60,
            keep: 2,
        },
    )
    .unwrap();
    hub.start("existing").unwrap();
    let runtime = hub.runtime("existing").unwrap();
    for _ in 0..2400 {
        let state = runtime.lock().unwrap().state;
        if state == "online" || state == "error" {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let online = runtime.lock().unwrap().state == "online";
    let players = if online {
        hub.online_players("existing")
    } else {
        Err(anyhow::anyhow!("Server never became ready"))
    };
    let whitelist = if online {
        hub.player_operation("existing", "enable", "")
    } else {
        Err(anyhow::anyhow!("Server never became ready"))
    };
    let _ = hub.stop("existing");
    for _ in 0..1200 {
        if !hub.busy() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let rt = runtime.lock().unwrap();
    let logs = rt
        .logs
        .iter()
        .rev()
        .take(30)
        .map(|l| l.text.clone())
        .collect::<Vec<_>>();
    let child_still_running = rt.child.is_some();
    drop(rt);
    if child_still_running {
        let mut rt = runtime.lock().unwrap();
        if let Some(child) = &mut rt.child {
            let _ = child.kill();
        }
    }
    assert!(online, "Fabric failed: {logs:?}");
    assert_eq!(players.unwrap(), Vec::<String>::new());
    assert!(whitelist.unwrap().enabled);
    assert!(!hub.busy(), "Fabric did not stop: {logs:?}");
    assert!(!hub.backups("existing").unwrap().is_empty());
    assert!(runtime.lock().unwrap().guard.is_none());
    fs::write(&artifact,serde_json::to_vec_pretty(&serde_json::json!({"root":root,"pack":pack,"stage":"passed","players":true,"whitelist":true,"automaticBackup":true})).unwrap()).unwrap();
    println!(
        "Verified Fabric + Fabric API + client pack + whitelist + backup: {}",
        root.display()
    );
}

#[test]
fn missing_external_destination_persists_schedule_error_and_retry() {
    let root = test_root();
    let hub = Hub::new_paused(root.clone()).unwrap();
    let id = hub.create(input("External error", 25590)).unwrap();
    let entry = hub.entry(&id).unwrap();
    fs::create_dir(entry.directory.join("world")).unwrap();
    fs::write(entry.directory.join("world/level.dat"), "fixture").unwrap();
    let external = root.join("external");
    fs::create_dir(&external).unwrap();
    crate::backup_destination::save(&root, &id, Some(&external), &entry.directory).unwrap();
    hub.save_backup_schedule(
        &id,
        crate::backup_schedule::Policy {
            enabled: true,
            interval_minutes: 15,
            keep: 2,
        },
    )
    .unwrap();
    fs::rename(&external, root.join("disconnected")).unwrap();
    hub.automatic_backup_tick(&id).unwrap();
    let status = hub.backup_schedule(&id).unwrap();
    assert!(status["schedule"]["error"]
        .as_str()
        .unwrap()
        .contains("disco"));
    assert!(
        status["schedule"]["nextDue"].as_u64().unwrap() > crate::backup_schedule::now().unwrap()
    );
    assert!(!external.exists());
    assert_eq!(hub.runtime(&id).unwrap().lock().unwrap().state, "offline");
}
#[test]
fn failed_automatic_backup_preserves_history_and_backs_off() {
    let root = test_root();
    let hub = Hub::new(root.clone()).unwrap();
    let id = hub.create(input("Falha de backup", 25586)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    fs::create_dir_all(directory.join("world")).unwrap();
    fs::write(directory.join("world/level.dat"), "saved").unwrap();
    let previous = hub.create_backup(&id).unwrap();
    hub.save_backup_schedule(
        &id,
        crate::backup_schedule::Policy {
            enabled: true,
            interval_minutes: 15,
            keep: 2,
        },
    )
    .unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("world/session.lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    hub.automatic_backup_tick(&id).unwrap();
    let schedule = crate::backup_schedule::load(&root, &id).unwrap();
    assert!(schedule.error.is_some());
    assert!(schedule.last_success.is_none());
    assert_eq!(hub.backups(&id).unwrap()[0].id, previous.id);
    assert!(!hub.busy());
    drop(lock);
    hub.automatic_backup_tick(&id).unwrap();
    assert_eq!(hub.backups(&id).unwrap().len(), 1);
    hub.save_backup_schedule(
        &id,
        crate::backup_schedule::Policy {
            enabled: false,
            interval_minutes: 15,
            keep: 2,
        },
    )
    .unwrap();
    assert!(
        !crate::backup_schedule::load(&root, &id)
            .unwrap()
            .policy
            .enabled
    );
}
#[test]
fn pending_firewall_operation_blocks_configuration_and_new_start() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Firewall de teste", 25574)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let before = fs::read(directory.join("server.properties")).unwrap();
    let runtime = hub.runtime(&id).unwrap();
    runtime.lock().unwrap().firewall_busy = true;
    assert!(hub.busy());
    assert!(hub.start(&id).is_err());
    assert!(hub.prepare_java(&id).is_err());
    assert!(hub
        .install_plugin(&id, "chunky", "unused", "unused")
        .is_err());
    assert!(hub.set_connection(&id, "local", "").is_err());
    assert!(hub
        .settings(
            &id,
            Settings {
                name: "Outro".into(),
                memory_mb: 4096,
                port: 25575,
                java_path: "java".into()
            }
        )
        .is_err());
    assert!(hub.firewall(&id, false, None).is_err());
    assert_eq!(
        fs::read(directory.join("server.properties")).unwrap(),
        before
    );
    runtime.lock().unwrap().firewall_busy = false;
    assert!(!hub.busy());
    assert!(hub.firewall(&id, true, None).is_err());
    assert!(!hub.busy());
}

#[test]
fn invalid_custom_java_is_preserved_without_downloading() {
    let root = test_root();
    let hub = Hub::new(root.clone()).unwrap();
    let mut data = input("Java personalizado", 25589);
    data.java_path = "missing-custom-java.exe".into();
    let id = hub.create(data).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let before = fs::read(directory.join("nethergate.json")).unwrap();
    assert!(hub
        .ensure_java(&directory, &id, &hub.runtime(&id).unwrap())
        .is_err());
    assert_eq!(before, fs::read(directory.join("nethergate.json")).unwrap());
    assert!(!root.join("runtimes").exists());
}

#[test]
#[ignore = "Uses the real managed Java cache; opt-in Windows integration"]
fn prepares_automatic_java_without_starting_minecraft() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("hub-data/java-integration");
    fs::create_dir_all(&root).unwrap();
    let hub = Hub::new(root.clone()).unwrap();
    let views = hub.list().unwrap();
    let port = (25590..25999)
        .find(|port| !views.iter().any(|s| s.port == *port))
        .unwrap();
    let mut data = input("Java automático validado", port);
    data.java_path = "auto".into();
    let id = hub.create(data).unwrap();
    hub.prepare_java(&id).unwrap();
    let runtime = hub.runtime(&id).unwrap();
    for _ in 0..600 {
        if runtime.lock().unwrap().state != "installing" {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let rt = runtime.lock().unwrap();
    assert_eq!(rt.state, "offline", "{:?}", rt.error);
    assert!(rt.child.is_none());
    drop(rt);
    let cfg = Config::load(&hub.entry(&id).unwrap().directory).unwrap();
    assert!(cfg.java.is_absolute());
    assert!(cfg.java.is_file());
    assert!(hub.java_status(&id).unwrap()["compatible"]
        .as_bool()
        .unwrap());
}

#[test]
fn connection_changes_are_explicit_local_reversible_and_blocked_while_busy() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Ligação de teste", 25573)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let path = directory.join("server.properties");
    let original = fs::read_to_string(&path).unwrap();
    let adapters = vec![connection::Adapter {
        name: "Ethernet de teste".into(),
        ip: "192.168.1.50".parse().unwrap(),
        gateway: Some("192.168.1.1".parse().unwrap()),
    }];
    for address in [
        "192.168.1.99",
        "8.8.8.8",
        "0.0.0.0",
        "127.0.0.1",
        "192.168.1.50\nonline-mode=false",
    ] {
        assert!(connection::save(&directory, "lan", address, &adapters).is_err());
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    connection::save(&directory, "lan", "192.168.1.50", &adapters).unwrap();
    assert_eq!(
        connection::scope(connection::bind_ip(&directory).unwrap()),
        "lan"
    );
    assert_eq!(
        connection::local_address(&directory, 25573).unwrap(),
        "192.168.1.50:25573"
    );
    let edited = fs::read_to_string(&path).unwrap();
    assert!(edited.contains("online-mode=true"));
    assert!(edited.contains("server-port=25573"));
    assert_eq!(
        fs::read_to_string(directory.join("server.properties.before-connection")).unwrap(),
        original
    );
    for state in [
        "online",
        "starting",
        "stopping",
        "installing",
        "backing_up",
        "restoring",
    ] {
        hub.runtime(&id).unwrap().lock().unwrap().state = state.parse().unwrap();
        assert!(hub.set_connection(&id, "local", "").is_err());
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Offline;
    let guard = config::lock(&directory).unwrap();
    assert!(hub.set_connection(&id, "local", "").is_err());
    drop(guard);
    hub.set_connection(&id, "local", "").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    config::set_property(&path, "online-mode", "false").unwrap();
    assert!(connection::save(&directory, "lan", "192.168.1.50", &adapters).is_err());
}

#[test]
fn player_management_requires_running_authenticated_server_and_preserves_files() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Jogadores de teste", 25571)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let path = directory.join("whitelist.json");
    fs::write(
        &path,
        r#"[{"uuid":"00000000-0000-4000-8000-000000000001","name":"PlayerOne"}]"#,
    )
    .unwrap();
    let original = fs::read(&path).unwrap();
    assert_eq!(hub.player_access(&id).unwrap().players[0].name, "PlayerOne");
    for state in [
        "offline",
        "error",
        "starting",
        "stopping",
        "installing",
        "backing_up",
        "restoring",
    ] {
        hub.runtime(&id).unwrap().lock().unwrap().state = state.parse().unwrap();
        assert!(hub.player_operation(&id, "remove", "PlayerOne").is_err());
        assert!(hub.online_players(&id).is_err());
    }
    hub.runtime(&id).unwrap().lock().unwrap().state = ServerState::Online;
    config::set_property(&directory.join("server.properties"), "online-mode", "false").unwrap();
    assert!(hub
        .player_operation(&id, "enable", "")
        .unwrap_err()
        .to_string()
        .contains("online-mode"));
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::write(&path, "broken json").unwrap();
    assert!(hub.player_access(&id).is_err());
    assert!(hub.player_operation(&id, "add", "Alice").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "broken json");
}

#[test]
fn gameplay_persists_preserves_access_and_rejects_busy_or_invalid_changes() {
    let hub = Hub::new(test_root()).unwrap();
    let id = hub.create(input("Jogo de teste", 25569)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let path = directory.join("server.properties");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("# Preserve me\n{original}difficulty=1\ngamemode=0\n"),
    )
    .unwrap();
    let mut settings = hub.gameplay(&id, None).unwrap();
    assert_eq!(settings.difficulty, "easy");
    settings.difficulty = "hard".into();
    settings.gamemode = "creative".into();
    settings.max_players = 8;
    settings.view_distance = 6;
    settings.simulation_distance = 4;
    settings.pvp = false;
    let result = hub.gameplay(&id, Some(settings)).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# Preserve me"));
    assert!(saved.contains("server-ip=127.0.0.1"));
    assert!(saved.contains("online-mode=true"));
    assert!(saved.contains("difficulty=hard"));
    assert!(saved.contains("simulation-distance=4"));
    assert!(saved.contains("pvp=false"));
    assert_eq!(result.max_players, 8);
    assert_eq!(Gameplay::load(&directory).unwrap(), result);
    assert!(directory.join("server.properties.before-gameplay").exists());
    let mut invalid = hub.gameplay(&id, None).unwrap();
    invalid.difficulty = "hard\nonline-mode=false".into();
    assert!(hub.gameplay(&id, Some(invalid)).is_err());
    let runtime = hub.runtime(&id).unwrap();
    for state in [
        "online",
        "starting",
        "stopping",
        "backing_up",
        "restoring",
        "installing",
    ] {
        runtime.lock().unwrap().state = state.parse().unwrap();
        assert!(hub
            .gameplay(&id, Some(Gameplay::load(&directory).unwrap()))
            .is_err());
    }
    runtime.lock().unwrap().state = ServerState::Offline;
    let guard = config::lock(&directory).unwrap();
    assert!(hub
        .gameplay(&id, Some(Gameplay::load(&directory).unwrap()))
        .is_err());
    drop(guard);
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn persists_servers_and_rejects_duplicate_ports() {
    let root = test_root();
    let hub = Hub::new(root.clone()).unwrap();
    let id = hub.create(input("Mundo de teste", 25565)).unwrap();
    assert!(hub.create(input("Outro", 25565)).is_err());
    assert!(hub.entry("../secret").is_err());
    drop(hub);
    let hub = Hub::new(root).unwrap();
    assert_eq!(hub.list().unwrap()[0].id, id);
    assert!(!hub.list().unwrap()[0].installed);
}
#[test]
fn requires_eula_and_keeps_new_servers_local() {
    let hub = Hub::new(test_root()).unwrap();
    let mut request = input("Novo", 25566);
    request.eula_accepted = false;
    assert!(hub.create(request).is_err());
    let id = hub.create(input("Novo", 25566)).unwrap();
    let properties =
        fs::read_to_string(hub.entry(&id).unwrap().directory.join("server.properties")).unwrap();
    assert!(properties.contains("server-ip=127.0.0.1"));
    assert!(properties.contains("online-mode=true"));
}

#[test]
fn poisoned_locks_return_errors_and_cleanup_recovers_without_panicking() {
    let hub = Hub::new_paused(test_root()).unwrap();
    let id = hub.create(input("Poison fixture", 25571)).unwrap();
    let directory = hub.entry(&id).unwrap().directory;
    let runtime = hub.runtime(&id).unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = runtime.lock().unwrap();
        panic!("simulated supervisor failure");
    }));
    let error = hub.stop(&id).unwrap_err();
    assert!(error.to_string().contains("lock"));
    hub.finish_java_exit(&directory, &id, &runtime, false, "fixture failure");
    assert_eq!(
        runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state,
        ServerState::Error
    );
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = hub.entries.lock().unwrap();
        panic!("simulated registry failure");
    }));
    assert!(hub.list().err().unwrap().to_string().contains("lock"));
    assert!(hub.energy().is_object());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = hub.background_paused.write().unwrap();
        panic!("simulated pause failure");
    }));
    hub.resume_background();
    assert!(!*hub
        .background_paused
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner));
}
