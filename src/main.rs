#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use nethergated::{
    config::{self, Config},
    paper, process,
};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(version, about = "Nethergate — gestor local de servidores PaperMC")]
struct Cli {
    #[arg(long, global = true, default_value = "server")]
    directory: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Executa o motor independente, exclusivamente local (Windows).
    Engine {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        preferences: PathBuf,
    },
    /// Consulta um motor já em execução sem iniciar outro.
    EngineStatus {
        #[arg(long)]
        preferences: PathBuf,
    },
    /// Guarda, pára os servidores e termina o motor local.
    EngineStop {
        #[arg(long)]
        preferences: PathBuf,
    },
    /// Executa uma operação autenticada no motor local existente.
    EngineCall {
        #[arg(long)]
        preferences: PathBuf,
        action: String,
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    /// Abre a API e o hub web exclusivamente em loopback.
    Hub {
        #[arg(long, default_value_t = 17843)]
        port: u16,
    },
    /// Cria uma configuração; nunca substitui uma configuração existente.
    Init {
        #[arg(long,default_value="paper",value_parser=["paper","fabric"])]
        software: String,
        #[arg(long)]
        minecraft: String,
        /// Email ou URL de contacto real, exigido pelo PaperMC.
        #[arg(long)]
        contact: String,
        /// Java mínimo exigido pela versão de Minecraft escolhida.
        #[arg(long, default_value_t = 21)]
        java_major: u32,
    },
    /// Verifica a configuração e o Java sem iniciar o servidor.
    Doctor,
    /// Descarrega o build estável mais recente da versão configurada.
    Install,
    /// Executa Java em primeiro plano; escreve `stop` na consola para terminar.
    Start {
        /// Confirma que leste e aceitas https://aka.ms/MinecraftEULA.
        #[arg(long)]
        accept_eula: bool,
    },
    /// Altera uma propriedade preservando as restantes e os comentários.
    Set { key: String, value: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Engine { root, preferences } => {
            let _guard = nethergated::daemon::process_guard()?;
            let result = nethergated::daemon::serve(root.clone(), preferences.clone());
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
        Commands::EngineStatus { preferences } => {
            println!(
                "{}",
                nethergated::daemon::Client::connect(preferences)?.info()?
            );
            return Ok(());
        }
        Commands::EngineStop { preferences } => {
            println!(
                "{}",
                nethergated::daemon::Client::connect(preferences)?.shutdown()?
            );
            return Ok(());
        }
        Commands::EngineCall {
            preferences,
            action,
            payload,
        } => {
            println!(
                "{}",
                nethergated::daemon::Client::connect(preferences)?
                    .dispatch(action, serde_json::from_str(payload)?)?
            );
            return Ok(());
        }
        _ => {}
    }
    if let Commands::Hub { port } = cli.command {
        return nethergated::web::serve(std::env::current_dir()?, port);
    }
    if let Commands::Init {
        software,
        minecraft,
        contact,
        java_major,
    } = &cli.command
    {
        let mut config = Config::new(minecraft.clone(), contact.clone(), *java_major)?;
        config.software = if software == "fabric" {
            config::Software::Fabric
        } else {
            config::Software::Paper
        };
        fs::create_dir_all(&cli.directory)?;
        let _guard = config::lock(&cli.directory)?;
        let path = cli.directory.join("nethergate.json");
        if path.exists() {
            bail!("Já existe uma configuração em {}", path.display());
        }
        config::write_json(&path, &config)?;
        println!("Configuração criada em {}", path.display());
        return Ok(());
    }
    let directory = cli
        .directory
        .canonicalize()
        .context("Executa primeiro `nethergated init`")?;
    let _guard = config::lock(&directory)?;
    let config = Config::load(&directory)?;
    match cli.command {
        Commands::Engine { .. }
        | Commands::EngineStatus { .. }
        | Commands::EngineStop { .. }
        | Commands::EngineCall { .. } => {
            bail!("O comando já deveria ter sido tratado antes de carregar o servidor")
        }
        Commands::Doctor => {
            process::check_java(&config)?;
            println!(
                "Configuração válida. Java disponível. Porta configurada: {}",
                config.port
            );
            println!("A acessibilidade externa ainda não é verificada neste MVP.");
        }
        Commands::Install => match config.software {
            config::Software::Paper => paper::install(&directory, &config)?,
            config::Software::Fabric => nethergated::fabric::install(&directory, &config)?,
        },
        Commands::Start { accept_eula } => process::start(&directory, &config, accept_eula)?,
        Commands::Set { key, value } => {
            config::set_property(&directory.join("server.properties"), &key, &value)?
        }
        Commands::Init { .. } => {
            bail!("O comando já deveria ter sido tratado antes de carregar o servidor")
        }
        Commands::Hub { .. } => {
            bail!("O comando já deveria ter sido tratado antes de carregar o servidor")
        }
    }
    Ok(())
}
