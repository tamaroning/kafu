use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KafuConfig {
    /// Name of the Kafu service.
    pub name: String,
    /// Application configuration.
    pub app: AppConfig,
    /// Node list in the Kafu service.
    /// Key is the node name.
    /// condition: nodes is not empty
    pub nodes: IndexMap<String, NodeConfig>,
    /// Cluster-wide behavior configuration (optional).
    ///
    /// Backward compatible: if omitted, defaults are used.
    #[serde(default)]
    pub cluster: ClusterConfig,

    /// Directory where the Kafu config file is located.
    /// This is used as a base directory when the wasm binary is specified as a relative path.
    #[serde(skip)]
    kafu_config_dir: PathBuf,
}

impl KafuConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let mut config: KafuConfig = serde_yaml::from_reader(
            std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e))?,
        )
        .map_err(|e| format!("Failed to parse YAML: {}", e))?;

        let path = path
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize path: {}", e))?;
        config.kafu_config_dir = path
            .parent()
            .ok_or_else(|| format!("Failed to get parent directory of path: {}", path.display()))?
            .to_path_buf();
        config.validate()?;

        Ok(config)
    }

    pub fn get_wasm_location(&self) -> WasmLocation {
        if let Some(path) = &self.app.path {
            if path.is_relative() {
                return WasmLocation::Path(self.kafu_config_dir.join(path));
            } else {
                return WasmLocation::Path(path.clone());
            }
        }

        if let Some(url) = &self.app.url {
            return WasmLocation::Url(url.clone());
        }

        unreachable!();
    }

    fn validate(&self) -> Result<()> {
        if !self.kafu_config_dir.is_dir() {
            return Err(format!(
                "Broken Kafu config path: kafu_config_dir is not a directory: {}",
                self.kafu_config_dir.display()
            ));
        }

        if self.name.is_empty() {
            return Err("Name is required in the name field".to_string());
        }

        if self.nodes.is_empty() {
            return Err("At least one node is required in the nodes field".to_string());
        }

        for (node_id, node_config) in self.nodes.iter() {
            if node_id.is_empty() {
                return Err("Node ID must not be empty".to_string());
            }

            if node_config.address.is_empty() {
                return Err("Address must not be empty".to_string());
            }
        }

        if self.app.path.is_some() && self.app.url.is_some() {
            return Err("Only one of path or url can be specified".to_string());
        }

        if self.app.path.is_none() && self.app.url.is_none() {
            return Err("One of path or url must be specified".to_string());
        }

        Ok(())
    }
}

pub enum WasmLocation {
    Path(PathBuf),
    Url(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ClusterConfig {
    /// Heartbeat / health-check monitoring policy.
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,

    /// Migration-related options.
    #[serde(default)]
    pub migration: MigrationConfig,
}

/// Memory migration strategy: send full snapshot every time, or only changed pages (delta) when the receiver has the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum MemoryMigrationMode {
    /// Always send full main memory (no delta).
    Full,
    /// Send only changed 64KB pages when the receiver has the baseline; otherwise send full.
    #[default]
    Delta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationConfig {
    /// Compress main memory with LZ4 when sending (reduces transfer size).
    ///
    /// This applies to:
    /// - Delta path: compress changed pages
    /// - Full snapshot path: compress full main memory blob
    ///
    /// Default: true.
    #[serde(
        rename = "memory_compression",
        alias = "delta_compression",
        alias = "memroy_compression",
        default = "migration_memory_compression_default"
    )]
    pub memory_compression: bool,

    /// Memory migration: "full" (always send full snapshot) or "delta" (send only changed pages when receiver has baseline).
    #[serde(default)]
    pub memory_migration: MemoryMigrationMode,
}

fn migration_memory_compression_default() -> bool {
    true
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            memory_compression: true,
            memory_migration: MemoryMigrationMode::Delta,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeartbeatConfig {
    /// Behavior for non-coordinator nodes when the coordinator becomes unreachable.
    ///
    /// Default: ignore (do nothing), which avoids cascading shutdowns on transient network issues.
    #[serde(default)]
    pub follower_on_coordinator_lost: FollowerOnCoordinatorLost,

    /// Heartbeat interval in milliseconds.
    ///
    /// Default: 1000ms.
    #[serde(default = "HeartbeatConfig::default_interval_ms")]
    pub interval_ms: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            follower_on_coordinator_lost: FollowerOnCoordinatorLost::default(),
            interval_ms: Self::default_interval_ms(),
        }
    }
}

impl HeartbeatConfig {
    fn default_interval_ms() -> u64 {
        1_000
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FollowerOnCoordinatorLost {
    /// Shut down this node (send shutdown signal).
    #[default]
    ShutdownSelf,
    /// Do nothing (log only).
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Path to the Wasm binary to run.
    /// If relative path is specified, it is relative to the directory where the Kafu config file is located.
    /// condition: Only one of path or url must be specified
    path: Option<PathBuf>,
    /// URL of the Wasm binary to run.
    /// condition: Only one of path or url must be specified
    pub url: Option<String>,
    /// Arguments to pass to the Wasm binary.
    pub args: Vec<String>,
    /// Preopened directory for the Wasm binary.
    pub preopened_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// IP address and port of the node.
    pub address: String,
    /// Port of the node.
    pub port: u16,
    /// Optional logical placement group for this node when running on orchestrators such as Kubernetes.
    ///
    /// This value is not used by the core runtime itself, but by integration tools
    /// (e.g. `kafu kustomize`) to decide how nodes are placed onto physical machines.
    /// For example, when generating Kubernetes manifests, this value can be mapped to
    /// a node label so that multiple logical Kafu nodes (Pods) share the same
    /// underlying Kubernetes node.
    ///
    /// Backward compatible: if omitted, tools should fall back to the node ID.
    #[serde(default)]
    pub placement: Option<String>,
}
