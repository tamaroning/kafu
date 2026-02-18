use std::path::Path;
use std::process::Command;

use kafu_config::KafuConfig;
use kafu_kustomize::{KustomizeCommandBuilder, generate_manifest};

#[test]
fn test_generate_manifest() {
    let config = KafuConfig::load("tests/fixtures/basic-input.yaml").unwrap();
    let kustomize_cmd: KustomizeCommandBuilder = Box::new(|path: &Path| {
        let mut cmd = Command::new("kustomize");
        cmd.arg("build").arg(path);
        cmd
    });
    let manifest = generate_manifest(&config, kustomize_cmd, None).unwrap();
    assert_eq!(manifest, include_str!("fixtures/basic-output.yaml"));
}
