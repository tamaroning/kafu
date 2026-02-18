use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Error;
use clap::{Parser, Subcommand};
use kafu_config::KafuConfig;
use kafu_kustomize::{KustomizeCommandBuilder, generate_manifest};

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
        /// Instance ID so the same config can be deployed multiple times in one namespace (unique resource names and labels).
        #[clap(long)]
        instance_id: Option<String>,
    },
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    // check kustomize is available.
    let kustomize_cmd = detect_kustomize_command()?;

    match cli.subcommand {
        Commands::Build {
            path,
            image,
            instance_id,
        } => {
            let config = KafuConfig::load(&path)
                .map_err(|e| anyhow::anyhow!("Failed to load Kafu config: {}", e))?;
            let output = generate_manifest(
                &config,
                kustomize_cmd,
                image.as_deref(),
                instance_id.as_deref(),
            )?;
            println!("{}", output);
        }
    }
    Ok(())
}

fn detect_kustomize_command() -> Result<KustomizeCommandBuilder, Error> {
    // First, try standalone kustomize command (usage: kustomize build <path>)
    if Command::new("kustomize").arg("version").output().is_ok() {
        return Ok(Box::new(|path: &Path| {
            let mut cmd = Command::new("kustomize");
            cmd.arg("build").arg(path);
            cmd
        }));
    }

    // If not available, try kubectl kustomize (usage: kubectl kustomize <path>; no "build" subcommand)
    if Command::new("kubectl")
        .arg("kustomize")
        .arg("--help")
        .output()
        .is_ok()
    {
        return Ok(Box::new(|path: &Path| {
            let mut cmd = Command::new("kubectl");
            cmd.arg("kustomize").arg(path);
            cmd
        }));
    }

    Err(anyhow::anyhow!(
        "Neither 'kustomize' nor 'kubectl kustomize' command is available. Please install kustomize or kubectl."
    ))
}
