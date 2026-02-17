//! Wasmtime-based runtime engine for Kafu.
//!
//! This module intentionally keeps the public API stable for downstream crates
//! (`kafu_serve`, `kafu_singlenode`, etc.) while splitting implementation into
//! focused submodules.

mod config;
mod instance;
mod kafu_metadata;
mod linker;
mod migration;
mod module;
mod store;

pub use config::{KafuRuntimeConfig, LinkerConfig, LinkerSnapifyConfig, WasiConfig};
pub use instance::{
    apply_memory_delta, apply_memory_delta_into, apply_memory_delta_into_sized, KafuRuntimeInstance,
};
pub use migration::{InterruptReason, MigrationStackEntry, PendingMigration};
pub use module::WasmModule;
pub use store::{KafuStore, MAIN_MEMORY_PAGE_SIZE};
