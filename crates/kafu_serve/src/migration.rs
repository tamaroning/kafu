use std::time::Instant;

use kafu_config::KafuConfig;
use kafu_runtime::engine::KafuRuntimeInstance;
use lz4_flex::block::compress_prepend_size;
use tokio::time::sleep;
use tonic::transport::Endpoint;

use crate::{
    error::{KafuError, KafuResult},
    grpc,
    grpc::kafu_proto::{MemoryDeltaPage, MigrateRequest, MigrationStackEntry},
    service::SnapshotCache,
};

const MIGRATION_SEND_MAX_ATTEMPTS: usize = 5;
const MIGRATION_SEND_INITIAL_BACKOFF_MS: u64 = 200;
const MIGRATION_SEND_MAX_BACKOFF_MS: u64 = 2_000;
const WASM_PAGE_SIZE: usize = 65536;

fn is_retryable_migration_send_error(err: &KafuError) -> bool {
    match err {
        // Network/transport level failures (DNS, connect, TLS, etc.)
        KafuError::GrpcTransportError(_) => true,
        // Some Status codes often represent transient transport failures too.
        KafuError::GrpcClientError(status) => matches!(
            status.code(),
            tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::Unknown
        ),
        _ => false,
    }
}

// Cache update is applied only after a migration request is delivered successfully.
#[derive(Debug)]
enum CacheUpdate {
    Delta {
        main_len: usize,
        snapify_len: usize,
        main_delta_pages_raw: Vec<(u32, Vec<u8>)>,
        snapify_delta_pages_raw: Vec<(u32, Vec<u8>)>,
    },
    Full {
        main: Vec<u8>,
        snapify: Vec<u8>,
    },
}

#[derive(Debug)]
struct PreparedMigration {
    req: MigrateRequest,
    total_size_bytes: usize,
    full_main_bytes: usize,
    cache_update: CacheUpdate,
}

struct PrepareMigrationRequestArgs<'a> {
    node_id: &'a str,
    endpoint_str: &'a str,
    endpoint: Endpoint,
    kafu_config: &'a KafuConfig,
    migration_stack: &'a [MigrationStackEntry],
    main_buf: &'a mut Vec<u8>,
    snapify_buf: &'a mut Vec<u8>,
    wasm_sha256: &'a [u8],
}

fn apply_delta_pages_in_place(
    mem: &mut Vec<u8>,
    target_len: usize,
    delta_pages: &[(u32, Vec<u8>)],
) {
    if mem.len() < target_len {
        mem.resize(target_len, 0);
    }
    for (page_index, data) in delta_pages {
        let start = (*page_index as usize).saturating_mul(WASM_PAGE_SIZE);
        let end = start.saturating_add(data.len());
        if end <= mem.len() && !data.is_empty() {
            mem[start..end].copy_from_slice(data.as_slice());
        }
    }
}

fn compress_if_smaller(data: &[u8]) -> (Vec<u8>, bool) {
    let compressed = compress_prepend_size(data);
    if compressed.len() < data.len() {
        (compressed, true)
    } else {
        (data.to_vec(), false)
    }
}

fn build_delta_pages_payload(
    raw_pages: &[(u32, Vec<u8>)],
    use_compression: bool,
) -> Vec<MemoryDeltaPage> {
    raw_pages
        .iter()
        .map(|(page_index, data)| {
            let (payload, data_compressed) = if use_compression {
                let compressed = compress_prepend_size(data.as_slice());
                if compressed.len() < data.len() {
                    (compressed, true)
                } else {
                    (data.clone(), false)
                }
            } else {
                (data.clone(), false)
            };
            MemoryDeltaPage {
                page_index: *page_index,
                data: payload,
                data_compressed,
            }
        })
        .collect()
}

async fn apply_cache_update(snapshot_cache: &SnapshotCache, node_id: &str, update: CacheUpdate) {
    match update {
        CacheUpdate::Delta {
            main_len,
            snapify_len,
            main_delta_pages_raw,
            snapify_delta_pages_raw,
        } => {
            let mut cache = snapshot_cache.lock().await;
            if let Some(entry) = cache.as_mut() {
                apply_delta_pages_in_place(&mut entry.main, main_len, &main_delta_pages_raw);
                apply_delta_pages_in_place(
                    &mut entry.snapify,
                    snapify_len,
                    &snapify_delta_pages_raw,
                );
            } else {
                tracing::warn!(
                    "{}: snapshot_cache missing baseline; reverse delta migration may require a full snapshot",
                    node_id
                );
            }
        }
        CacheUpdate::Full { main, snapify } => {
            *snapshot_cache.lock().await =
                Some(crate::service::SnapshotCacheEntry { main, snapify });
        }
    }
}

fn log_payload(
    node_id: &str,
    req: &MigrateRequest,
    total_size_bytes: usize,
    full_main_bytes: usize,
    attempt: usize,
    endpoint_str: &str,
) {
    let main_img = req.main_memory.as_ref();
    let main_bytes = main_img.map(|m| m.data.len()).unwrap_or(0);
    let main_delta_bytes: usize = main_img
        .map(|m| m.delta_pages.iter().map(|p| p.data.len()).sum())
        .unwrap_or(0);
    let main_or_diff_bytes = main_bytes + main_delta_bytes;
    let snapify_bytes = req
        .snapify_memory
        .as_ref()
        .map(|m| m.data.len())
        .unwrap_or(0);
    let total_mb = (main_or_diff_bytes + snapify_bytes) as f64 / 1_000_000.0;
    let full_total_bytes = full_main_bytes + snapify_bytes;
    let main_pages = req.main_memory.as_ref().map(|m| m.pages).unwrap_or(0);
    let snapify_pages = req.snapify_memory.as_ref().map(|m| m.pages).unwrap_or(0);
    let snapify_delta_bytes: usize = req
        .snapify_memory
        .as_ref()
        .map(|m| m.delta_pages.iter().map(|p| p.data.len()).sum())
        .unwrap_or(0);

    tracing::debug!(
        "{}: migrate payload {:.2}MB (main: data={}B delta={}B pages={}, snapify: data={}B delta={}B pages={}) full={}B ({:.2}MB)",
        node_id,
        total_mb,
        main_bytes,
        main_delta_bytes,
        main_pages,
        snapify_bytes,
        snapify_delta_bytes,
        snapify_pages,
        full_total_bytes,
        full_total_bytes as f64 / 1_000_000.0
    );

    tracing::debug!(
        "{}: Sending migration request to {} (message size: {:.2} MB, attempt {}/{})",
        node_id,
        endpoint_str,
        total_size_bytes as f64 / 1_000_000.0,
        attempt,
        MIGRATION_SEND_MAX_ATTEMPTS
    );
}

async fn prepare_full_snapshot_request(
    instance: &mut KafuRuntimeInstance,
    node_id: &str,
    kafu_config: &KafuConfig,
    migration_stack: &[MigrationStackEntry],
    main_buf: &mut Vec<u8>,
    snapify_buf: &mut Vec<u8>,
    wasm_sha256: &[u8],
) -> KafuResult<PreparedMigration> {
    instance
        .get_snapshot_into(main_buf, snapify_buf)
        .await
        .map_err(|e| {
            KafuError::WasmMigrationError(anyhow::anyhow!("Failed to get snapshot: {}", e))
        })?;
    let main_memory = std::mem::take(main_buf);
    let snapify_memory = std::mem::take(snapify_buf);
    let main_memory_size = main_memory.len();
    let snapify_memory_size = snapify_memory.len();

    let use_compression = kafu_config.cluster.migration.memory_compression;
    let (snapshot_main_memory, snapshot_main_memory_compressed, cache_main) = if use_compression {
        // Keep uncompressed for cache; send compressed only if beneficial.
        let (compressed, is_compressed) = compress_if_smaller(main_memory.as_slice());
        if is_compressed {
            (compressed, true, main_memory)
        } else {
            // Requires clone to keep baseline for reverse migration.
            (main_memory.clone(), false, main_memory)
        }
    } else {
        // Requires clone to keep baseline for reverse migration.
        (main_memory.clone(), false, main_memory)
    };

    tracing::debug!(
        "{}: Snapshot sizes - main: {} bytes ({} KB), snapify: {} bytes ({} KB), total: {} bytes ({} KB)",
        node_id,
        main_memory_size,
        main_memory_size / 1024,
        snapify_memory_size,
        snapify_memory_size / 1024,
        snapshot_main_memory.len() + snapify_memory_size,
        (snapshot_main_memory.len() + snapify_memory_size) / 1024
    );

    let req = MigrateRequest {
        wasm_sha256: wasm_sha256.to_vec(),
        migration_stack: migration_stack.to_vec(),
        main_memory: Some(crate::grpc::kafu_proto::MemoryImage {
            data: snapshot_main_memory,
            compressed: snapshot_main_memory_compressed,
            pages: (main_memory_size / WASM_PAGE_SIZE) as u64,
            delta_pages: vec![],
        }),
        snapify_memory: Some(crate::grpc::kafu_proto::MemoryImage {
            data: snapify_memory.clone(),
            compressed: false,
            pages: (snapify_memory_size / WASM_PAGE_SIZE) as u64,
            delta_pages: vec![],
        }),
    };

    Ok(PreparedMigration {
        total_size_bytes: req.main_memory.as_ref().map(|m| m.data.len()).unwrap_or(0)
            + req
                .snapify_memory
                .as_ref()
                .map(|m| m.data.len())
                .unwrap_or(0),
        full_main_bytes: main_memory_size,
        cache_update: CacheUpdate::Full {
            main: cache_main,
            snapify: snapify_memory,
        },
        req,
    })
}

async fn prepare_migration_request(
    instance: &mut KafuRuntimeInstance,
    args: PrepareMigrationRequestArgs<'_>,
) -> KafuResult<PreparedMigration> {
    let receiver_has_cache = grpc::client::check_snapshot_cache(args.endpoint.clone())
        .await
        .map(|r| r.has_cache)
        .unwrap_or(false);

    // Prefer delta only when the receiver has a baseline.
    if receiver_has_cache {
        let Some((main_delta_raw, snapify_delta_raw, main_len, snapify_len)) = instance
            .checkpoint_and_get_delta_pages()
            .await
            .map_err(|e| {
                KafuError::WasmMigrationError(anyhow::anyhow!(
                    "Failed to checkpoint+diff delta pages: {}",
                    e
                ))
            })?
        else {
            // No baseline in runtime; fall back to full snapshot.
            return prepare_full_snapshot_request(
                instance,
                args.node_id,
                args.kafu_config,
                args.migration_stack,
                args.main_buf,
                args.snapify_buf,
                args.wasm_sha256,
            )
            .await;
        };

        let use_delta = !main_delta_raw.is_empty() || !snapify_delta_raw.is_empty();
        if use_delta {
            let raw_delta_bytes: usize = main_delta_raw.iter().map(|(_, p)| p.len()).sum::<usize>()
                + snapify_delta_raw
                    .iter()
                    .map(|(_, p)| p.len())
                    .sum::<usize>();

            let use_compression = args.kafu_config.cluster.migration.memory_compression;
            let main_delta_pages = build_delta_pages_payload(&main_delta_raw, use_compression);
            let snapify_delta_pages =
                build_delta_pages_payload(&snapify_delta_raw, use_compression);

            let delta_bytes: usize = main_delta_pages.iter().map(|p| p.data.len()).sum::<usize>()
                + snapify_delta_pages
                    .iter()
                    .map(|p| p.data.len())
                    .sum::<usize>();

            tracing::debug!(
                "{}: main {:.2} MiB, diff {:.2} MiB, LZ4 after {:.2} MiB",
                args.node_id,
                main_len as f64 / (1024.0 * 1024.0),
                raw_delta_bytes as f64 / (1024.0 * 1024.0),
                delta_bytes as f64 / (1024.0 * 1024.0)
            );
            tracing::debug!(
                "{}: Sending delta to {} (main {} pages, snapify {} pages, {} bytes delta (raw {} bytes))",
                args.node_id,
                args.endpoint_str,
                main_delta_pages.len(),
                snapify_delta_pages.len(),
                delta_bytes,
                raw_delta_bytes,
            );

            let req = MigrateRequest {
                wasm_sha256: args.wasm_sha256.to_vec(),
                migration_stack: args.migration_stack.to_vec(),
                main_memory: Some(crate::grpc::kafu_proto::MemoryImage {
                    data: vec![],
                    compressed: false,
                    pages: (main_len / WASM_PAGE_SIZE) as u64,
                    delta_pages: main_delta_pages,
                }),
                snapify_memory: Some(crate::grpc::kafu_proto::MemoryImage {
                    data: vec![],
                    compressed: false,
                    pages: (snapify_len / WASM_PAGE_SIZE) as u64,
                    delta_pages: snapify_delta_pages,
                }),
            };
            return Ok(PreparedMigration {
                req,
                total_size_bytes: delta_bytes,
                full_main_bytes: main_len,
                cache_update: CacheUpdate::Delta {
                    main_len,
                    snapify_len,
                    main_delta_pages_raw: main_delta_raw,
                    snapify_delta_pages_raw: snapify_delta_raw,
                },
            });
        }
    }

    // Fall back to full snapshot when delta isn't usable.
    prepare_full_snapshot_request(
        instance,
        args.node_id,
        args.kafu_config,
        args.migration_stack,
        args.main_buf,
        args.snapify_buf,
        args.wasm_sha256,
    )
    .await
}

/// Reusable buffers can be passed to avoid allocating on each checkpoint.
pub async fn send_migration_request(
    instance: &mut KafuRuntimeInstance,
    node_id: &str,
    kafu_config: &KafuConfig,
    snapshot_cache: SnapshotCache,
    main_buf: &mut Vec<u8>,
    snapify_buf: &mut Vec<u8>,
    wasm_sha256: &[u8],
) -> KafuResult<()> {
    let pending_migration_request = instance.take_pending_migration_request().ok_or_else(|| {
        KafuError::WasmMigrationError(anyhow::anyhow!("No pending migration request on instance"))
    })?;
    let to_node_id = pending_migration_request.to_node_id;
    let dest_node_config = kafu_config.nodes.get(&to_node_id).ok_or_else(|| {
        KafuError::WasmMigrationError(anyhow::anyhow!(
            "Destination node '{}' not found in configuration",
            to_node_id
        ))
    })?;

    let endpoint = Endpoint::from_shared(format!(
        "http://{}:{}",
        dest_node_config.address, dest_node_config.port
    ))
    .map_err(|e| {
        KafuError::WasmMigrationError(anyhow::anyhow!(
            "Invalid URL in destination node configuration: {}",
            e
        ))
    })?;

    let endpoint_str = format!("{}:{}", dest_node_config.address, dest_node_config.port);

    let migration_stack = instance
        .get_store()
        .data()
        .get_migration_ctx()
        .get_migration_stack()
        .iter()
        .map(|entry| MigrationStackEntry {
            from_node_id: entry.from_node_id.clone(),
            wasm_stack_height: entry.wasm_stack_height,
        })
        .collect::<Vec<_>>();

    let res = {
        let mut attempt: usize = 1;
        let mut backoff_ms = MIGRATION_SEND_INITIAL_BACKOFF_MS;
        loop {
            if attempt > 1 {
                tracing::warn!(
                    "{}: Retrying migration request to {} (attempt {}/{}, backoff {} ms)",
                    node_id,
                    endpoint_str,
                    attempt,
                    MIGRATION_SEND_MAX_ATTEMPTS,
                    backoff_ms
                );
                sleep(std::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(MIGRATION_SEND_MAX_BACKOFF_MS);
            }

            let t0_checkpoint = Instant::now();
            let PreparedMigration {
                req,
                total_size_bytes,
                full_main_bytes,
                cache_update,
            } = prepare_migration_request(
                instance,
                PrepareMigrationRequestArgs {
                    node_id,
                    endpoint_str: &endpoint_str,
                    endpoint: endpoint.clone(),
                    kafu_config,
                    migration_stack: &migration_stack,
                    main_buf,
                    snapify_buf,
                    wasm_sha256,
                },
            )
            .await?;

            let checkpoint_total_sec = t0_checkpoint.elapsed().as_secs_f64();
            tracing::debug!(
                "{}: Checkpoint completed (total={:.3}s)",
                node_id,
                checkpoint_total_sec
            );

            log_payload(
                node_id,
                &req,
                total_size_bytes,
                full_main_bytes,
                attempt,
                &endpoint_str,
            );

            match grpc::client::send_migration_request(req, endpoint.clone()).await {
                Ok(res) => {
                    tracing::debug!(
                        "{}: Migration request delivered to {}",
                        node_id,
                        endpoint_str
                    );
                    apply_cache_update(&snapshot_cache, node_id, cache_update).await;
                    break Ok(res);
                }
                Err(e) => {
                    tracing::error!(
                        "{}: Failed to send migration request to {} (attempt {}/{}): {}",
                        node_id,
                        endpoint_str,
                        attempt,
                        MIGRATION_SEND_MAX_ATTEMPTS,
                        e
                    );
                    if attempt < MIGRATION_SEND_MAX_ATTEMPTS
                        && is_retryable_migration_send_error(&e)
                    {
                        attempt += 1;
                        continue;
                    }
                    break Err(e);
                }
            }
        }
    }
    .map_err(|e| {
        KafuError::WasmMigrationError(anyhow::anyhow!(
            "Failed to send migration request to {} after {} attempts: {}",
            endpoint_str,
            MIGRATION_SEND_MAX_ATTEMPTS,
            e
        ))
    })?;

    if !res.success {
        Err(KafuError::WasmMigrationError(anyhow::anyhow!(
            "gRPC server at {} returned failure",
            endpoint_str
        )))
    } else {
        Ok(())
    }
}
