use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use kafu_config::KafuConfig;

#[derive(Clone)]
pub struct WasiConfig {
    pub args: Vec<String>,
    pub preopened_dir: Option<PathBuf>,
    pub inherit_stdin: bool,
    pub inherit_stdout: bool,
    pub inherit_stderr: bool,
    pub inherit_env: bool,
    /// Capture stdout into this buffer when set.
    pub stdout: Option<Arc<RwLock<Vec<u8>>>>,
    /// Capture stderr into this buffer when set.
    pub stderr: Option<Arc<RwLock<Vec<u8>>>>,
}

impl WasiConfig {
    pub fn create_from_kafu_config(kafu_config: &KafuConfig) -> Self {
        Self {
            args: kafu_config.app.args.clone(),
            preopened_dir: kafu_config.app.preopened_dir.clone(),
            inherit_stdin: true,
            inherit_stdout: true,
            inherit_stderr: true,
            inherit_env: false,
            stdout: None,
            stderr: None,
        }
    }
}

impl Default for WasiConfig {
    fn default() -> Self {
        Self {
            args: vec![],
            preopened_dir: None,
            inherit_stdin: true,
            inherit_stdout: true,
            inherit_stderr: true,
            inherit_env: false,
            stdout: None,
            stderr: None,
        }
    }
}

#[derive(Clone)]
pub struct KafuRuntimeConfig {
    pub node_id: String,
    pub wasi_config: WasiConfig,
    pub linker_config: LinkerConfig,
}

#[derive(Debug, Clone)]
pub struct LinkerConfig {
    pub wasip1: bool,
    pub wasi_nn: bool,
    pub spectest: bool,
    pub kafu_helper: bool,
    pub snapify: LinkerSnapifyConfig,
}

impl Default for LinkerConfig {
    fn default() -> Self {
        Self {
            wasip1: true,
            wasi_nn: true,
            spectest: true,
            kafu_helper: true,
            snapify: LinkerSnapifyConfig::Enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkerSnapifyConfig {
    /// Link snapify imports.
    Enabled,
    /// Link snapify imports but use dummy implementation.
    Dummy,
    /// Do not link snapify imports.
    Disabled,
}
