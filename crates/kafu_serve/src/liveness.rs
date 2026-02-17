use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use kafu_config::KafuConfig;
use tokio::{
    sync::{broadcast, watch},
    task::JoinSet,
    time::{MissedTickBehavior, sleep},
};
use tonic::transport::Endpoint;

use crate::grpc::kafu_proto::HeartbeatRequest;
use crate::{
    cluster::request_cluster_shutdown_and_exit,
    constants::LEADER_EXECUTION_HEALTH_SERVICE,
    error::{KafuError, KafuResult},
    grpc,
    service::LeaderHeartbeatState,
};

pub async fn wait_for_other_nodes_healthy(
    node_id: &str,
    kafu_config: &KafuConfig,
) -> KafuResult<()> {
    // NOTE: Retry for a limited time to verify other nodes are up.
    // If a node becomes SERVING within the timeout, it's OK; otherwise fail fast.
    const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
    const INITIAL_BACKOFF: Duration = Duration::from_millis(250);
    const MAX_BACKOFF: Duration = Duration::from_secs(2);

    let mut join_set = JoinSet::new();

    for (other_node_id, node_config) in kafu_config.nodes.iter() {
        if other_node_id == node_id {
            continue;
        }
        let other_node_id = other_node_id.clone();
        let endpoint_str = format!("http://{}:{}", node_config.address, node_config.port);

        join_set.spawn(async move {
            let start = tokio::time::Instant::now();
            let mut backoff = INITIAL_BACKOFF;
            let mut last_reason: String;

            loop {
                let endpoint = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
                    KafuError::HealthCheckFailed {
                        node_id: other_node_id.clone(),
                        endpoint: endpoint_str.clone(),
                        reason: format!("invalid endpoint: {}", e),
                    }
                })?;

                match grpc::client::health_check(endpoint).await {
                    Ok(tonic_health::ServingStatus::Serving) => {
                        return Ok::<(), KafuError>(());
                    }
                    Ok(status) => {
                        last_reason = format!("status is {:?}", status);
                    }
                    Err(e) => {
                        last_reason = format!("{}", e);
                    }
                }

                if start.elapsed() >= TOTAL_TIMEOUT {
                    return Err(KafuError::HealthCheckFailed {
                        node_id: other_node_id.clone(),
                        endpoint: endpoint_str.clone(),
                        reason: last_reason,
                    });
                }

                sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
            }
        });
    }

    while let Some(r) = join_set.join_next().await {
        // Fail the whole startup as soon as any node fails the health check.
        match r {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(e) => {
                return Err(KafuError::HealthCheckFailed {
                    node_id: "(unknown)".to_string(),
                    endpoint: "(unknown)".to_string(),
                    reason: format!("health check task failed: {}", e),
                });
            }
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct PeerHeartbeatState {
    consecutive_failures: u32,
    first_failure_at: Option<tokio::time::Instant>,
    last_reason: Option<String>,
}

// Periodically health-check all peers and trigger a coordinated shutdown if any peer stays unhealthy.
pub async fn run_peer_heartbeat_monitor(
    node_id: String,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
) {
    // Heartbeat/health check policy (coordinator only; currently: first node).
    // Keep tuning constants in kafu_serve; behavior toggles live in kafu-config.
    let heartbeat_interval = Duration::from_millis(kafu_config.cluster.heartbeat.interval_ms);
    const RPC_TIMEOUT: Duration = Duration::from_secs(2);
    const FAILURES_TO_SHUTDOWN: u32 = 5;
    const FAILURE_MIN_DURATION: Duration = Duration::from_secs(5);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let mut ticker = tokio::time::interval(heartbeat_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut peer_states: HashMap<String, PeerHeartbeatState> = kafu_config
        .nodes
        .keys()
        .filter(|peer_id| *peer_id != &node_id)
        .map(|peer_id| (peer_id.clone(), PeerHeartbeatState::default()))
        .collect();

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::debug!("{}: Heartbeat monitor stopping due to shutdown", node_id);
                return;
            }
            _ = ticker.tick() => {}
        }

        if peer_states.is_empty() {
            continue;
        }

        let mut join_set: JoinSet<(
            String,
            String,
            Result<tonic_health::ServingStatus, KafuError>,
        )> = JoinSet::new();

        for (peer_id, node_config) in kafu_config.nodes.iter() {
            if *peer_id == node_id {
                continue;
            }

            let peer_id = peer_id.clone();
            let endpoint_str = format!("http://{}:{}", node_config.address, node_config.port);

            join_set.spawn(async move {
                let res = match Endpoint::from_shared(endpoint_str.clone()) {
                    Ok(ep) => {
                        let ep = ep.connect_timeout(RPC_TIMEOUT).timeout(RPC_TIMEOUT);
                        grpc::client::health_check(ep).await
                    }
                    Err(e) => Err(KafuError::HealthCheckFailed {
                        node_id: peer_id.clone(),
                        endpoint: endpoint_str.clone(),
                        reason: format!("invalid endpoint: {}", e),
                    }),
                };

                (peer_id, endpoint_str, res)
            });
        }

        let mut should_shutdown: Option<(String, String)> = None;

        while let Some(r) = join_set.join_next().await {
            match r {
                Ok((peer_id, endpoint_str, Ok(tonic_health::ServingStatus::Serving))) => {
                    if let Some(state) = peer_states.get_mut(&peer_id) {
                        if state.consecutive_failures > 0 {
                            tracing::debug!(
                                "{}: Peer {} ({}) recovered (was failing {}x)",
                                node_id,
                                peer_id,
                                endpoint_str,
                                state.consecutive_failures
                            );
                        }
                        *state = PeerHeartbeatState::default();
                    }
                }
                Ok((peer_id, endpoint_str, Ok(status))) => {
                    let reason = format!("status is {:?}", status);
                    let state = peer_states.entry(peer_id.clone()).or_default();
                    state.consecutive_failures += 1;
                    state.last_reason = Some(reason.clone());
                    state
                        .first_failure_at
                        .get_or_insert_with(tokio::time::Instant::now);

                    tracing::warn!(
                        "{}: Peer {} ({}) health check not SERVING ({}x): {}",
                        node_id,
                        peer_id,
                        endpoint_str,
                        state.consecutive_failures,
                        reason
                    );
                }
                Ok((peer_id, endpoint_str, Err(e))) => {
                    let reason = format!("{}", e);
                    let state = peer_states.entry(peer_id.clone()).or_default();
                    state.consecutive_failures += 1;
                    state.last_reason = Some(reason.clone());
                    state
                        .first_failure_at
                        .get_or_insert_with(tokio::time::Instant::now);

                    tracing::warn!(
                        "{}: Peer {} ({}) health check failed ({}x): {}",
                        node_id,
                        peer_id,
                        endpoint_str,
                        state.consecutive_failures,
                        reason
                    );
                }
                Err(e) => {
                    tracing::warn!("{}: Heartbeat check task failed: {}", node_id, e);
                }
            }
        }

        for (peer_id, state) in peer_states.iter() {
            if state.consecutive_failures < FAILURES_TO_SHUTDOWN {
                continue;
            }
            if let Some(first_failure_at) = state.first_failure_at {
                if first_failure_at.elapsed() < FAILURE_MIN_DURATION {
                    continue;
                }
            } else {
                continue;
            }

            let reason = state
                .last_reason
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            should_shutdown = Some((
                peer_id.clone(),
                format!(
                    "heartbeat failed for peer '{}' ({} consecutive failures over {:?}): {}",
                    peer_id, state.consecutive_failures, FAILURE_MIN_DURATION, reason
                ),
            ));
            break;
        }

        if let Some((_peer_id, reason)) = should_shutdown {
            tracing::error!("{}: {}", node_id, reason);
            request_cluster_shutdown_and_exit(
                &node_id,
                Arc::clone(&kafu_config),
                shutdown_tx,
                &reason,
            )
            .await;
            return;
        }
    }
}

#[allow(dead_code)]
// On a follower, detect coordinator loss via health checks and act based on configured policy.
pub async fn run_coordinator_loss_monitor(
    node_id: String,
    coordinator_id: String,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
) {
    let hb = &kafu_config.cluster.heartbeat;

    if matches!(
        hb.follower_on_coordinator_lost,
        kafu_config::FollowerOnCoordinatorLost::Ignore
    ) {
        return;
    }

    let heartbeat_interval = Duration::from_millis(kafu_config.cluster.heartbeat.interval_ms);
    const RPC_TIMEOUT: Duration = Duration::from_secs(2);
    const FAILURES_TO_SHUTDOWN: u32 = 5;
    const FAILURE_MIN_DURATION: Duration = Duration::from_secs(5);

    let Some(coord_cfg) = kafu_config.nodes.get(&coordinator_id) else {
        tracing::warn!(
            "{}: Coordinator '{}' not found in config; follower monitor disabled",
            node_id,
            coordinator_id
        );
        return;
    };

    let coordinator_endpoint_str = format!("http://{}:{}", coord_cfg.address, coord_cfg.port);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let mut ticker = tokio::time::interval(heartbeat_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut state = PeerHeartbeatState::default();
    let mut coordinator_execution_observed = false;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::debug!("{}: Coordinator-loss monitor stopping due to shutdown", node_id);
                return;
            }
            _ = ticker.tick() => {}
        }

        let res = match Endpoint::from_shared(coordinator_endpoint_str.clone()) {
            Ok(ep) => {
                let ep = ep.connect_timeout(RPC_TIMEOUT).timeout(RPC_TIMEOUT);
                grpc::client::health_check_service(ep, LEADER_EXECUTION_HEALTH_SERVICE).await
            }
            Err(e) => Err(KafuError::HealthCheckFailed {
                node_id: coordinator_id.clone(),
                endpoint: coordinator_endpoint_str.clone(),
                reason: format!("invalid endpoint: {}", e),
            }),
        };

        // Gate: do not start "loss" detection until the coordinator has started program execution.
        // This prevents followers from shutting down just because the coordinator hasn't started yet.
        if !coordinator_execution_observed {
            match &res {
                Ok(tonic_health::ServingStatus::Serving) => {
                    coordinator_execution_observed = true;
                    state = PeerHeartbeatState::default();
                    tracing::debug!(
                        "{}: Coordinator {} started program execution; enabling loss detection",
                        node_id,
                        coordinator_id
                    );
                }
                Ok(_) => {}
                Err(KafuError::GrpcClientError(status))
                    if status.code() == tonic::Code::NotFound => {}
                Err(_) => {}
            }
            continue;
        }

        match res {
            Ok(tonic_health::ServingStatus::Serving) => {
                if state.consecutive_failures > 0 {
                    tracing::debug!(
                        "{}: Coordinator {} recovered (was failing {}x)",
                        node_id,
                        coordinator_id,
                        state.consecutive_failures
                    );
                }
                state = PeerHeartbeatState::default();
            }
            Ok(status) => {
                let reason = format!("status is {:?}", status);
                state.consecutive_failures += 1;
                state.last_reason = Some(reason.clone());
                state
                    .first_failure_at
                    .get_or_insert_with(tokio::time::Instant::now);
                tracing::warn!(
                    "{}: Coordinator {} not SERVING ({}x): {}",
                    node_id,
                    coordinator_id,
                    state.consecutive_failures,
                    reason
                );
            }
            Err(e) => {
                let reason = format!("{}", e);
                state.consecutive_failures += 1;
                state.last_reason = Some(reason.clone());
                state
                    .first_failure_at
                    .get_or_insert_with(tokio::time::Instant::now);
                tracing::warn!(
                    "{}: Coordinator {} health check failed ({}x): {}",
                    node_id,
                    coordinator_id,
                    state.consecutive_failures,
                    reason
                );
            }
        }

        if state.consecutive_failures < FAILURES_TO_SHUTDOWN {
            continue;
        }
        let Some(first_failure_at) = state.first_failure_at else {
            continue;
        };
        if first_failure_at.elapsed() < FAILURE_MIN_DURATION {
            continue;
        }

        let reason = state
            .last_reason
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        tracing::error!(
            "{}: Coordinator '{}' unreachable ({} consecutive failures over {:?}): {}",
            node_id,
            coordinator_id,
            state.consecutive_failures,
            FAILURE_MIN_DURATION,
            reason
        );

        match hb.follower_on_coordinator_lost {
            kafu_config::FollowerOnCoordinatorLost::ShutdownSelf => {
                let _ = shutdown_tx.send(());
                return;
            }
            kafu_config::FollowerOnCoordinatorLost::Ignore => return,
        }
    }
}

// On the leader, periodically send heartbeat RPCs to all peers (best-effort; failures are logged).
pub async fn run_leader_heartbeat_sender(
    leader_id: String,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
    execution_started: Arc<AtomicBool>,
) {
    let heartbeat_interval = Duration::from_millis(kafu_config.cluster.heartbeat.interval_ms);
    const RPC_TIMEOUT: Duration = Duration::from_secs(2);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let mut ticker = tokio::time::interval(heartbeat_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::debug!("{}: Leader heartbeat sender stopping due to shutdown", leader_id);
                return;
            }
            _ = ticker.tick() => {}
        }

        let mut join_set: JoinSet<(String, String, Result<(), KafuError>)> = JoinSet::new();

        for (peer_id, node_config) in kafu_config.nodes.iter() {
            if *peer_id == leader_id {
                continue;
            }
            let peer_id = peer_id.clone();
            let endpoint_str = format!("http://{}:{}", node_config.address, node_config.port);
            let leader_id_ = leader_id.clone();
            let execution_started = execution_started.load(Ordering::Relaxed);

            join_set.spawn(async move {
                let req = HeartbeatRequest {
                    from_node_id: leader_id_.clone(),
                    execution_started,
                };

                let res = match Endpoint::from_shared(endpoint_str.clone()) {
                    Ok(ep) => {
                        let ep = ep.connect_timeout(RPC_TIMEOUT).timeout(RPC_TIMEOUT);
                        grpc::client::send_heartbeat(req, ep).await.map(|_r| ())
                    }
                    Err(e) => Err(KafuError::HealthCheckFailed {
                        node_id: peer_id.clone(),
                        endpoint: endpoint_str.clone(),
                        reason: format!("invalid endpoint: {}", e),
                    }),
                };

                (peer_id, endpoint_str, res)
            });
        }

        while let Some(r) = join_set.join_next().await {
            match r {
                Ok((_peer_id, _endpoint_str, Ok(()))) => {}
                Ok((peer_id, endpoint_str, Err(e))) => {
                    tracing::debug!(
                        "{}: Heartbeat to {} ({}) failed: {}",
                        leader_id,
                        peer_id,
                        endpoint_str,
                        e
                    );
                }
                Err(e) => {
                    tracing::debug!("{}: Heartbeat send task failed: {}", leader_id, e);
                }
            }
        }
    }
}

// On a follower, detect coordinator loss by observing push heartbeats.
// A follower does not send push heartbeats; only the coordinator does.
pub async fn run_coordinator_push_heartbeat_monitor(
    node_id: String,
    coordinator_id: String,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
    mut heartbeat_rx: watch::Receiver<LeaderHeartbeatState>,
) {
    let hb = &kafu_config.cluster.heartbeat;

    if matches!(
        hb.follower_on_coordinator_lost,
        kafu_config::FollowerOnCoordinatorLost::Ignore
    ) {
        return;
    }

    let heartbeat_interval = Duration::from_millis(kafu_config.cluster.heartbeat.interval_ms);
    const FAILURES_TO_SHUTDOWN: u32 = 5;
    const FAILURE_MIN_DURATION: Duration = Duration::from_secs(5);

    let mut shutdown_rx = shutdown_tx.subscribe();
    let mut ticker = tokio::time::interval(heartbeat_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut state = PeerHeartbeatState::default();
    let mut coordinator_execution_observed = false;

    let mut last_seen: Option<tokio::time::Instant> = None;
    let mut last_seen_checked: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::debug!("{}: Push-heartbeat monitor stopping due to shutdown", node_id);
                return;
            }
            changed = heartbeat_rx.changed() => {
                if changed.is_ok() {
                    let st = heartbeat_rx.borrow_and_update().clone();
                    if st.from_node_id == coordinator_id {
                        last_seen = Some(st.last_seen);
                        if st.execution_started && !coordinator_execution_observed {
                            coordinator_execution_observed = true;
                            state = PeerHeartbeatState::default();
                            last_seen_checked = None;
                            tracing::debug!(
                                "{}: Coordinator {} started program execution; enabling loss detection (push)",
                                node_id,
                                coordinator_id
                            );
                        } else if coordinator_execution_observed {
                            // Any heartbeat from coordinator after gating resets failures.
                            if state.consecutive_failures > 0 {
                                tracing::debug!(
                                    "{}: Coordinator {} heartbeat recovered (was failing {}x)",
                                    node_id,
                                    coordinator_id,
                                    state.consecutive_failures
                                );
                            }
                            state = PeerHeartbeatState::default();
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                if !coordinator_execution_observed {
                    continue;
                }

                let now_last_seen = last_seen;
                // Treat both "no heartbeat yet" and "no new heartbeat since last check" as failures.
                if now_last_seen.is_none() || now_last_seen == last_seen_checked {
                    state.consecutive_failures += 1;
                    state.first_failure_at.get_or_insert_with(tokio::time::Instant::now);
                } else {
                    // Heartbeat advanced since last check.
                    state = PeerHeartbeatState::default();
                    last_seen_checked = now_last_seen;
                    continue;
                }

                // Record a helpful reason.
                let reason = match now_last_seen {
                    Some(ls) => format!("no new heartbeat observed; last seen {:?} ago", ls.elapsed()),
                    None => "no heartbeat received from coordinator yet".to_string(),
                };
                state.last_reason = Some(reason.clone());

                tracing::warn!(
                    "{}: Coordinator {} push-heartbeat missing ({}x): {}",
                    node_id,
                    coordinator_id,
                    state.consecutive_failures,
                    reason
                );

                if state.consecutive_failures < FAILURES_TO_SHUTDOWN {
                    continue;
                }
                let Some(first_failure_at) = state.first_failure_at else {
                    continue;
                };
                if first_failure_at.elapsed() < FAILURE_MIN_DURATION {
                    continue;
                }

                let reason = state
                    .last_reason
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());

                tracing::error!(
                    "{}: Coordinator '{}' heartbeat missing ({} consecutive failures over {:?}): {}",
                    node_id,
                    coordinator_id,
                    state.consecutive_failures,
                    FAILURE_MIN_DURATION,
                    reason
                );

                match hb.follower_on_coordinator_lost {
                    kafu_config::FollowerOnCoordinatorLost::ShutdownSelf => {
                        let _ = shutdown_tx.send(());
                        return;
                    }
                    kafu_config::FollowerOnCoordinatorLost::Ignore => return,
                }
            }
        }
    }
}
