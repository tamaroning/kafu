use anyhow::{Context as _, Result};
use wasi_common::pipe::WritePipe;
use wasi_common::sync;
use wasmtime::{Caller, Linker, Store};

use super::config::{LinkerConfig, LinkerSnapifyConfig};
use super::migration::handle_migration_point;
use super::migration::InterruptReason;
use super::store::KafuStore;

pub(crate) fn link_imports(
    config: &LinkerConfig,
    linker: &mut Linker<KafuStore>,
    store: &mut Store<KafuStore>,
) -> Result<()> {
    if config.wasip1 {
        wasi_common::sync::add_to_linker(linker, |cx| &mut cx.libctx.wasi)?;
    }
    if config.wasi_nn {
        link_wasi_nn_imports(linker)?;
    }
    if config.spectest {
        link_spectest_imports(linker, store)?;
    }
    if config.kafu_helper {
        link_kafu_helper_imports(linker)?;
    }
    match config.snapify {
        LinkerSnapifyConfig::Enabled => link_snapify_imports(linker)?,
        LinkerSnapifyConfig::Dummy => link_snapify_imports_dummy(linker)?,
        LinkerSnapifyConfig::Disabled => {}
    }
    Ok(())
}

pub(crate) fn wasi_ctx(wasi_config: &super::config::WasiConfig) -> Result<wasi_common::WasiCtx> {
    let mut builder = sync::WasiCtxBuilder::new();
    builder.arg("program")?;
    for arg in wasi_config.args.iter() {
        builder.arg(arg)?;
    }
    builder.inherit_env()?;
    builder.inherit_stdin();
    if let Some(stdout) = wasi_config.stdout.clone() {
        builder.stdout(Box::new(WritePipe::from_shared(stdout)));
    } else {
        builder.inherit_stdout();
    }
    if let Some(stderr) = wasi_config.stderr.clone() {
        builder.stderr(Box::new(WritePipe::from_shared(stderr)));
    } else {
        builder.inherit_stderr();
    }

    let preopen = wasi_config
        .preopened_dir
        .clone()
        .unwrap_or_else(|| ".".into());
    let dir = cap_std::fs::Dir::from_std_file(std::fs::File::open(preopen)?);
    builder.preopened_dir(dir, "/")?;

    Ok(builder.build())
}

fn link_snapify_imports(linker: &mut Linker<KafuStore>) -> Result<()> {
    linker.func_wrap(
        "snapify",
        "should_checkpoint",
        move |mut caller: Caller<'_, KafuStore>, reason: i32| -> i32 {
            let reason = InterruptReason::new(reason);
            match handle_migration_point(&mut caller, reason) {
                Ok(Some(pending)) => {
                    // Suspend the program.
                    caller.data_mut().migration_ctx.pending_migration_request = Some(pending);
                    1
                }
                Ok(None) => 0,
                Err(e) => {
                    tracing::warn!("failed to handle migration point: {e:#}");
                    0
                }
            }
        },
    )?;
    Ok(())
}

/// Dummy implementation for singlenode emulation.
fn link_snapify_imports_dummy(linker: &mut Linker<KafuStore>) -> Result<()> {
    linker.func_wrap(
        "snapify",
        "should_checkpoint",
        move |mut caller: Caller<'_, KafuStore>, reason: i32| -> i32 {
            let reason = InterruptReason::new(reason);
            match handle_migration_point(&mut caller, reason) {
                Ok(Some(pending)) => {
                    // For singlenode emulation, just switch node_id.
                    caller.data_mut().node_id = pending.to_node_id;
                    0
                }
                Ok(None) => 0,
                Err(e) => {
                    tracing::warn!("failed to handle migration point (dummy): {e:#}");
                    0
                }
            }
        },
    )?;
    Ok(())
}

fn link_spectest_imports<C: 'static>(linker: &mut Linker<C>, store: &mut Store<C>) -> Result<()> {
    let config = wasmtime_wast::SpectestConfig {
        use_shared_memory: false,
        suppress_prints: false,
    };
    wasmtime_wast::link_spectest(linker, store, &config)?;
    Ok(())
}

fn link_kafu_helper_imports(linker: &mut Linker<KafuStore>) -> Result<()> {
    crate::witx::add_to_linker(linker, |cx: &mut KafuStore| &mut cx.libctx.kafu_helper)?;
    Ok(())
}

fn link_wasi_nn_imports(linker: &mut Linker<KafuStore>) -> Result<()> {
    wasmtime_wasi_nn::witx::add_to_linker(linker, |cx: &mut KafuStore| &mut cx.libctx.wasi_nn)
        .context("failed to add wasi-nn to linker")?;
    Ok(())
}
