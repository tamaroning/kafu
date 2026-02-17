use std::{path::PathBuf, process::Command};

use anyhow::Error;
use clap::{Parser, Subcommand};
use kafu_config::KafuConfig;
use kafu_kustomize::generate_manifest;

/// A tool to generate Kubernetes manifest from Kafu config.
#[derive(Parser)]
struct Cli {
    #[clap(subcommand)]
    subcommand: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build {
        /// Path to the Kafu config file.
        path: PathBuf,
        /// Override container image used for kafu-server Pods.
        #[clap(long)]
        image: Option<String>,
    },
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    // check kustomize is available.
    let kustomize_cmd = detect_kustomize_command()?;

    match cli.subcommand {
        Commands::Build { path, image } => {
            let config = KafuConfig::load(&path)
                .map_err(|e| anyhow::anyhow!("Failed to load Kafu config: {}", e))?;
            let output = generate_manifest(&config, kustomize_cmd, image.as_deref())?;
            println!("{}", output);
        }
    }
    Ok(())
}

fn detect_kustomize_command() -> Result<Box<dyn Fn() -> Command>, Error> {
    // First, try standalone kustomize command
    if Command::new("kustomize").arg("version").output().is_ok() {
        return Ok(Box::new(|| Command::new("kustomize")));
    }

    // If not available, try kubectl kustomize
    if Command::new("kubectl")
        .arg("kustomize")
        .arg("--help")
        .output()
        .is_ok()
    {
        return Ok(Box::new(|| {
            let mut cmd = Command::new("kubectl");
            cmd.arg("kustomize");
            cmd
        }));
    }

    Err(anyhow::anyhow!(
        "Neither 'kustomize' nor 'kubectl kustomize' command is available. Please install kustomize or kubectl."
    ))
}
