use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result};
use rayon::prelude::*;
use wasmtime::{Engine, Instance, Module, Store, TypedFunc};

use super::store::MAIN_MEMORY_PAGE_SIZE;
use wasmtime_wasi_nn::preload;
use wasmtime_wast::{Async, WastContext};

use super::config::KafuRuntimeConfig;
use super::linker::{link_imports, wasi_ctx};
use super::migration::{MigrationContext, MigrationStackEntry, PendingMigration};
use super::module::WasmModule;
use super::store::{KafuLibraryContext, KafuStore};

/// Delta pages for baseline comparison.
type SnapshotMemoryDelta = Vec<(u32, Vec<u8>)>;

pub struct KafuRuntimeInstance {
    instance: Instance,
    store: Store<KafuStore>,
    /// Cached for fast restore; resolved lazily so modules without snapify exports can still start().
    start_restore_func: Option<TypedFunc<(), ()>>,
    restore_globals_func: Option<TypedFunc<(), ()>>,
    start_func: Option<TypedFunc<(), ()>>,
}

impl KafuRuntimeInstance {
    pub async fn new(wasm: Arc<WasmModule>, config: &KafuRuntimeConfig) -> Result<Self> {
        let mut wasmtime_config = wasmtime::Config::new();
        wasmtime_config.async_support(true);
        wasmtime_config.wasm_backtrace(true);
        let engine = Engine::new(&wasmtime_config)?;

        let main_module = Module::new(&engine, &wasm.wasm)?;

        let wasi = wasi_ctx(&config.wasi_config)?;
        let (backends, registry) = preload(&[])?;
        let wasi_nn = wasmtime_wasi_nn::witx::WasiNnCtx::new(backends, registry);
        let wast = WastContext::new(&engine, Async::No, |_| {});

        let mut store = Store::new(
            &engine,
            KafuStore {
                node_id: config.node_id.clone(),
                libctx: KafuLibraryContext {
                    wasi,
                    wasi_nn,
                    kafu_helper: crate::witx::KafuHelperCtx::new(),
                    _wast: wast,
                },
                module: wasm,
                migration_ctx: MigrationContext {
                    pending_migration_request: None,
                    migration_stack: vec![],
                },
                baseline_main_memory: None,
                baseline_snapify_memory: None,
            },
        );

        let mut linker: wasmtime::Linker<KafuStore> = wasmtime::Linker::new(&engine);
        link_imports(&config.linker_config, &mut linker, &mut store)?;

        let instance = linker
            .instantiate_async(&mut store, &main_module)
            .await
            .context("failed to instantiate main module")?;
        Ok(Self {
            instance,
            store,
            start_restore_func: None,
            restore_globals_func: None,
            start_func: None,
        })
    }

    fn get_or_resolve_start_restore(&mut self) -> Result<&TypedFunc<(), ()>> {
        if self.start_restore_func.is_none() {
            let func = self
                .instance
                .get_typed_func::<(), ()>(&mut self.store, "snapify_start_restore")
                .context("export `snapify_start_restore` not found")?;
            self.start_restore_func = Some(func);
        }
        Ok(self.start_restore_func.as_ref().unwrap())
    }

    fn get_or_resolve_restore_globals(&mut self) -> Result<&TypedFunc<(), ()>> {
        if self.restore_globals_func.is_none() {
            let func = self
                .instance
                .get_typed_func::<(), ()>(&mut self.store, "snapify_restore_globals")
                .context("export `snapify_restore_globals` not found")?;
            self.restore_globals_func = Some(func);
        }
        Ok(self.restore_globals_func.as_ref().unwrap())
    }

    fn get_or_resolve_start(&mut self) -> Result<&TypedFunc<(), ()>> {
        if self.start_func.is_none() {
            let func = self
                .instance
                .get_typed_func::<(), ()>(&mut self.store, "_start")
                .context("failed to get `_start` function")?;
            self.start_func = Some(func);
        }
        Ok(self.start_func.as_ref().unwrap())
    }

    pub async fn restore(
        &mut self,
        migration_stack: Vec<MigrationStackEntry>,
        main_memory: Vec<u8>,
        snapify_memory: Vec<u8>,
    ) -> Result<()> {
        // NOTE: restore() can be performance-critical on slower devices (e.g. Raspberry Pi).
        // Keep a concise breakdown so users can pinpoint bottlenecks. Program execution (`_start`)
        // is intentionally excluded; call resume() separately when you're ready.
        let t0 = Instant::now();

        let t_mem = Instant::now();
        self.grow_and_restore_memory("memory", &main_memory)?;
        self.grow_and_restore_memory("snapify_memory", &snapify_memory)?;
        let dt_mem = t_mem.elapsed();

        self.store.data_mut().migration_ctx.migration_stack = migration_stack;

        // Baseline = memories we just wrote (before restore_globals).
        // Delta will include both restore_globals changes and program writes.
        self.store
            .data_mut()
            .baseline_main_memory
            .replace(Arc::new(main_memory));
        self.store
            .data_mut()
            .baseline_snapify_memory
            .replace(Arc::new(snapify_memory));

        // Call snapify_start_restore (using cached TypedFunc when available)
        let start_restore = self.get_or_resolve_start_restore()?.clone();
        start_restore.call_async(&mut self.store, ()).await?;

        // Call snapify_restore_globals (using cached TypedFunc when available)
        let restore_globals = self.get_or_resolve_restore_globals()?.clone();
        restore_globals.call_async(&mut self.store, ()).await?;

        tracing::debug!(
            "{}: Restore completed (total={:.3}s mem={:.3}s)",
            self.store.data().get_node_id(),
            t0.elapsed().as_secs_f64(),
            dt_mem.as_secs_f64()
        );

        Ok(())
    }

    /// Resume program execution after a successful restore().
    ///
    /// This invokes the WASM module's exported start function (`_start`).
    pub async fn resume(&mut self) -> Result<()> {
        self.start().await
    }

    fn grow_and_restore_memory(&mut self, memory_name: &str, memory: &[u8]) -> Result<()> {
        let mem_instance = self
            .instance
            .get_memory(&mut self.store, memory_name)
            .with_context(|| format!("memory export `{memory_name}` not found"))?;
        let memory_page_size = (memory.len() / 65536) as u64;
        let current = mem_instance.size(&mut self.store);
        let delta = memory_page_size.saturating_sub(current);
        if delta > 0 {
            mem_instance.grow(&mut self.store, delta)?;
        }
        mem_instance.write(&mut self.store, 0, memory)?;
        Ok(())
    }

    /// Invoke the start function in the WASM module.
    /// In modules processed by Snapify, the start function is exported with the name `_start`.
    pub async fn start(&mut self) -> Result<()> {
        let start = self.get_or_resolve_start()?.clone();
        start
            .call_async(&mut self.store, ())
            .await
            .context("failed to call `_start`")?;
        Ok(())
    }

    pub fn has_pending_migration_request(&mut self) -> bool {
        self.store
            .data()
            .migration_ctx
            .pending_migration_request
            .is_some()
    }

    pub fn take_pending_migration_request(&mut self) -> Option<PendingMigration> {
        self.store
            .data_mut()
            .migration_ctx
            .pending_migration_request
            .take()
    }

    /// Precondition: The program is suspended.
    /// Fills the provided buffers to avoid allocations; buffers are resized to match memory sizes.
    pub async fn get_snapshot_into(
        &mut self,
        main_buf: &mut Vec<u8>,
        snapify_buf: &mut Vec<u8>,
    ) -> Result<()> {
        let checkpoint_globals = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, "snapify_checkpoint_globals")
            .context("export `snapify_checkpoint_globals` not found")?;
        checkpoint_globals.call_async(&mut self.store, ()).await?;

        let main_mem = self
            .instance
            .get_memory(&mut self.store, "memory")
            .context("memory export `memory` not found")?;
        let main_slice = main_mem.data(&mut self.store);
        main_buf.resize(main_slice.len(), 0);
        main_buf.copy_from_slice(main_slice);

        let snapify_mem = self
            .instance
            .get_memory(&mut self.store, "snapify_memory")
            .context("memory export `snapify_memory` not found")?;
        let snapify_slice = snapify_mem.data(&mut self.store);
        snapify_buf.resize(snapify_slice.len(), 0);
        snapify_buf.copy_from_slice(snapify_slice);

        Ok(())
    }

    /// Checkpoint globals and compute delta pages against the stored baseline (last restore).
    ///
    /// This avoids copying the full linear memory into a temporary Vec before diffing.
    /// Returns:
    /// - delta pages for main memory
    /// - delta pages for snapify memory
    /// - current main memory length (bytes)
    /// - current snapify memory length (bytes)
    pub async fn checkpoint_and_get_delta_pages(
        &mut self,
    ) -> Result<Option<(SnapshotMemoryDelta, SnapshotMemoryDelta, usize, usize)>> {
        let baseline_main = self
            .store
            .data()
            .baseline_main_memory
            .as_ref()
            .map(Arc::clone);
        let baseline_snapify = self
            .store
            .data()
            .baseline_snapify_memory
            .as_ref()
            .map(Arc::clone);
        let (Some(baseline_main), Some(baseline_snapify)) = (baseline_main, baseline_snapify)
        else {
            return Ok(None);
        };

        let checkpoint_globals = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, "snapify_checkpoint_globals")
            .context("export `snapify_checkpoint_globals` not found")?;
        checkpoint_globals.call_async(&mut self.store, ()).await?;

        // NOTE: `Memory::data(&mut store)` returns a slice tied to `store`'s mutable borrow.
        // Compute deltas in separate scopes to avoid overlapping mutable borrows.
        let (main_delta, main_len) = {
            let main_mem = self
                .instance
                .get_memory(&mut self.store, "memory")
                .context("memory export `memory` not found")?;
            let main_slice = main_mem.data(&mut self.store);
            (
                compute_memory_delta_pages(baseline_main.as_slice(), main_slice, "main"),
                main_slice.len(),
            )
        };

        let (snapify_delta, snapify_len) = {
            let snapify_mem = self
                .instance
                .get_memory(&mut self.store, "snapify_memory")
                .context("memory export `snapify_memory` not found")?;
            let snapify_slice = snapify_mem.data(&mut self.store);
            (
                compute_memory_delta_pages(baseline_snapify.as_slice(), snapify_slice, "snapify"),
                snapify_slice.len(),
            )
        };

        Ok(Some((main_delta, snapify_delta, main_len, snapify_len)))
    }

    /// Precondition: The program is suspended.
    pub async fn get_snapshot(&mut self) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut main_buf = Vec::new();
        let mut snapify_buf = Vec::new();
        self.get_snapshot_into(&mut main_buf, &mut snapify_buf)
            .await?;
        Ok((main_buf, snapify_buf))
    }

    /// Returns delta pages for main memory when a baseline exists.
    /// Delta pages are determined by byte comparison only. When current size differs from
    /// baseline (e.g. memory.grow happened), overlapping region is compared and new pages
    /// are included in full so delta transfer still works.
    /// Precondition: The program is suspended (call get_snapshot or checkpoint first).
    pub fn get_snapshot_main_memory_delta(
        &mut self,
        current_main_memory: &[u8],
    ) -> Option<SnapshotMemoryDelta> {
        let baseline = self.store.data().baseline_main_memory.as_deref()?;
        Some(compute_memory_delta_pages(
            baseline,
            current_main_memory,
            "main",
        ))
    }

    pub fn get_snapshot_snapify_memory_delta(
        &mut self,
        current_snapify_memory: &[u8],
    ) -> Option<SnapshotMemoryDelta> {
        let baseline = self.store.data().baseline_snapify_memory.as_deref()?;
        Some(compute_memory_delta_pages(
            baseline,
            current_snapify_memory,
            "snapify",
        ))
    }

    pub fn get_store(&self) -> &Store<KafuStore> {
        &self.store
    }
}

fn compute_memory_delta_pages(baseline: &[u8], current: &[u8], label: &str) -> SnapshotMemoryDelta {
    let mut delta_pages = Vec::new();
    let overlap = baseline.len().min(current.len());
    let mut page_index = 0u32;
    let mut offset = 0;
    while offset < current.len() {
        let end = (offset + MAIN_MEMORY_PAGE_SIZE).min(current.len());
        let differs = if offset < overlap {
            let base_end = (offset + MAIN_MEMORY_PAGE_SIZE).min(baseline.len());
            baseline[offset..base_end] != current[offset..base_end]
        } else {
            true
        };
        if differs {
            // Copy only pages that actually differ to reduce allocations and memcpy costs.
            // This is performance-critical for checkpoint on slower devices.
            let page_data = current[offset..end].to_vec();
            delta_pages.push((page_index, page_data));
        }
        page_index += 1;
        offset += MAIN_MEMORY_PAGE_SIZE;
    }
    if current.len() != baseline.len() {
        tracing::debug!(
            "get_snapshot_{}_memory_delta: size changed (baseline {} vs current {}), delta has {} pages",
            label,
            baseline.len(),
            current.len(),
            delta_pages.len()
        );
    }
    delta_pages
}

/// Applies delta pages to a baseline memory image and writes the full memory into `out`.
///
/// - When delta pages extend past baseline (memory.grow case), `out` is sized to fit.
/// - When `target_len_bytes` is larger than what delta implies, `out` is grown to that size.
///   This is useful when the sender transmits the final memory size out-of-band (e.g. via RPC),
///   allowing the receiver to grow/allocate to the final size even if new pages are all-zero and
///   omitted from delta transfer.
///
/// Reuses `out`'s allocation when possible. Delta application is parallelized.
/// Each (page_index, data) writes to a disjoint region; page indices are unique per delta.
pub fn apply_memory_delta_into_sized(
    baseline: &[u8],
    delta_pages: &[(u32, &[u8])],
    out: &mut Vec<u8>,
    target_len_bytes: usize,
) -> Result<(), anyhow::Error> {
    if baseline.is_empty() && delta_pages.is_empty() {
        anyhow::bail!("Baseline length is zero and no delta pages");
    }
    let mut out_len = baseline.len();
    for (page_index, data) in delta_pages {
        let start = (*page_index as usize).saturating_mul(MAIN_MEMORY_PAGE_SIZE);
        let end = start + data.len();
        if end > out_len {
            out_len = end;
        }
    }
    if target_len_bytes > out_len {
        out_len = target_len_bytes;
    }
    tracing::debug!(
        "apply_memory_delta_into: baseline_len={}, out_len={}, num_delta_pages={}",
        baseline.len(),
        out_len,
        delta_pages.len()
    );
    out.resize(out_len, 0);
    if !baseline.is_empty() {
        let copy_len = baseline.len().min(out_len);
        out[..copy_len].copy_from_slice(&baseline[..copy_len]);
    }
    let base_addr = out.as_mut_ptr() as usize;
    delta_pages.par_iter().for_each(|(page_index, data)| {
        let start = (*page_index as usize).saturating_mul(MAIN_MEMORY_PAGE_SIZE);
        let len = data.len();
        if start + len <= out_len && !data.is_empty() {
            let dest = base_addr as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), dest.add(start), len);
            }
        }
    });
    Ok(())
}

pub fn apply_memory_delta_into(
    baseline: &[u8],
    delta_pages: &[(u32, &[u8])],
    out: &mut Vec<u8>,
) -> Result<(), anyhow::Error> {
    apply_memory_delta_into_sized(baseline, delta_pages, out, 0)
}

/// Applies delta pages to baseline memory and returns the full memory.
pub fn apply_memory_delta(
    baseline: &[u8],
    delta_pages: &[(u32, &[u8])],
) -> Result<Vec<u8>, anyhow::Error> {
    let mut out = Vec::new();
    apply_memory_delta_into_sized(baseline, delta_pages, &mut out, 0)?;
    Ok(out)
}
