use std::sync::{Arc, RwLock};

use kafu_runtime::engine::{
    KafuRuntimeConfig, KafuRuntimeInstance, LinkerConfig, LinkerSnapifyConfig, WasiConfig,
    WasmModule,
};

#[tokio::test]
async fn wat_can_run_start() -> anyhow::Result<()> {
    let wat_src = r#"
(module
  (func (export "_start"))
)
"#;

    let wasm = wat::parse_str(wat_src)?;
    let module = Arc::new(WasmModule::new(wasm).await?);

    // In this test, avoid unnecessary imports/links and start with the minimal configuration.
    let config = KafuRuntimeConfig {
        node_id: "test-node".to_string(),
        wasi_config: WasiConfig::default(),
        linker_config: LinkerConfig {
            wasip1: false,
            wasi_nn: false,
            spectest: false,
            kafu_helper: false,
            snapify: LinkerSnapifyConfig::Disabled,
        },
    };

    let mut instance = KafuRuntimeInstance::new(module, &config).await?;
    instance.start().await?;
    Ok(())
}

#[tokio::test]
async fn wasip1_hello_world_stdout_matches() -> anyhow::Result<()> {
    let wat_src = r#"
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))

  (memory (export "memory") 1)

  ;; iovec[0] = { ptr=8, len=12 }
  (data (i32.const 8) "hello world\n")

  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 8))
    (i32.store (i32.const 4) (i32.const 12))
    ;; fd_write(fd=1, iovs=0, iovs_len=1, nwritten=20)
    (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20))
    drop)
)
"#;

    let wasm = wat::parse_str(wat_src)?;
    let module = Arc::new(WasmModule::new(wasm).await?);

    let stdout = Arc::new(RwLock::new(Vec::<u8>::new()));
    let wasi_config = WasiConfig {
        stdout: Some(stdout.clone()),
        ..WasiConfig::default()
    };

    let config = KafuRuntimeConfig {
        node_id: "test-node".to_string(),
        wasi_config,
        linker_config: LinkerConfig {
            wasip1: true,
            wasi_nn: false,
            spectest: false,
            kafu_helper: false,
            snapify: LinkerSnapifyConfig::Disabled,
        },
    };

    let mut instance = KafuRuntimeInstance::new(module, &config).await?;
    instance.start().await?;

    let out = String::from_utf8(stdout.read().unwrap().clone())?;
    assert_eq!(out, "hello world\n");
    Ok(())
}
