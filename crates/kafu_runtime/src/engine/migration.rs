use anyhow::{Context as _, Result};
use wasmtime::{Caller, WasmBacktrace};

use super::kafu_metadata::KafuFunctionMetadata;

use super::store::KafuStore;

/// Pair of a WASM stack frame height and a node ID.
#[derive(Debug, Clone)]
pub struct MigrationStackEntry {
    pub from_node_id: String,
    pub wasm_stack_height: u32,
}

#[derive(Debug, Clone)]
pub struct PendingMigration {
    /// Function metadata.
    pub func: KafuFunctionMetadata,
    /// Actual migration destination node ID.
    pub to_node_id: String,
    /// Reason for the migration.
    pub reason: InterruptReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptReason {
    /// KAFU_DEST function is called.
    FuncEntry = 0,
    /// Return from KAFU_DEST function.
    FuncExit = 1,
}

impl InterruptReason {
    pub(crate) fn new(reason: i32) -> Self {
        match reason {
            0 => Self::FuncEntry,
            1 => Self::FuncExit,
            _ => panic!("Invalid interrupt reason: {}", reason),
        }
    }
}

pub struct MigrationContext {
    /// pending migration request
    pub(crate) pending_migration_request: Option<PendingMigration>,
    /// Records on which stack frame the migration happened.
    /// FuncEntry: push onto the stack on the source node.
    /// FuncExit: pop from the stack on the source node.
    /// This information is also sent when issuing a migration request.
    pub(crate) migration_stack: Vec<MigrationStackEntry>,
}

impl MigrationContext {
    pub fn get_migration_stack(&self) -> &Vec<MigrationStackEntry> {
        &self.migration_stack
    }

    pub(crate) fn should_migrate(
        &self,
        from_node_id: &str,
        reason: InterruptReason,
        current_wasm_stack_height: u32,
        to_node_id: &str,
    ) -> bool {
        if from_node_id == to_node_id {
            return false;
        }

        match reason {
            InterruptReason::FuncEntry => true,
            InterruptReason::FuncExit => {
                let Some(entry) = self.migration_stack.last() else {
                    tracing::warn!("migration stack is empty at FuncExit; skipping migration");
                    return false;
                };

                if current_wasm_stack_height == entry.wasm_stack_height {
                    debug_assert!(to_node_id == entry.from_node_id);
                    true
                } else {
                    false
                }
            }
        }
    }

    pub(crate) fn on_migrate(
        &mut self,
        from_node_id: &str,
        reason: InterruptReason,
        current_wasm_stack_height: u32,
    ) {
        match reason {
            InterruptReason::FuncEntry => {
                self.migration_stack.push(MigrationStackEntry {
                    from_node_id: from_node_id.to_owned(),
                    wasm_stack_height: current_wasm_stack_height,
                });
            }
            InterruptReason::FuncExit => {
                self.migration_stack.pop();
            }
        }
    }
}

pub(crate) fn handle_migration_point(
    caller: &mut Caller<'_, KafuStore>,
    reason: InterruptReason,
) -> Result<Option<PendingMigration>> {
    // Use WasmBacktrace (frame pointer walking) instead of debug_frames (guest_debug).
    // frames()[0] is the wasm function that called into this host function.
    let trace = WasmBacktrace::capture(&*caller);
    let frames = trace.frames();
    let caller_frame = frames.get(1).unwrap();
    let func_idx = caller_frame.func_index();

    let meta = caller
        .data()
        .module
        .metadata
        .functions
        .get(&func_idx)
        .with_context(|| format!("function metadata not found for func_idx={func_idx}"))?;

    let to_node_id = match reason {
        InterruptReason::FuncEntry => match meta.dest.clone() {
            Some(dest) => dest,
            None => {
                tracing::warn!(
                    "migration destination is not set (func={:?}, reason={:?})",
                    meta.name,
                    reason
                );
                return Ok(None);
            }
        },
        InterruptReason::FuncExit => {
            // TODO: check if the migration should happen here.
            // In some cases, the migration must not happen at all function exits.
            let Some(entry) = caller.data().migration_ctx.migration_stack.last() else {
                return Ok(None);
            };
            entry.from_node_id.clone()
        }
    };

    // TODO: use actual wasm stack height when executing migration points at function-exit.
    // This prevents misaligned migration at function-exit.
    // In the following example, if migration should happen at the exit of the first call to f.
    // ```c
    // KAFU_DEST("foo")
    // void f() { if (rand() % 2 == 0) { return; } else { f(); } }
    // ```
    let current_wasm_stack_height = 0;
    let from_node_id = caller.data().node_id.clone();
    let should_migrate = caller.data().migration_ctx.should_migrate(
        &from_node_id,
        reason,
        current_wasm_stack_height,
        &to_node_id,
    );
    if should_migrate {
        let meta = meta.clone();
        tracing::info!(
            "Migration {} -> {} ({} {})",
            caller.data().node_id,
            to_node_id,
            if reason == InterruptReason::FuncEntry {
                "Entering"
            } else {
                "Returning from"
            },
            meta.name.as_ref().unwrap_or(&"".to_string()),
        );
        caller.data_mut().migration_ctx.on_migrate(
            &from_node_id,
            reason,
            current_wasm_stack_height,
        );

        Ok(Some(PendingMigration {
            func: meta,
            to_node_id,
            reason,
        }))
    } else {
        Ok(None)
    }
}
