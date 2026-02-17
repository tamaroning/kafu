use std::sync::Arc;

use wasi_common::WasiCtx;
use wasmtime_wasi_nn::witx::WasiNnCtx;
use wasmtime_wast::WastContext;

use crate::witx;

use super::migration::MigrationContext;
use super::module::WasmModule;

pub(crate) struct KafuLibraryContext {
    /// WASI context and the implementation.
    pub(crate) wasi: WasiCtx,
    /// WASI-NN context and the implementation.
    pub(crate) wasi_nn: WasiNnCtx,
    /// Kafu helper context and the implementation.
    pub(crate) kafu_helper: witx::KafuHelperCtx,
    /// Spectest context and the implementation.
    pub(crate) _wast: WastContext,
}

/// Wasm page size (64KB). Used for main memory delta encoding.
pub const MAIN_MEMORY_PAGE_SIZE: usize = 65536;

pub struct KafuStore {
    /// The node ID of the runtime.
    pub(crate) node_id: String,
    // Wasm module info
    pub(crate) module: Arc<WasmModule>,
    /// WASI/WASI-NN/kafu_helper/spectest contexts.
    pub(crate) libctx: KafuLibraryContext,
    /// Execution-related context (migration, etc.).
    pub migration_ctx: MigrationContext,
    /// Baseline main memory at last restore; used to compute delta for migration.
    pub(crate) baseline_main_memory: Option<Arc<Vec<u8>>>,
    /// Baseline snapify memory at last restore; used to compute delta for migration.
    pub(crate) baseline_snapify_memory: Option<Arc<Vec<u8>>>,
}

impl KafuStore {
    pub fn get_node_id(&self) -> &str {
        &self.node_id
    }

    pub fn get_migration_ctx(&self) -> &MigrationContext {
        &self.migration_ctx
    }
}
