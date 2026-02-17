use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    /// Node ID.
    #[arg(long)]
    pub node_id: String,

    /// Path to the configuration file.
    pub config: PathBuf,
}
