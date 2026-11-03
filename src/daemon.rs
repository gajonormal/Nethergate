//! Local engine process. No public listener, browser session endpoint or remote access.
use crate::{
    config,
    library::{Preview, Session},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use tiny_http::{Method, Request, Response, Server};

const PROTOCOL: u8 = 1;
/// Held by process entry points until explicit process exit, never by embedded servers.
pub struct ProcessGuard {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}
pub fn process_guard() -> Result<ProcessGuard> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::{JobObjects::*, Threading::GetCurrentProcess};
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        anyhow::ensure!(
            !job.is_null(),
            "Não foi possível proteger os processos do motor"
        );
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) } == 0
        {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
            bail!("O Windows não permitiu proteger os processos do motor");
        }
        Ok(ProcessGuard { job })
    }
    #[cfg(not(windows))]
    {
        Ok(ProcessGuard {})
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    protocol: u8,
    port: u16,
    token: String,
    pid: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Call { action: String, payload: Value },
    Info {},
    Preview { path: PathBuf },
    Open { path: PathBuf, fingerprint: String },
    Notifications {},
    Shutdown {},
}
fn runtime(preferences: &Path) -> Result<PathBuf> {
    Ok(preferences
        .parent()
        .context("Falta a pasta das preferências")?
        .join("engine-runtime"))
}
fn endpoint_path(preferences: &Path) -> Result<PathBuf> {
    Ok(runtime(preferences)?.join("endpoint.protected"))
}
fn read_endpoint(preferences: &Path) -> Result<Endpoint> {
    let bytes = fs::read(endpoint_path(preferences)?)?;
    anyhow::ensure!(bytes.len() <= 16384, "Estado do motor inválido");
    let endpoint: Endpoint = serde_json::from_slice(&crate::secrets::protect(&bytes, true)?)?;
    anyhow::ensure!(
        endpoint.protocol == PROTOCOL
            && endpoint.port != 0
            && endpoint.token.len() == 64
            && endpoint.token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Versão ou estado do motor incompatível"
    );
    Ok(endpoint)
}
fn http(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
pub struct Client {
    endpoint: Endpoint,
}
impl Client {
    pub fn connect(preferences: &Path) -> Result<Self> {
        let client = Self {
            endpoint: read_endpoint(preferences)?,
        };
        client.request(&Operation::Info {}, Duration::from_secs(3))?;
        Ok(client)
    }
    pub fn connect_or_spawn(exe: &Path, root: &Path, preferences: &Path) -> Result<Self> {
        let parent = preferences
            .parent()
            .context("Falta a pasta de preferências")?;
        let launch = parent.join("engine-launch");
        fs::create_dir_all(&launch)?;
        let _launch = config::lock(&launch).context(
            "O motor está a ser preparado noutra janela. Aguarda e tenta abrir novamente",
        )?;
        if let Ok(client) = Self::connect(preferences) {
            return Ok(client);
        }
        let directory = runtime(preferences)?;
        fs::create_dir_all(&directory)?;
        // Never replace the endpoint or launch a duplicate while an engine owns its lock.
        drop(config::lock(&directory).context("Existe um motor ativo mas não foi possível ligar. Não foi iniciada outra instância; tenta abrir novamente")?);
        let mut command = Command::new(exe);
        command
            .arg("--engine")
            .arg("--engine-root")
            .arg(root)
            .arg("--engine-preferences")
            .arg(preferences)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::process::hide_window(&mut command);
        let mut child = command
            .spawn()
            .context("Não foi possível iniciar o motor local")?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(client) = Self::connect(preferences) {
                return Ok(client);
            }
            if let Some(status) = child.try_wait()? {
                bail!("O motor terminou antes de ficar pronto ({status}). Confirma que a biblioteca não está aberta numa versão anterior");
            }
            if Instant::now() >= deadline {
                bail!("O motor ainda não confirmou o arranque. Reabre o painel para tentar ligar; não foram parados processos");
            }
            thread::sleep(Duration::from_millis(150));
        }
    }
    fn request(&self, operation: &Operation, timeout: Duration) -> Result<Value> {
        let response = http(timeout)?
            .post(format!("http://127.0.0.1:{}/engine", self.endpoint.port))
            .header("X-NetherGate-Engine", &self.endpoint.token)
            .json(operation)
            .send()
            .map_err(|_| {
                anyhow::anyhow!(
                    "Não foi possível contactar o motor local. Reabre o painel para voltar a ligar"
                )
            })?;
        let success = response.status().is_success();
        let mut bytes = Vec::new();
        response.take(8_388_609).read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() <= 8_388_608,
            "Resposta do motor demasiado grande"
        );
        let value: Value = serde_json::from_slice(&bytes).context("Resposta do motor inválida")?;
        if !success {
            bail!(
                "{}",
                value["error"]
                    .as_str()
                    .unwrap_or("O motor recusou o pedido")
            );
        }
        Ok(value)
    }
    pub fn dispatch(&self, action: &str, payload: Value) -> Result<Value> {
        self.request(
            &Operation::Call {
                action: action.into(),
                payload,
            },
            Duration::from_secs(900),
        )
    }
    pub fn info(&self) -> Result<Value> {
        self.request(&Operation::Info {}, Duration::from_secs(5))
    }
    pub fn preview(&self, path: &Path) -> Result<Preview> {
        Ok(serde_json::from_value(self.request(
            &Operation::Preview { path: path.into() },
            Duration::from_secs(30),
        )?)?)
    }
    pub fn open(&self, path: &Path, fingerprint: &str) -> Result<Value> {
        self.request(
            &Operation::Open {
                path: path.into(),
                fingerprint: fingerprint.into(),
            },
            Duration::from_secs(30),
        )
    }
    pub fn notification_snapshot(&self) -> Result<crate::notifications::Snapshot> {
        Ok(serde_json::from_value(self.request(
            &Operation::Notifications {},
            Duration::from_secs(5),
        )?)?)
    }
    pub fn shutdown(&self) -> Result<Value> {
        self.request(&Operation::Shutdown {}, Duration::from_secs(900))
    }
}
struct Control {
    accepting: AtomicBool,
    exit: AtomicBool,
    operations: RwLock<()>,
    workers: AtomicUsize,
}
fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().into())
}
fn respond(request: Request, status: u16, value: Value) {
    let mut response = Response::from_data(value.to_string().into_bytes()).with_status_code(status);
    for (name, value) in [
        ("Content-Type", "application/json"),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        match tiny_http::Header::from_bytes(name, value) {
            Ok(header) => response.add_header(header),
            Err(_) => {
                let _ = request.respond(tiny_http::Response::empty(500));
                return;
            }
        }
    }
    let _ = request.respond(response);
}
fn handle(mut request: Request, session: Arc<Session>, endpoint: &Endpoint, control: &Control) {
    if request.method() != &Method::Post
        || request.url() != "/engine"
        || header(&request, "Host").as_deref()
            != Some(format!("127.0.0.1:{}", endpoint.port).as_str())
        || header(&request, "Origin").is_some()
        || header(&request, "X-NetherGate-Engine").as_deref() != Some(&endpoint.token)
    {
        respond(request, 403, json!({"error":"Pedido local não autorizado"}));
        return;
    }
    let operation = (|| -> Result<Operation> {
        let mut body = Vec::new();
        request.as_reader().take(65537).read_to_end(&mut body)?;
        anyhow::ensure!(body.len() <= 65536, "Pedido demasiado grande");
        serde_json::from_slice(&body).context("Pedido do motor inválido")
    })();
    let operation = match operation {
        Ok(value) => value,
        Err(error) => {
            respond(request, 400, json!({"error":error.to_string()}));
            return;
        }
    };
    if matches!(operation, Operation::Shutdown {}) {
        if !control.accepting.swap(false, Ordering::SeqCst) {
            respond(
                request,
                409,
                json!({"error":"O motor já está a guardar e parar"}),
            );
            return;
        }
        let _exclusive = control
            .operations
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let hub = session.current();
        hub.stop_all();
        while hub.busy() || hub.pause_background().is_err() {
            thread::sleep(Duration::from_millis(200));
        }
        respond(request, 200, json!({"stopped":true}));
        control.exit.store(true, Ordering::SeqCst);
        return;
    }
    let _operation = control
        .operations
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !control.accepting.load(Ordering::SeqCst) {
        respond(
            request,
            409,
            json!({"error":"O motor está a guardar e parar os servidores"}),
        );
        return;
    }
    let result = (|| -> Result<Value> {
        Ok(match operation {
            Operation::Info {} => {
                let mut info = session.info()?;
                info["independent"] = json!(true);
                info["pid"] = json!(endpoint.pid);
                info["protocol"] = json!(PROTOCOL);
                info
            }
            Operation::Call { action, payload } => session.dispatch(&action, payload)?,
            Operation::Preview { path } => serde_json::to_value(session.preview(&path)?)?,
            Operation::Open { path, fingerprint } => session.open(&path, &fingerprint)?,
            Operation::Notifications {} => serde_json::to_value(session.notification_snapshot()?)?,
            Operation::Shutdown {} => {
                anyhow::bail!("A paragem do motor já deveria ter sido tratada")
            }
        })
    })();
    match result {
        Ok(value) => respond(request, 200, value),
        Err(error) => respond(request, 400, json!({"error":format!("{error:#}")})),
    }
}
pub fn serve(root: PathBuf, preferences: PathBuf) -> Result<()> {
    anyhow::ensure!(
        root.is_absolute() && preferences.is_absolute(),
        "O motor exige caminhos absolutos"
    );
    let directory = runtime(&preferences)?;
    fs::create_dir_all(&directory)?;
    let _guard = config::lock(&directory).context("Já existe um motor para estas preferências")?;
    let session = Arc::new(Session::new(root, preferences.clone(), None)?);
    let server = Server::http(("127.0.0.1", 0))
        .map_err(|_| anyhow::anyhow!("Não foi possível abrir o canal local do motor"))?;
    let port = server
        .server_addr()
        .to_ip()
        .context("Canal local inválido")?
        .port();
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random)
        .map_err(|_| anyhow::anyhow!("Falha ao gerar credencial do motor"))?;
    let endpoint = Arc::new(Endpoint {
        protocol: PROTOCOL,
        port,
        token: random.iter().map(|b| format!("{b:02x}")).collect(),
        pid: std::process::id(),
    });
    let encrypted = crate::secrets::protect(&serde_json::to_vec(endpoint.as_ref())?, false)?;
    let path = endpoint_path(&preferences)?;
    let staging = path.with_extension("protected.tmp");
    fs::write(&staging, encrypted)?;
    fs::rename(staging, &path)?;
    let control = Arc::new(Control {
        accepting: AtomicBool::new(true),
        exit: AtomicBool::new(false),
        operations: RwLock::new(()),
        workers: AtomicUsize::new(0),
    });
    while !control.exit.load(Ordering::SeqCst) {
        let Some(request) = server.recv_timeout(Duration::from_millis(200))? else {
            continue;
        };
        if control.workers.fetch_add(1, Ordering::SeqCst) >= 16 {
            control.workers.fetch_sub(1, Ordering::SeqCst);
            respond(
                request,
                503,
                json!({"error":"Motor ocupado. Aguarda as operações em curso"}),
            );
            continue;
        }
        let (session, endpoint, control) = (session.clone(), endpoint.clone(), control.clone());
        thread::spawn(move || {
            handle(request, session, &endpoint, &control);
            control.workers.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let _ = fs::remove_file(path);
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ng-engine-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("hub-data")).unwrap();
        fs::write(root.join("hub-data/registry.json"), "[]").unwrap();
        root
    }
    struct Running {
        client: Client,
        join: Option<thread::JoinHandle<Result<()>>>,
    }
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.client.shutdown();
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
    fn run(root: &Path, preferences: &Path) -> Running {
        let (root, prefs) = (root.to_owned(), preferences.to_owned());
        let join = thread::spawn(move || serve(root, prefs));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(client) = Client::connect(preferences) {
                return Running {
                    client,
                    join: Some(join),
                };
            }
            assert!(Instant::now() < deadline, "Engine did not become ready");
            thread::sleep(Duration::from_millis(50));
        }
    }
    #[test]
    fn authenticated_engine_survives_clients_and_preserves_library_switching() {
        let root = fixture("session");
        let prefs = root.join("prefs/library.json");
        let mut running = run(&root, &prefs);
        let first = Client::connect(&prefs).unwrap();
        let pid = first.info().unwrap()["pid"].clone();
        first.dispatch("create",json!({"software":"paper","name":"Motor isolado","version":"1.21.11","memoryMb":1024,"port":25585,"contact":"https://example.org","javaMajor":21,"javaPath":"java","eulaAccepted":true})).unwrap();
        drop(first);
        let second = Client::connect(&prefs).unwrap();
        assert_eq!(second.info().unwrap()["pid"], pid);
        assert_eq!(
            second
                .dispatch("list", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(second.notification_snapshot().unwrap().servers.len(), 1);
        assert!(
            serve(root.clone(), prefs.clone()).is_err(),
            "No duplicate engine can own the endpoint"
        );
        let next = fixture("next-library");
        let preview = second.preview(&next).unwrap();
        second.open(&next, &preview.fingerprint).unwrap();
        assert_eq!(second.info().unwrap()["serverCount"], 0);
        assert_eq!(
            crate::library::selected(&prefs).unwrap().unwrap(),
            next.canonicalize().unwrap()
        );
        assert_eq!(Client::connect(&prefs).unwrap().info().unwrap()["pid"], pid);
        second.shutdown().unwrap();
        running.join.take().unwrap().join().unwrap().unwrap();
        assert!(!endpoint_path(&prefs).unwrap().exists());
        assert!(Client::connect(&prefs).is_err());
    }
    #[test]
    fn engine_rejects_browser_origins_missing_credentials_and_unbounded_input() {
        let root = fixture("auth");
        let prefs = root.join("prefs/library.json");
        let _running = run(&root, &prefs);
        let endpoint = read_endpoint(&prefs).unwrap();
        let protected = fs::read(endpoint_path(&prefs).unwrap()).unwrap();
        assert!(!protected
            .windows(endpoint.token.len())
            .any(|w| w == endpoint.token.as_bytes()));
        let url = format!("http://127.0.0.1:{}/engine", endpoint.port);
        let client = http(Duration::from_secs(5)).unwrap();
        assert_eq!(
            client
                .post(&url)
                .json(&Operation::Info {})
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .post(&url)
                .header("X-NetherGate-Engine", "wrong")
                .json(&Operation::Info {})
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .post(&url)
                .header("X-NetherGate-Engine", &endpoint.token)
                .header("Origin", "http://127.0.0.1")
                .json(&Operation::Info {})
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .post(&url)
                .header("X-NetherGate-Engine", &endpoint.token)
                .header("Host", "example.org")
                .json(&Operation::Info {})
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("http://127.0.0.1:{}/api/session", endpoint.port))
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .post(&url)
                .header("X-NetherGate-Engine", &endpoint.token)
                .body("x".repeat(65537))
                .send()
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            client
                .post(&url)
                .header("X-NetherGate-Engine", &endpoint.token)
                .json(&json!({"kind":"info","unexpected":"field"}))
                .send()
                .unwrap()
                .status(),
            400
        );
    }
    #[test]
    #[ignore = "Starts the compiled desktop engine and an isolated Paper world using existing immutable caches"]
    fn independent_engine_keeps_real_paper_alive_between_clients_and_saves_on_exit() {
        fn copy_cache(from: &Path, to: &Path) -> Result<()> {
            fs::create_dir_all(to)?;
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                anyhow::ensure!(!metadata.file_type().is_symlink(), "Unexpected cache link");
                if metadata.is_dir() {
                    copy_cache(&entry.path(), &to.join(entry.file_name()))?;
                } else {
                    fs::copy(entry.path(), to.join(entry.file_name()))?;
                }
            }
            Ok(())
        }
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
        let exe = workspace.join("src-tauri/target/debug/nethergate-desktop.exe");
        assert!(exe.is_file(), "Build the desktop first");
        let source = workspace.join("server");
        let mut cfg = crate::config::Config::load(&source).unwrap();
        cfg.memory_mb = 1024;
        crate::process::check_java(&cfg).unwrap();
        let root = fixture("paper-process");
        fs::remove_file(root.join("hub-data/registry.json")).unwrap();
        let server = root.join("server");
        fs::create_dir_all(&server).unwrap();
        for folder in ["libraries", "cache", "versions"] {
            copy_cache(&source.join(folder), &server.join(folder)).unwrap();
        }
        for name in ["paper.jar", "paper.lock.json"] {
            fs::copy(source.join(name), server.join(name)).unwrap();
        }
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        cfg.port = listener.local_addr().unwrap().port();
        drop(listener);
        config::write_json(&server.join("nethergate.json"), &cfg).unwrap();
        fs::write(server.join("eula.txt"), "eula=true\n").unwrap();
        fs::write(server.join("server.properties"),format!("server-ip=127.0.0.1\nserver-port={}\nonline-mode=true\nlevel-type=minecraft:flat\nview-distance=2\nsimulation-distance=2\nmax-players=1\n",cfg.port)).unwrap();
        let preferences = root.join("prefs/library.json");
        let first = Client::connect_or_spawn(&exe, &root, &preferences).unwrap();
        struct Stop<'a>(&'a Path);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                if let Ok(client) = Client::connect(self.0) {
                    let _ = client.shutdown();
                }
            }
        }
        let _cleanup = Stop(&preferences);
        first
            .dispatch(
                "save_backup_schedule",
                json!({"id":"existing","policy":{"enabled":true,"intervalMinutes":60,"keep":2}}),
            )
            .unwrap();
        first.dispatch("start", json!({"id":"existing"})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        let online = loop {
            let list = first.dispatch("list", json!({})).unwrap();
            if list[0]["state"] == "online" {
                break list[0].clone();
            }
            assert_ne!(list[0]["state"], "error", "{}", list[0]["error"]);
            assert!(Instant::now() < deadline, "Paper readiness timeout");
            thread::sleep(Duration::from_millis(500));
        };
        let engine_pid = first.info().unwrap()["pid"].clone();
        assert_ne!(engine_pid, json!(std::process::id()));
        drop(first);
        thread::sleep(Duration::from_secs(2));
        let second = Client::connect(&preferences).unwrap();
        assert_eq!(second.info().unwrap()["pid"], engine_pid);
        let current = second.dispatch("list", json!({})).unwrap();
        assert_eq!(current[0]["pid"], online["pid"]);
        assert_eq!(current[0]["state"], "online");
        assert_eq!(
            crate::connection::probe(([127, 0, 0, 1], cfg.port).into()).status,
            "minecraft"
        );
        let third = Client::connect_or_spawn(&exe, &root, &preferences).unwrap();
        assert_eq!(
            third.info().unwrap()["pid"],
            engine_pid,
            "Reopening never starts a second engine"
        );
        second.shutdown().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while endpoint_path(&preferences).unwrap().exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(!endpoint_path(&preferences).unwrap().exists());
        let _lock = config::lock(&root.join("hub-data")).unwrap();
        assert!(server.join("world/level.dat").is_file());
        assert!(
            !crate::backups::list(&root.join("hub-data/backups/existing"), "existing")
                .unwrap()
                .is_empty(),
            "Graceful engine shutdown completes the automatic world backup"
        );
    }
}
