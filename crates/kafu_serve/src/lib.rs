//! Library interface for `kafu_serve`.
//!
//! This crate is primarily a binary, but exposing a small library surface makes it easier
//! to write integration tests without spawning an external process.

mod cli;
mod cluster;
mod constants;
mod error;
mod grpc;
mod liveness;
mod migration;
mod runtime;
mod service;

mod testing;

// Public surface: keep it minimal and intentional.
pub use grpc::client::{health_check, health_check_service, send_heartbeat, send_shutdown_request};
pub use grpc::kafu_proto::{HeartbeatRequest, ShutdownRequest};
pub use service::LeaderHeartbeatState;
pub use testing::{TestServerHandle, start_test_grpc_server};

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{cli::Cli, error::KafuError};
use clap::Parser as _;
use grpc::kafu_proto::command_server::CommandServer;
use kafu_config::{KafuConfig, WasmLocation};
use kafu_runtime::engine::{
    KafuRuntimeConfig, KafuRuntimeInstance, LinkerConfig, WasiConfig, WasmModule,
};
use sha2::{Digest, Sha256};
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};
use tonic::transport::Server;
use tonic_health::pb::health_server::{Health, HealthServer};

fn load_kafu_config(cli: &Cli) -> Result<Arc<KafuConfig>, KafuError> {
    let kafu_config = KafuConfig::load(&cli.config).map_err(KafuError::InvalidConfig)?;
    Ok(Arc::new(kafu_config))
}

fn get_first_node_id(kafu_config: &KafuConfig) -> Result<String, KafuError> {
    kafu_config
        .nodes
        .first()
        .map(|(node_id, _)| node_id.clone())
        .ok_or_else(|| KafuError::InvalidConfig("No nodes found in the config".to_string()))
}

fn resolve_node_config<'a>(
    kafu_config: &'a KafuConfig,
    node_id: &str,
) -> Result<&'a kafu_config::NodeConfig, KafuError> {
    kafu_config.nodes.get(node_id).ok_or_else(|| {
        KafuError::InvalidConfig(format!("Node {} not found in the config", node_id))
    })
}

fn make_bind_address(node_id: &str, port: u16) -> Result<SocketAddr, KafuError> {
    format!("0.0.0.0:{port}")
        .parse()
        .map_err(|e| KafuError::InvalidConfig(format!("Invalid port for node {node_id}: {e}")))
}

fn make_runtime_config(node_id: &str, kafu_config: &KafuConfig) -> Arc<KafuRuntimeConfig> {
    Arc::new(KafuRuntimeConfig {
        node_id: node_id.to_string(),
        wasi_config: WasiConfig::create_from_kafu_config(kafu_config),
        linker_config: LinkerConfig::default(),
    })
}

async fn load_wasm_binary(kafu_config: &KafuConfig) -> Result<Vec<u8>, KafuError> {
    let wasm_location = kafu_config.get_wasm_location();
    match wasm_location {
        WasmLocation::Path(path) => std::fs::read(path).map_err(|e| {
            KafuError::WasmInstantiationError(anyhow::anyhow!("Failed to read wasm file: {e}"))
        }),
        WasmLocation::Url(url) => {
            tracing::info!("Fetching WASM module from URL: {}", url);
            let bytes = reqwest::get(url)
                .await
                .map_err(|e| KafuError::WasmInstantiationError(anyhow::anyhow!("{e}")))?
                .error_for_status()
                .map_err(|e| KafuError::WasmInstantiationError(anyhow::anyhow!("{e}")))?
                .bytes()
                .await
                .map_err(|e| KafuError::WasmInstantiationError(anyhow::anyhow!("{e}")))?;
            Ok(bytes.to_vec())
        }
    }
}

async fn build_wasm_module(wasm_binary: Vec<u8>) -> Result<Arc<WasmModule>, KafuError> {
    let wasm = WasmModule::new(wasm_binary)
        .await
        .map_err(KafuError::WasmInstantiationError)?;
    Ok(Arc::new(wasm))
}

async fn create_runtime_instance(
    wasm_module: Arc<WasmModule>,
    runtime_config: Arc<KafuRuntimeConfig>,
) -> Result<Arc<tokio::sync::Mutex<KafuRuntimeInstance>>, KafuError> {
    let instance = KafuRuntimeInstance::new(wasm_module, &runtime_config)
        .await
        .map_err(|e| KafuError::WasmInstantiationError(anyhow::anyhow!("{e}")))?;
    Ok(Arc::new(tokio::sync::Mutex::new(instance)))
}

async fn init_health_services() -> (
    tonic_health::server::HealthReporter,
    HealthServer<impl Health>,
) {
    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<CommandServer<service::KafuService>>()
        .await;
    // Register coordinator execution status as NOT_SERVING initially.
    health_reporter
        .set_service_status(
            constants::LEADER_EXECUTION_HEALTH_SERVICE,
            tonic_health::ServingStatus::NotServing,
        )
        .await;
    (health_reporter, health_service)
}

fn spawn_grpc_server(
    bind_address: SocketAddr,
    health_service: HealthServer<impl Health>,
    kafu_service: service::KafuService,
    shutdown_tx: broadcast::Sender<()>,
) -> JoinHandle<Result<(), tonic::transport::Error>> {
    let mut server_shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        Server::builder()
            .add_service(health_service)
            .add_service(
                CommandServer::new(kafu_service)
                    .max_decoding_message_size(constants::MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(constants::MAX_MESSAGE_SIZE),
            )
            .serve_with_shutdown(bind_address, async move {
                let _ = server_shutdown_rx.recv().await;
            })
            .await
    })
}

fn log_start_mode(node_id: &str, should_start: bool) {
    if should_start {
        tracing::debug!("{}: Starting as the first node (start mode)", node_id);
    } else {
        tracing::debug!("{}: Starting in wait mode", node_id);
    }
}

struct LeaderTasksArgs {
    node_id: String,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
    health_reporter: tonic_health::server::HealthReporter,
    runtime: Arc<tokio::sync::Mutex<KafuRuntimeInstance>>,
    snapshot_cache: crate::service::SnapshotCache,
    snapshot_buffers: Arc<Mutex<(Vec<u8>, Vec<u8>)>>,
    wasm_sha256: [u8; 32],
}

async fn start_leader_tasks(args: LeaderTasksArgs) {
    let LeaderTasksArgs {
        node_id,
        kafu_config,
        shutdown_tx,
        health_reporter,
        runtime,
        snapshot_cache,
        snapshot_buffers,
        wasm_sha256,
    } = args;
    let followers_expect_push_heartbeat = matches!(
        kafu_config.cluster.heartbeat.follower_on_coordinator_lost,
        kafu_config::FollowerOnCoordinatorLost::ShutdownSelf
    );

    // Start push heartbeats (leader -> followers) only if followers care.
    let execution_started = Arc::new(AtomicBool::new(false));
    if followers_expect_push_heartbeat {
        tokio::spawn(liveness::run_leader_heartbeat_sender(
            node_id.clone(),
            Arc::clone(&kafu_config),
            shutdown_tx.clone(),
            Arc::clone(&execution_started),
        ));
    }

    let _handle: JoinHandle<Result<(), KafuError>> = tokio::task::spawn(async move {
        // Mark coordinator execution as started (external signal).
        health_reporter
            .set_service_status(
                constants::LEADER_EXECUTION_HEALTH_SERVICE,
                tonic_health::ServingStatus::Serving,
            )
            .await;
        execution_started.store(true, Ordering::Relaxed);

        // Start periodic peer health monitoring after execution has started.
        // We tie this to follower_on_coordinator_lost to avoid configuration divergence.
        if matches!(
            kafu_config.cluster.heartbeat.follower_on_coordinator_lost,
            kafu_config::FollowerOnCoordinatorLost::ShutdownSelf
        ) {
            tokio::spawn(liveness::run_peer_heartbeat_monitor(
                node_id.clone(),
                Arc::clone(&kafu_config),
                shutdown_tx.clone(),
            ));
        }

        // Call the `_start` function in the WASM module.
        {
            let mut instance = runtime.lock().await;
            instance
                .start()
                .await
                .map_err(KafuError::WasmExecutionError)?;
        }

        runtime::handle_pending_migration_or_shutdown(
            &runtime,
            node_id.as_str(),
            kafu_config,
            shutdown_tx,
            snapshot_cache,
            snapshot_buffers,
            &wasm_sha256,
        )
        .await?;

        Ok(())
    });
}

/// Runs the `kafu_serve` binary logic.
///
/// Keeping the main logic in the library allows integration tests to call it directly,
/// while the actual binary (`main.rs`) can stay as a thin wrapper without `mod` declarations.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let kafu_config = load_kafu_config(&cli)?;
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(16);
    let (leader_heartbeat_tx, leader_heartbeat_rx) =
        watch::channel(service::LeaderHeartbeatState::default());

    // Get the first node ID from the config
    let first_node_id = get_first_node_id(&kafu_config)?;

    // Determine if this node should start (first node) or wait (other nodes)
    let should_start = cli.node_id == first_node_id;

    let runtime_config = make_runtime_config(&cli.node_id, &kafu_config);
    let wasm_binary = load_wasm_binary(&kafu_config).await?;
    let wasm_sha256: [u8; 32] = Sha256::digest(&wasm_binary).into();
    let wasm_module = build_wasm_module(wasm_binary).await?;

    let node_id = &cli.node_id;
    let node_config = resolve_node_config(&kafu_config, node_id)?;

    tracing::debug!("Loaded configuration: {:?}", kafu_config);
    log_start_mode(node_id, should_start);

    let bind_address = make_bind_address(node_id, node_config.port)?;
    tracing::info!("{}: Server is listening on {}:", node_id, bind_address);

    // Create runtime for all nodes (leader runs start(); followers receive restore() on migrate).
    let runtime = create_runtime_instance(Arc::clone(&wasm_module), runtime_config).await?;
    let (health_reporter, health_service) = init_health_services().await;

    let snapshot_buffers = Arc::new(Mutex::new((Vec::new(), Vec::new())));
    let kafu_service = service::KafuService::new(
        node_id,
        Arc::clone(&kafu_config),
        Arc::clone(&runtime),
        shutdown_tx.clone(),
        leader_heartbeat_tx,
        snapshot_buffers.clone(),
        wasm_sha256,
    );
    let snapshot_cache = Arc::clone(&kafu_service.snapshot_cache);

    // Start the gRPC server first; on the first node, wait for peers before starting WASM.
    let server_handle = spawn_grpc_server(
        bind_address,
        health_service,
        kafu_service,
        shutdown_tx.clone(),
    );

    if should_start {
        tracing::info!("{}: Waiting for other nodes to become healthy", node_id);
        if let Err(e) = liveness::wait_for_other_nodes_healthy(node_id, &kafu_config).await {
            tracing::error!("{}: {}", node_id, e);
            let _ = shutdown_tx.send(());
            // Wait for the server to stop, then exit with an error.
            let _ = server_handle.await;
            return Err(Box::new(e) as Box<dyn std::error::Error>);
        }
        tracing::info!("{}: All other nodes are healthy", node_id);
        start_leader_tasks(LeaderTasksArgs {
            node_id: node_id.clone(),
            kafu_config: Arc::clone(&kafu_config),
            shutdown_tx: shutdown_tx.clone(),
            health_reporter: health_reporter.clone(),
            runtime: Arc::clone(&runtime),
            snapshot_cache: Arc::clone(&snapshot_cache),
            snapshot_buffers: Arc::clone(&snapshot_buffers),
            wasm_sha256,
        })
        .await;
    }

    // On non-coordinator nodes, optionally monitor the coordinator and shut down on loss.
    if !should_start {
        tokio::spawn(liveness::run_coordinator_push_heartbeat_monitor(
            cli.node_id.clone(),
            first_node_id.clone(),
            Arc::clone(&kafu_config),
            shutdown_tx.clone(),
            leader_heartbeat_rx.clone(),
        ));
    }

    // Wait for server termination (shutdown or error).
    server_handle.await??;
    Ok(())
}
