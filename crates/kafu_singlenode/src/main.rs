use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::{Parser, Subcommand};
use kafu_config::{KafuConfig, WasmLocation};

use kafu_runtime::engine::{
    KafuRuntimeConfig, KafuRuntimeInstance, LinkerConfig, LinkerSnapifyConfig, WasiConfig,
    WasmModule,
};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};

/// Run a Kafu service on a single node by emulating a multi-node environment.
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        /// Path to the Kafu config file.
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "kafu_runtime=info,kafu_singlenode=info") };
    }

    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false))
        .with(EnvFilter::from_default_env());
    subscriber.try_init()?;

    let cli = Cli::parse();

    match cli.command {
        Command::Serve { config } => {
            let kafu_config = KafuConfig::load(&config)
                .map_err(|e| anyhow::anyhow!("Failed to load Kafu config: {}", e))?;
            run_service(&kafu_config).await
        }
    }
}

async fn load_wasm_binary(config: &KafuConfig) -> Result<Vec<u8>> {
    let wasm_location = config.get_wasm_location();
    match wasm_location {
        WasmLocation::Path(path) => std::fs::read(path).map_err(anyhow::Error::from),
        WasmLocation::Url(url) => {
            let bytes = reqwest::get(url)
                .await?
                .error_for_status()?
                .bytes()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch WASM binary: {}", e))?;
            Ok(bytes.to_vec())
        }
    }
}

async fn run_service(config: &KafuConfig) -> Result<()> {
    let wasm = load_wasm_binary(config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load WASM binary: {}", e))?;

    let start_node_id = config.nodes.keys().next().unwrap().clone();

    let runtime_config = KafuRuntimeConfig {
        node_id: start_node_id,
        wasi_config: WasiConfig::create_from_kafu_config(config),
        linker_config: LinkerConfig {
            wasip1: true,
            wasi_nn: true,
            spectest: true,
            kafu_helper: true,
            snapify: LinkerSnapifyConfig::Dummy,
        },
    };

    let wasm = WasmModule::new(wasm)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load WASM module: {}", e))?;

    tracing::info!("Program is starting on {}", runtime_config.node_id);
    let mut runtime = KafuRuntimeInstance::new(Arc::new(wasm), &runtime_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create runtime instance: {}", e))?;

    runtime
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute WASM module: {}", e))?;

    let store = runtime.get_store();
    tracing::info!("Program finished on {}", store.data().get_node_id());

    Ok(())
}
