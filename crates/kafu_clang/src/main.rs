use std::{
    path::PathBuf,
    process::{Command, ExitCode},
};

use anyhow::Error;
use clap::Parser;

const DISPLAY_INTERNAL_COMMAND: bool = true;

/// Clang and wasm-opt wrapper for Kafu.
/// Internally, it runs clang and wasm-opt in sequence.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Arguments to pass to the clang compiler.
    /// If -- is passed, it will be passed to clang and exit without running wasm-opt.
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<String>,
}

fn get_clang_path() -> Result<PathBuf, Error> {
    let wasi_sdk_path = match std::env::var("WASI_SDK_PATH") {
        Ok(path) => PathBuf::from(path),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "WASI_SDK_PATH must be set in the environment: {}",
                e
            ));
        }
    };
    let clang_path = wasi_sdk_path.join("bin/clang");
    Ok(clang_path)
}

fn get_kafu_sdk_path() -> Result<PathBuf, Error> {
    let kafu_sdk_path = match std::env::var("KAFU_SDK_PATH") {
        Ok(path) => PathBuf::from(path),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "KAFU_SDK_PATH must be set in the environment: {}",
                e
            ));
        }
    };
    Ok(kafu_sdk_path)
}

fn get_wasm_opt_path() -> Result<PathBuf, Error> {
    let kafu_sdk_path = get_kafu_sdk_path()?;
    let wasm_opt_path = kafu_sdk_path.join("libexec/kafu_wasm-opt");
    Ok(wasm_opt_path)
}

fn get_kafu_sdk_include_path() -> Result<PathBuf, Error> {
    let kafu_sdk_path = get_kafu_sdk_path()?;
    let kafu_sdk_include_path = kafu_sdk_path.join("include");
    Ok(kafu_sdk_include_path)
}

fn main() -> Result<ExitCode, Error> {
    let cli = Cli::parse();

    let clang_path = get_clang_path()?;
    let wasm_opt_path = get_wasm_opt_path()?;

    // If -- is passed, exit with running only the clang command.
    let env_args: Vec<String> = std::env::args().collect();
    let has_double_dash = env_args.iter().any(|arg| arg == "--");
    if has_double_dash {
        let mut clang_cmd = Command::new(clang_path);
        clang_cmd.args(&cli.args);
        run_command("clang", &mut clang_cmd, "Failed to run clang command")?;
        return Ok(ExitCode::SUCCESS);
    }

    // Stage1: Clang
    // Extract output path from -o flag if present, and remove -o <file> from args
    let mut output_path = PathBuf::from("a.out");
    for arg in cli.args.windows(2) {
        if arg[0] == "-o" {
            output_path = PathBuf::from(arg[1].clone());
            break;
        }
    }

    let temp_dir = tempfile::tempdir()?;
    let stage1 = temp_dir.path().join("stage1.wasm");
    let stage2 = temp_dir.path().join("stage2.wasm");
    let stage3 = output_path.clone();

    // Remove -o flag and its following filename from clang args
    let mut clang_args = cli.args.clone();
    let output_index = clang_args.iter().position(|arg| arg == "-o");
    if let Some(output_index) = output_index {
        clang_args.remove(output_index);
        clang_args.remove(output_index);
    }

    // Add -o <stage1.wasm> to clang args
    clang_args.push("-o".to_string());
    clang_args.push(stage1.to_string_lossy().to_string());

    // Add -I $KAFU_SDK_PATH/include to clang args
    clang_args.push("-I".to_string());
    clang_args.push(get_kafu_sdk_include_path()?.to_string_lossy().to_string());

    // Add -Wl,--no-gc-sections to prevent Kafu attributes from being removed by the linker.
    clang_args.push("-Wl,--no-gc-sections".to_string());

    let mut clang_cmd = Command::new(clang_path);
    clang_cmd.args(&clang_args);
    run_command("clang", &mut clang_cmd, "Failed to run clang command")?;

    // Stage2: Snapify
    // wasm-opt stage1.wasm -O0 --enable-multimemory --snapify --pass-arg=policy@kafu -o stage2.wasm
    let mut snapify_cmd = Command::new(&wasm_opt_path);
    snapify_cmd
        .arg(&stage1)
        .arg("-O0")
        .arg("-g")
        .arg("--enable-multimemory")
        .arg("--snapify")
        .arg("--pass-arg=policy@kafu")
        .arg("-o")
        .arg(&stage2);
    run_command(
        "snapify",
        &mut snapify_cmd,
        "Failed to run wasm-opt (snapify) command",
    )?;

    // Stage3: Asyncify
    // wasm-opt stage2.wasm -O3 --enable-multimemory --asyncify --pass-arg=asyncify-memory@snapify_memory -o stage3.wasm
    let mut asyncify_cmd = Command::new(wasm_opt_path);
    asyncify_cmd
        .arg(&stage2)
        .arg("-O3")
        .arg("-g")
        .arg("--enable-multimemory")
        .arg("--asyncify")
        .arg("--pass-arg=asyncify-memory@snapify_memory")
        .arg("-o")
        .arg(&stage3);
    run_command(
        "asyncify",
        &mut asyncify_cmd,
        "Failed to run wasm-opt (asyncify) command",
    )?;

    Ok(ExitCode::SUCCESS)
}

fn display_command(cmd: &Command) {
    if DISPLAY_INTERNAL_COMMAND {
        let program = cmd.get_program().to_string_lossy();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        eprintln!("$ {} {}", program, args.join(" "));
    }
}

fn run_command(_name: &str, cmd: &mut Command, error_msg: &str) -> Result<(), Error> {
    display_command(cmd);
    let status = cmd.status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("{}", error_msg));
    }
    Ok(())
}
