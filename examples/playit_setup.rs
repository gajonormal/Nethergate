//! Development helper for the same setup used by the desktop UI. Never prints a secret.
fn main() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let id = "existing";
    if std::env::args().any(|a| a == "--test-session") {
        let access = nethergated::players::access(&root.join("server"))?;
        anyhow::ensure!(
            access.authenticated && access.enabled && !access.players.is_empty(),
            "Ativa autenticação e whitelist antes de testar"
        );
        let cfg = nethergated::config::Config::load(&root.join("server"))?;
        let target = nethergated::connection::endpoint(
            nethergated::connection::bind_ip(&root.join("server"))?,
            cfg.port,
        );
        nethergated::playit::validate(&root, id, target)?;
        let mut sharing = nethergated::sharing::Sharing::default();
        sharing.tunnel(&root, id, target)?;
        let started = std::time::Instant::now();
        loop {
            let state = sharing.status(id);
            if state["state"] == "tunnel" {
                println!("{}", state);
                break;
            }
            anyhow::ensure!(
                state["state"] == "connecting" && started.elapsed().as_secs() < 95,
                "Agente sem prontidão: {}",
                state["detail"]
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        std::thread::sleep(std::time::Duration::from_secs(45));
        sharing.stop(id);
        anyhow::ensure!(sharing.status(id)["address"].is_null());
        println!("Túnel terminado; endereço guardado.");
        return Ok(());
    }
    let value = if std::env::args().any(|a| a == "--confirm") {
        let cfg = nethergated::config::Config::load(&root.join("server"))?;
        let bind = nethergated::connection::bind_ip(&root.join("server"))?;
        nethergated::playit::confirm(&root, id, nethergated::connection::endpoint(bind, cfg.port))?
    } else {
        nethergated::playit::prepare(&root, id)?
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
