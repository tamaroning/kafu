use std::sync::{Arc, Mutex};

use kafu_config::KafuConfig;
use kafu_runtime::engine::KafuRuntimeInstance;
use tokio::sync::{Mutex as TokioMutex, broadcast};

use crate::{cluster, error::KafuResult, migration, service::SnapshotCache};

/// Reusable (main, snapify) buffers for checkpoint; avoids allocating on each migration send.
pub type SnapshotBuffers = Arc<Mutex<(Vec<u8>, Vec<u8>)>>;

/// After runtime execution/restore, either:
/// - send a pending migration request, or
/// - request cluster shutdown (program finished).
pub async fn handle_pending_migration_or_shutdown(
    runtime: &Arc<TokioMutex<KafuRuntimeInstance>>,
    node_id: &str,
    kafu_config: Arc<KafuConfig>,
    shutdown_tx: broadcast::Sender<()>,
    snapshot_cache: SnapshotCache,
    snapshot_buffers: SnapshotBuffers,
    wasm_sha256: &[u8],
) -> KafuResult<()> {
    let mut instance = runtime.lock().await;
    if instance.has_pending_migration_request() {
        let (mut main_buf, mut snapify_buf) = {
            let mut g = snapshot_buffers.lock().unwrap();
            (std::mem::take(&mut g.0), std::mem::take(&mut g.1))
        };
        migration::send_migration_request(
            &mut instance,
            node_id,
            &kafu_config,
            snapshot_cache,
            &mut main_buf,
            &mut snapify_buf,
            wasm_sha256,
        )
        .await?;
        {
            let mut g = snapshot_buffers.lock().unwrap();
            g.0 = main_buf;
            g.1 = snapify_buf;
        }
    } else {
        tracing::info!("{}: Program finished", node_id);
        cluster::request_cluster_shutdown_and_exit(
            node_id,
            kafu_config,
            shutdown_tx,
            "program finished",
        )
        .await;
    }

    Ok(())
}
