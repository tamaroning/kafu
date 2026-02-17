use anyhow::Result;

use super::kafu_metadata::{self, KafuModuleMetadata};

pub struct WasmModule {
    pub(crate) wasm: Vec<u8>,
    pub(crate) metadata: KafuModuleMetadata,
}

impl WasmModule {
    /// Create a new `WasmModule` from a WASM binary.
    pub async fn new(wasm: Vec<u8>) -> Result<Self> {
        let metadata = kafu_metadata::go(&wasm)?;
        Ok(Self { wasm, metadata })
    }
}
