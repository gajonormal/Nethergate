use crate::hub::{self, Hub};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::{Component, PathBuf},
    sync::Arc,
};
use tiny_http::{Header, Method, Request, Response, Server};

#[derive(Deserialize)]
struct Call {
    action: String,
    #[serde(default)]
    payload: serde_json::Value,
}

pub fn serve(root: PathBuf, port: u16) -> Result<()> {
    let root = root.canonicalize()?;
    let hub = Hub::new(root.clone())?;
    run(hub, root.join("dist"), port)?;
    Ok(())
}

pub fn run(hub: Arc<Hub>, assets: PathBuf, port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("Não foi possível abrir o hub: {e}"))?;
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("Falha ao gerar sessão: {e}"))?;
    let token = Arc::new(
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    println!("Nethergate Hub: http://127.0.0.1:{port}");
    println!("Acesso apenas local. Para os servidores no hub antes de terminar este processo.");
    for request in server.incoming_requests() {
        let hub = hub.clone();
        let assets = assets.clone();
        let token = token.clone();
        std::thread::spawn(move || handle(request, hub, assets, &token, port));
    }
    Ok(())
}

fn header(request: &Request, key: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(key))
        .map(|h| h.value.as_str().to_string())
}

fn handle(mut request: Request, hub: Arc<Hub>, assets: PathBuf, token: &str, port: u16) {
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://{host}");
    let valid_origin =
        header(&request, "Origin").is_none_or(|o| o == origin || o == "http://127.0.0.1:5173");
    if header(&request, "Host").as_deref() != Some(&host) || !valid_origin {
        respond(
            request,
            403,
            "application/json",
            b"{\"error\":\"Origem recusada\"}".to_vec(),
        );
        return;
    }
    let path = request.url().split('?').next().unwrap_or("/").to_string();
    if path == "/api/session"
        && request.method() == &Method::Get
        && header(&request, "X-NetherGate-Client").as_deref() == Some("hub")
    {
        respond(
            request,
            200,
            "application/json",
            serde_json::json!({"token": token}).to_string().into_bytes(),
        );
        return;
    }
    if path == "/api/call" && request.method() == &Method::Post {
        if header(&request, "X-NetherGate-Token").as_deref() != Some(token) {
            respond(
                request,
                403,
                "application/json",
                b"{\"error\":\"Sessao invalida\"}".to_vec(),
            );
            return;
        }
        let result = (|| -> Result<serde_json::Value> {
            let mut body = String::new();
            request.as_reader().take(65537).read_to_string(&mut body)?;
            anyhow::ensure!(body.len() <= 65536, "Pedido demasiado grande");
            let call: Call = serde_json::from_str(&body).context("Pedido inválido")?;
            hub::dispatch(&hub, &call.action, call.payload)
        })();
        match result {
            Ok(value) => respond(
                request,
                200,
                "application/json",
                value.to_string().into_bytes(),
            ),
            Err(error) => respond(
                request,
                400,
                "application/json",
                serde_json::json!({"error": format!("{error:#}")})
                    .to_string()
                    .into_bytes(),
            ),
        }
        return;
    }
    if request.method() != &Method::Get || path.starts_with("/api/") {
        respond(request, 404, "text/plain", b"Not found".to_vec());
        return;
    }
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if PathBuf::from(relative)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || relative.contains('\\')
    {
        respond(request, 403, "text/plain", b"Forbidden".to_vec());
        return;
    }
    let file = assets.join(relative);
    let mime = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "js" => "application/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    };
    match fs::read(file) {
        Ok(bytes) => respond(request, 200, mime, bytes),
        Err(_) => respond(
            request,
            404,
            "text/plain",
            b"Interface nao encontrada. Executa npm run build.".to_vec(),
        ),
    }
}

fn respond(request: Request, status: u16, mime: &str, body: Vec<u8>) {
    let mut response = Response::from_data(body).with_status_code(status);
    for (key, value) in [("Content-Type", mime), ("Cache-Control", "no-store"), ("X-Frame-Options", "DENY"), ("X-Content-Type-Options", "nosniff"),
        ("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'")] {
        match Header::from_bytes(key, value) {
            Ok(header) => response.add_header(header),
            Err(_) => {
                let _ = request.respond(tiny_http::Response::empty(500));
                return;
            }
        }
    }
    let _ = request.respond(response);
}
