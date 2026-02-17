use std::sync::Arc;

use kafu_config::KafuConfig;
use tokio::{sync::broadcast, task::JoinSet};
use tonic::transport::Endpoint;

use crate::{grpc, grpc::kafu_proto::ShutdownRequest};

pub async fn request_cluster_shutdown_and_exit(
    node_id: &str,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
    reason: &str,
) {
    tracing::debug!(
        "{}: Requesting cluster shutdown (reason: {:?})",
        node_id,
        reason
    );

    let mut join_set = JoinSet::new();

    for (other_node_id, node_config) in kafu_config.nodes.iter() {
        if other_node_id == node_id {
            continue;
        }

        let endpoint = match Endpoint::from_shared(format!(
            "http://{}:{}",
            node_config.address, node_config.port
        )) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                tracing::warn!("{}: Invalid endpoint for {}: {}", node_id, other_node_id, e);
                continue;
            }
        };

        let request = ShutdownRequest {
            from_node_id: node_id.to_string(),
            reason: reason.to_string(),
        };
        let other_node_id = other_node_id.clone();

        join_set.spawn(async move {
            let res = grpc::client::send_shutdown_request(request, endpoint).await;
            (other_node_id, res)
        });
    }

    while let Some(r) = join_set.join_next().await {
        match r {
            Ok((other_node_id, Ok(res))) => {
                tracing::debug!(
                    "{}: Shutdown request response from {} (accepted: {})",
                    node_id,
                    other_node_id,
                    res.accepted
                );
            }
            Ok((other_node_id, Err(e))) => {
                tracing::warn!(
                    "{}: Failed to send shutdown request to {}: {}",
                    node_id,
                    other_node_id,
                    e
                );
            }
            Err(e) => {
                tracing::warn!("{}: Shutdown request task failed: {}", node_id, e);
            }
        }
    }

    let _ = shutdown_tx.send(());
    tracing::debug!("{}: Shutdown signal sent", node_id);
}
