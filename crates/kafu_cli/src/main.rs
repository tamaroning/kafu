use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "kafu", about)]
struct Cli {
    /// Invoked built-in subcommand
    #[command(subcommand)]
    cmd: Option<KnownSubcommand>,

    /// Invoked installed subcommand name
    #[arg(value_name = "SUBCOMMAND", allow_hyphen_values = true)]
    installed: Option<OsString>,

    /// Forward all arguments to the subcommand
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    installed_args: Vec<OsString>,

    #[arg(long)]
    version: bool,
}

#[derive(Subcommand, Debug)]
enum KnownSubcommand {
    /// Diagnose installation and environment problems.
    Doctor {
        /// Show more details (such as discovered subcommand paths).
        #[arg(short, long)]
        verbose: bool,
    },
    /// List available installed subcommands.
    List {
        /// Print full paths in addition to names.
        #[arg(short, long)]
        long: bool,
    },
}

fn main() -> ExitCode {
    // Prevent clap from panicking on unknown subcommands
    // This cannot be specified with derive alone, so we manipulate Command and parse
    let mut cmd = Cli::command();
    cmd = cmd.allow_external_subcommands(true);

    // First process built-in subcommands with clap, and if unknown, fall back to installed ones
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("arg match");

    // Handle --version flag
    if cli.version {
        println!("Kafu CLI {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    match cli.cmd {
        Some(KnownSubcommand::Doctor { verbose }) => doctor(verbose),
        Some(KnownSubcommand::List { long }) => list_plugins(long),
        None => match &cli.installed {
            Some(name) => {
                let Ok(kafu_sdk_path) = std::env::var("KAFU_SDK_PATH") else {
                    eprintln!("KAFU_SDK_PATH is not set");
                    return ExitCode::FAILURE;
                };

                let name = name.to_string_lossy().to_string();
                let bin = format!("kafu_{name}");
                let bin = PathBuf::from(kafu_sdk_path).join("libexec").join(bin);
                let forwarded: Vec<OsString> = cli
                    .installed_args
                    .iter()
                    .map(|v| v.to_os_string())
                    .collect::<Vec<_>>();
                exec_or_spawn(&bin, forwarded)
            }
            None => {
                eprintln!("no subcommand. try `kafu --help`");
                ExitCode::FAILURE
            }
        },
    }
}

fn doctor(verbose: bool) -> ExitCode {
    let mut has_error = false;

    println!("Kafu CLI {}", env!("CARGO_PKG_VERSION"));

    let kafu_sdk_path = std::env::var("KAFU_SDK_PATH").ok();
    let wasi_sdk_path = std::env::var("WASI_SDK_PATH").ok();

    match &kafu_sdk_path {
        Some(v) if !v.is_empty() => println!("KAFU_SDK_PATH: {v}"),
        _ => {
            println!("KAFU_SDK_PATH: not set");
            println!("  - fix: export KAFU_SDK_PATH=/path/to/kafu-sdk");
            has_error = true;
        }
    }

    match &wasi_sdk_path {
        Some(v) if !v.is_empty() => println!("WASI_SDK_PATH: {v}"),
        _ => {
            println!("WASI_SDK_PATH: not set");
            println!("  - note: required by `kafu clang`");
        }
    }

    if let Some(kafu_sdk_path) = &kafu_sdk_path {
        let sdk_dir = PathBuf::from(kafu_sdk_path);
        check_dir_exists(&sdk_dir, "KAFU_SDK_PATH", &mut has_error);

        let libexec_dir = sdk_dir.join("libexec");
        check_dir_exists(&libexec_dir, "$KAFU_SDK_PATH/libexec", &mut has_error);

        let plugins = discover_plugins(&libexec_dir);
        match plugins {
            Ok(plugins) => {
                println!("installed subcommands: {}", plugins.len());
                if verbose {
                    for p in &plugins {
                        println!("  - {} ({})", p.name, p.path.display());
                    }
                }
                if plugins.is_empty() {
                    println!(
                        "  - note: no `kafu_*` found under {}",
                        libexec_dir.display()
                    );
                }
            }
            Err(err) => {
                println!(
                    "installed subcommands: failed to scan {}: {err}",
                    libexec_dir.display()
                );
                has_error = true;
            }
        }
    }

    if has_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn list_plugins(long: bool) -> ExitCode {
    let Ok(kafu_sdk_path) = std::env::var("KAFU_SDK_PATH") else {
        eprintln!("KAFU_SDK_PATH is not set");
        return ExitCode::FAILURE;
    };

    let libexec_dir = PathBuf::from(kafu_sdk_path).join("libexec");
    match discover_plugins(&libexec_dir) {
        Ok(mut plugins) => {
            plugins.sort_by(|a, b| a.name.cmp(&b.name));
            for p in plugins {
                if long {
                    println!("{}\t{}", p.name, p.path.display());
                } else {
                    println!("{}", p.name);
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("failed to scan {}: {err}", libexec_dir.display());
            ExitCode::FAILURE
        }
    }
}

fn check_dir_exists(path: &Path, label: &str, has_error: &mut bool) {
    if !path.exists() {
        println!("{label}: {} (missing)", path.display());
        *has_error = true;
        return;
    }
    if !path.is_dir() {
        println!("{label}: {} (not a directory)", path.display());
        *has_error = true;
    }
}

#[derive(Debug, Clone)]
struct Plugin {
    name: String,
    path: PathBuf,
}

fn discover_plugins(dir: &Path) -> std::io::Result<Vec<Plugin>> {
    let mut out = Vec::new();
    let rd = fs::read_dir(dir)?;
    for ent in rd {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(rest) = file_name.strip_prefix("kafu_") else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        out.push(Plugin {
            name: rest.to_string(),
            path,
        });
    }
    Ok(out)
}

/// Spawn `kafu_<name>` and return its exit code
fn exec_or_spawn(bin: &Path, args: Vec<OsString>) -> ExitCode {
    let mut cmd = Command::new(bin);
    cmd.args(&args);

    match cmd.spawn() {
        Ok(mut child) => match child.wait() {
            Ok(status) => status
                .code()
                .map(|code| ExitCode::from(code as u8))
                .unwrap_or(ExitCode::FAILURE),
            Err(err) => {
                eprintln!("failed to wait for `{}`: {}", bin.display(), err);
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("failed to spawn `{}`: {}", bin.display(), err);
            ExitCode::from(127)
        }
    }
}
