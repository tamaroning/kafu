use std::{sync::Arc, time::Duration};

use kafu_config::KafuConfig;
use kafu_runtime::engine::WasmModule;
use sha2::{Digest as _, Sha256};
use tonic::transport::Endpoint;

use kafu_serve::{
    HeartbeatRequest, LeaderHeartbeatState, ShutdownRequest, TestServerHandle, health_check,
    send_heartbeat, send_shutdown_request, start_test_grpc_server,
};

fn write_test_config_yaml(wasm_filename: &str, port: u16) -> String {
    // Keep the config minimal; only fields required by KafuConfig validation are included.
    format!(
        r#"
name: "test-service"
app:
  path: "{wasm_filename}"
  args: []
  preopened_dir: null
nodes:
  node-1:
    address: "127.0.0.1"
    port: {port}
cluster:
  heartbeat:
    follower_on_coordinator_lost: ignore
    interval_ms: 1000
"#
    )
}

async fn setup_test_server() -> anyhow::Result<(
    TestServerHandle,
    tokio::sync::watch::Receiver<LeaderHeartbeatState>,
    Endpoint,
)> {
    let td = tempfile::tempdir()?;

    // Use an existing wasm fixture from the repository and place it next to the config.
    let wasm_wat = include_str!("fixtures/minimal.wat");
    let wasm_bytes = wat::parse_str(wasm_wat)?.to_vec();
    let wasm_filename = "program.wasm";
    let wasm_path = td.path().join(wasm_filename);
    std::fs::write(&wasm_path, &wasm_bytes)?;

    let cfg_path = td.path().join("kafu-config.yaml");
    std::fs::write(&cfg_path, write_test_config_yaml(wasm_filename, 0))?;

    let kafu_config = Arc::new(
        KafuConfig::load(&cfg_path)
            .map_err(|e| anyhow::anyhow!("failed to load test config: {}", e))?,
    );

    let wasm_sha256: [u8; 32] = Sha256::digest(&wasm_bytes).into();
    let wasm_module = Arc::new(WasmModule::new(wasm_bytes).await?);

    let (server, leader_hb_rx) =
        start_test_grpc_server("node-1", Arc::clone(&kafu_config), wasm_module, wasm_sha256)
            .await?;

    let endpoint = Endpoint::from_shared(server.endpoint())?
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(1));

    Ok((server, leader_hb_rx, endpoint))
}

async fn wait_until_serving(endpoint: Endpoint) -> anyhow::Result<()> {
    const TOTAL: Duration = Duration::from_secs(2);
    const STEP: Duration = Duration::from_millis(25);

    let start = tokio::time::Instant::now();
    loop {
        match health_check(endpoint.clone()).await {
            Ok(tonic_health::ServingStatus::Serving) => return Ok(()),
            Ok(_status) => {}
            Err(_e) => {}
        }

        if start.elapsed() >= TOTAL {
            anyhow::bail!("server did not become SERVING within {:?}", TOTAL);
        }
        tokio::time::sleep(STEP).await;
    }
}

// This integration test exercises the in-process gRPC server end-to-end:
// - waits for health to become SERVING,
// - verifies `Heartbeat` updates the shared leader heartbeat watch state,
// - verifies `Shutdown` is accepted and the server can terminate gracefully.
#[tokio::test]
async fn heartbeat_updates_state_and_shutdown_stops_server() -> anyhow::Result<()> {
    let (server, mut leader_hb_rx, endpoint) = setup_test_server().await?;

    wait_until_serving(endpoint.clone()).await?;

    // 1) Heartbeat updates the shared leader heartbeat state.
    let hb = HeartbeatRequest {
        from_node_id: "leader-1".to_string(),
        execution_started: true,
    };
    let resp = send_heartbeat(hb, endpoint.clone()).await?;
    assert!(resp.accepted);

    leader_hb_rx.changed().await?;
    let st = leader_hb_rx.borrow().clone();
    assert_eq!(st.from_node_id, "leader-1");
    assert!(st.execution_started);

    // 2) Shutdown request stops the server.
    let shutdown = ShutdownRequest {
        from_node_id: "tester".to_string(),
        reason: "integration-test".to_string(),
    };
    let resp = send_shutdown_request(shutdown, endpoint).await?;
    assert!(resp.accepted);

    server.shutdown().await?;
    Ok(())
}
