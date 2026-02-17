//! Test utilities for `kafu_serve`.
//!
//! NOTE: This module is part of the public API to support integration tests.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context as _;
use kafu_config::KafuConfig;
use kafu_runtime::engine::{
    KafuRuntimeConfig, KafuRuntimeInstance, LinkerConfig, WasiConfig, WasmModule,
};
use tokio::{sync::broadcast, task::JoinHandle};
use tonic::transport::Server;

use crate::{
    constants,
    grpc::kafu_proto::command_server::CommandServer,
    service::{KafuService, LeaderHeartbeatState},
};

/// Handle for a running test gRPC server.
pub struct TestServerHandle {
    addr: SocketAddr,
    shutdown_tx: broadcast::Sender<()>,
    join: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestServerHandle {
    /// Returns the server's bound socket address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns the gRPC endpoint URL (e.g. `http://127.0.0.1:12345`).
    pub fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Requests server shutdown and waits for it to stop.
    pub async fn shutdown(self) -> anyhow::Result<()> {
        let _ = self.shutdown_tx.send(());
        self.join
            .await
            .context("gRPC server task join failed")?
            .context("gRPC server exited with error")?;
        Ok(())
    }
}

/// Starts a gRPC server bound to `127.0.0.1:0` (ephemeral port) for tests.
///
/// Returns a handle and a watch receiver that observes leader heartbeat state changes.
pub async fn start_test_grpc_server(
    node_id: &str,
    kafu_config: Arc<KafuConfig>,
    wasm_module: Arc<WasmModule>,
    wasm_sha256: [u8; 32],
) -> anyhow::Result<(
    TestServerHandle,
    tokio::sync::watch::Receiver<LeaderHeartbeatState>,
)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind test listener")?;
    let addr = listener.local_addr().context("failed to get local_addr")?;

    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(16);
    let (leader_heartbeat_tx, leader_heartbeat_rx) =
        tokio::sync::watch::channel(LeaderHeartbeatState::default());

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<CommandServer<KafuService>>()
        .await;
    health_reporter
        .set_service_status(
            constants::LEADER_EXECUTION_HEALTH_SERVICE,
            tonic_health::ServingStatus::NotServing,
        )
        .await;

    let runtime_config = KafuRuntimeConfig {
        node_id: node_id.to_string(),
        wasi_config: WasiConfig::create_from_kafu_config(&kafu_config),
        linker_config: LinkerConfig::default(),
    };
    let instance = KafuRuntimeInstance::new(Arc::clone(&wasm_module), &runtime_config)
        .await
        .context("failed to create runtime instance")?;
    let runtime = Arc::new(tokio::sync::Mutex::new(instance));

    let snapshot_buffers = std::sync::Arc::new(std::sync::Mutex::new((Vec::new(), Vec::new())));
    let service = KafuService::new(
        node_id,
        Arc::clone(&kafu_config),
        runtime,
        shutdown_tx.clone(),
        leader_heartbeat_tx,
        snapshot_buffers,
        wasm_sha256,
    );

    // Graceful shutdown is driven by a broadcast signal.
    let mut server_shutdown_rx = shutdown_tx.subscribe();

    // Tonic needs an incoming stream so we can pre-bind to port 0 and still learn the actual port.
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let join: JoinHandle<Result<(), tonic::transport::Error>> = tokio::spawn(async move {
        Server::builder()
            .add_service(health_service)
            .add_service(
                CommandServer::new(service)
                    .max_decoding_message_size(constants::MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(constants::MAX_MESSAGE_SIZE),
            )
            .serve_with_incoming_shutdown(incoming, async move {
                // Wait for shutdown signal; ignore errors (e.g. sender dropped).
                let _ = server_shutdown_rx.recv().await;
            })
            .await
    });

    // Give the accept loop a moment to start to reduce flakiness on very slow CI.
    tokio::time::sleep(Duration::from_millis(10)).await;

    Ok((
        TestServerHandle {
            addr,
            shutdown_tx,
            join,
        },
        leader_heartbeat_rx,
    ))
}
