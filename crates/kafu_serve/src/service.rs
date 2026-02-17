use std::sync::Arc;

use kafu_config::KafuConfig;
use kafu_runtime::engine::{KafuRuntimeInstance, apply_memory_delta_into_sized};
use lz4_flex::block::decompress_size_prepended;
use tokio::sync::{Mutex, broadcast, watch};
use tonic::{Request, Response, Status};

use crate::{
    grpc::kafu_proto::{
        CheckSnapshotCacheRequest, CheckSnapshotCacheResponse, HeartbeatRequest, HeartbeatResponse,
        MemoryImage, MigrateRequest, MigrateResponse, ShutdownRequest, ShutdownResponse,
        command_server::Command,
    },
    runtime::{self, SnapshotBuffers},
};

#[derive(Clone, Debug)]
pub struct LeaderHeartbeatState {
    pub from_node_id: String,
    pub last_seen: tokio::time::Instant,
    pub execution_started: bool,
}

impl Default for LeaderHeartbeatState {
    fn default() -> Self {
        Self {
            from_node_id: String::new(),
            last_seen: tokio::time::Instant::now(),
            execution_started: false,
        }
    }
}

/// Snapshot cache: baseline hash -> main memory. Used to apply delta without re-transferring full memory.
#[derive(Clone, Debug)]
pub struct SnapshotCacheEntry {
    pub main: Vec<u8>,
    pub snapify: Vec<u8>,
}

pub type SnapshotCache = Arc<Mutex<Option<SnapshotCacheEntry>>>;

pub struct KafuService {
    pub node_id: String,
    pub kafu_config: Arc<KafuConfig>,
    /// Single runtime instance per server process.
    pub runtime: Arc<Mutex<KafuRuntimeInstance>>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub leader_heartbeat_tx: watch::Sender<LeaderHeartbeatState>,
    /// Cache of received main memory snapshots by SHA-256 hash; enables delta-based migration.
    pub snapshot_cache: SnapshotCache,
    /// Reusable buffers for checkpoint (sender) and for delta reconstruction (receiver).
    pub snapshot_buffers: SnapshotBuffers,
    /// Reusable buffer for apply_main_memory_delta_into on receiver (delta path).
    pub reconstruct_buf: Mutex<Vec<u8>>,
    /// Reusable buffer for snapify delta reconstruction (receiver).
    pub reconstruct_snapify_buf: Mutex<Vec<u8>>,
    /// SHA-256 digest (32 bytes) of the loaded Wasm binary, used to verify migration requests.
    pub wasm_sha256: [u8; 32],
}

impl KafuService {
    pub fn new(
        node_id: &str,
        kafu_config: Arc<KafuConfig>,
        runtime: Arc<Mutex<KafuRuntimeInstance>>,
        shutdown_tx: broadcast::Sender<()>,
        leader_heartbeat_tx: watch::Sender<LeaderHeartbeatState>,
        snapshot_buffers: SnapshotBuffers,
        wasm_sha256: [u8; 32],
    ) -> Self {
        Self {
            node_id: node_id.to_string(),
            kafu_config,
            runtime,
            shutdown_tx,
            leader_heartbeat_tx,
            snapshot_cache: Arc::new(Mutex::new(None)),
            snapshot_buffers,
            reconstruct_buf: Mutex::new(Vec::new()),
            reconstruct_snapify_buf: Mutex::new(Vec::new()),
            wasm_sha256,
        }
    }
}

#[tonic::async_trait]
impl Command for KafuService {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();

        // Followers use this as a push heartbeat from the leader.
        // Keep it lightweight: update shared state and return.
        let state = LeaderHeartbeatState {
            from_node_id: request.from_node_id,
            last_seen: tokio::time::Instant::now(),
            execution_started: request.execution_started,
        };
        let _ = self.leader_heartbeat_tx.send_replace(state);

        Ok(Response::new(HeartbeatResponse { accepted: true }))
    }

    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        let request = request.into_inner();
        tracing::info!(
            "{}: Received shutdown request from {:?} (reason: {:?})",
            self.node_id,
            request.from_node_id,
            request.reason
        );

        // Best-effort: if the server already started shutting down, this may fail.
        let _ = self.shutdown_tx.send(());

        Ok(Response::new(ShutdownResponse { accepted: true }))
    }

    async fn check_snapshot_cache(
        &self,
        request: Request<CheckSnapshotCacheRequest>,
    ) -> Result<Response<CheckSnapshotCacheResponse>, Status> {
        let _req = request.into_inner();
        let cache = self.snapshot_cache.lock().await;
        // We intentionally do not validate the hash here. The cache is treated as "has baseline or not".
        // Receiver-side migration will use the cached baseline (single-entry policy) when present.
        let has_cache = cache.is_some();
        Ok(Response::new(CheckSnapshotCacheResponse { has_cache }))
    }

    async fn migrate(
        &self,
        request: Request<MigrateRequest>,
    ) -> Result<Response<MigrateResponse>, Status> {
        let mut request = request.into_inner();

        let kafu_config = Arc::clone(&self.kafu_config);
        let node_id = self.node_id.clone();
        let node_id_for_error = node_id.clone();
        let shutdown_tx = self.shutdown_tx.clone();
        let runtime = Arc::clone(&self.runtime);
        let snapshot_cache = Arc::clone(&self.snapshot_cache);
        let snapshot_buffers = self.snapshot_buffers.clone();
        let main_img: MemoryImage = request
            .main_memory
            .take()
            .ok_or_else(|| Status::invalid_argument("Missing main_memory"))?;
        let snapify_img: MemoryImage = request
            .snapify_memory
            .take()
            .ok_or_else(|| Status::invalid_argument("Missing snapify_memory"))?;
        let uses_delta = !main_img.delta_pages.is_empty() || !snapify_img.delta_pages.is_empty();
        tracing::debug!(
            "{}: Received migration request (stack_depth={} uses_delta={} main: pages={} data={}B delta_pages={} compressed={} snapify: pages={} data={}B delta_pages={} compressed={})",
            node_id,
            request.migration_stack.len(),
            uses_delta,
            main_img.pages,
            main_img.data.len(),
            main_img.delta_pages.len(),
            main_img.compressed,
            snapify_img.pages,
            snapify_img.data.len(),
            snapify_img.delta_pages.len(),
            snapify_img.compressed
        );

        // Verify that the sender is running the same Wasm binary.
        if request.wasm_sha256.as_slice() != self.wasm_sha256 {
            let to_hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
            return Err(Status::failed_precondition(format!(
                "Wasm SHA-256 mismatch: sender={}, local={}",
                to_hex(&request.wasm_sha256),
                to_hex(&self.wasm_sha256)
            )));
        }
        if main_img.pages == 0 {
            return Err(Status::invalid_argument(
                "main_memory.pages must be non-zero",
            ));
        }
        if snapify_img.pages == 0 {
            return Err(Status::invalid_argument(
                "snapify_memory.pages must be non-zero",
            ));
        }
        let pages_to_len = |pages: u64| -> Result<usize, Status> {
            (pages as usize)
                .checked_mul(65536)
                .ok_or_else(|| Status::invalid_argument("Requested memory pages overflow"))
        };
        let requested_main_len = pages_to_len(main_img.pages)?;
        let requested_snapify_len = pages_to_len(snapify_img.pages)?;
        let migration_stack = request
            .migration_stack
            .iter()
            .map(|entry| kafu_runtime::engine::MigrationStackEntry {
                from_node_id: entry.from_node_id.clone(),
                wasm_stack_height: entry.wasm_stack_height,
            })
            .collect();

        let (mut main_memory, mut snapify_memory) = {
            if uses_delta {
                // Lock reusable reconstruct buffers first, then lock snapshot_cache.
                // This avoids holding snapshot_cache across any `.await` points.
                let mut main_buf = self.reconstruct_buf.lock().await;
                let mut snapify_buf = self.reconstruct_snapify_buf.lock().await;

                let mut cache = snapshot_cache.lock().await;
                let cached = cache.as_ref().ok_or_else(|| {
                    Status::failed_precondition(
                        "Baseline not in cache; sender should send full snapshot",
                    )
                })?;

                let reconstruct_one = |label: &str,
                                       img: &MemoryImage,
                                       baseline: &[u8],
                                       target_len: usize,
                                       buf: &mut Vec<u8>|
                 -> Result<Vec<u8>, Status> {
                    let page_size = 65536usize;
                    if baseline.len() > target_len {
                        return Err(Status::invalid_argument(format!(
                            "{label} baseline size {} exceeds requested {}",
                            baseline.len(),
                            target_len
                        )));
                    }
                    // Start from either provided full data (if any) or baseline.
                    let mut base = if !img.data.is_empty() {
                        if img.compressed {
                            decompress_size_prepended(&img.data).map_err(|e| {
                                Status::invalid_argument(format!(
                                    "{label} memory decompress: {}",
                                    e
                                ))
                            })?
                        } else {
                            img.data.clone()
                        }
                    } else {
                        baseline.to_vec()
                    };
                    if base.len() > target_len {
                        return Err(Status::invalid_argument(format!(
                            "{label} memory size {} exceeds requested {}",
                            base.len(),
                            target_len
                        )));
                    }
                    base.resize(target_len, 0);

                    if img.delta_pages.is_empty() {
                        return Ok(base);
                    }

                    // Decompress and validate delta pages.
                    let mut delta_pages_decompressed: Vec<(u32, Vec<u8>)> =
                        Vec::with_capacity(img.delta_pages.len());
                    for p in &img.delta_pages {
                        if (p.page_index as u64) >= img.pages {
                            return Err(Status::invalid_argument(format!(
                                "{label} delta page_index {} out of range (pages={})",
                                p.page_index, img.pages
                            )));
                        }
                        let data = if p.data_compressed {
                            decompress_size_prepended(&p.data).map_err(|e| {
                                Status::invalid_argument(format!(
                                    "{label} delta page decompress: {}",
                                    e
                                ))
                            })?
                        } else {
                            p.data.clone()
                        };
                        if data.len() != page_size {
                            return Err(Status::invalid_argument(format!(
                                "{label} delta page has invalid size {} (expected {})",
                                data.len(),
                                page_size
                            )));
                        }
                        let start = (p.page_index as usize).saturating_mul(page_size);
                        let end = start + data.len();
                        if end > target_len {
                            return Err(Status::invalid_argument(format!(
                                "{label} delta page out of bounds (end {} > target_len {})",
                                end, target_len
                            )));
                        }
                        delta_pages_decompressed.push((p.page_index, data));
                    }
                    let delta_refs: Vec<(u32, &[u8])> = delta_pages_decompressed
                        .iter()
                        .map(|(i, v)| (*i, v.as_slice()))
                        .collect();

                    // Apply delta into reusable buffer, starting from `base` as baseline.
                    // We pass `base` as baseline because it already includes either the transmitted full data
                    // or the cached baseline and is resized to the final length.
                    buf.clear();
                    apply_memory_delta_into_sized(base.as_slice(), &delta_refs, buf, target_len)
                        .map_err(|e| {
                            Status::invalid_argument(format!("{label} delta apply: {}", e))
                        })?;
                    Ok(std::mem::take(buf))
                };

                let main = reconstruct_one(
                    "main",
                    &main_img,
                    &cached.main,
                    requested_main_len,
                    &mut main_buf,
                )?;
                let snapify = reconstruct_one(
                    "snapify",
                    &snapify_img,
                    &cached.snapify,
                    requested_snapify_len,
                    &mut snapify_buf,
                )?;

                *cache = Some(SnapshotCacheEntry {
                    main: main.clone(),
                    snapify: snapify.clone(),
                });
                (main, snapify)
            } else {
                if main_img.data.is_empty() || snapify_img.data.is_empty() {
                    return Err(Status::invalid_argument(
                        "Full snapshot (main_memory + snapify_memory) or valid delta required",
                    ));
                }
                let main = if main_img.compressed {
                    decompress_size_prepended(&main_img.data).map_err(|e| {
                        Status::invalid_argument(format!("Main memory decompress: {}", e))
                    })?
                } else {
                    main_img.data.clone()
                };
                let snapify = if snapify_img.compressed {
                    decompress_size_prepended(&snapify_img.data).map_err(|e| {
                        Status::invalid_argument(format!("Snapify memory decompress: {}", e))
                    })?
                } else {
                    snapify_img.data.clone()
                };
                {
                    let mut cache = snapshot_cache.lock().await;
                    *cache = Some(SnapshotCacheEntry {
                        main: main.clone(),
                        snapify: snapify.clone(),
                    });
                }
                (main, snapify)
            }
        };

        // Ensure memory lengths match requested page sizes so restore() grows first.
        if main_memory.len() > requested_main_len {
            return Err(Status::invalid_argument(format!(
                "Main memory size {} exceeds requested {}",
                main_memory.len(),
                requested_main_len
            )));
        }
        main_memory.resize(requested_main_len, 0);
        if snapify_memory.len() > requested_snapify_len {
            return Err(Status::invalid_argument(format!(
                "Snapify memory size {} exceeds requested {}",
                snapify_memory.len(),
                requested_snapify_len
            )));
        }
        snapify_memory.resize(requested_snapify_len, 0);

        let wasm_sha256 = self.wasm_sha256;
        let handle = tokio::spawn(async move {
            {
                let mut instance = runtime.lock().await;
                if let Err(e) = instance
                    .restore(migration_stack, main_memory, snapify_memory)
                    .await
                {
                    tracing::error!("{}: Failed to restore snapshot: {:?}", node_id, e);
                    return;
                }
                if let Err(e) = instance.resume().await {
                    tracing::error!("{}: Failed to resume after restore: {:?}", node_id, e);
                    return;
                }
            }

            if let Err(e) = runtime::handle_pending_migration_or_shutdown(
                &runtime,
                node_id.as_str(),
                kafu_config,
                shutdown_tx,
                snapshot_cache,
                snapshot_buffers,
                &wasm_sha256,
            )
            .await
            {
                tracing::error!("{}: Failed to handle post-run action: {:?}", node_id, e);
            }
        });

        // Return the response without waiting for task completion, but keep observing the task
        // in the background so we can log panics/errors.
        tokio::spawn(async move {
            if let Err(e) = handle.await {
                tracing::error!("{}: Migration task panicked: {:?}", node_id_for_error, e);
            }
        });

        Ok(Response::new(MigrateResponse { success: true }))
    }
}
