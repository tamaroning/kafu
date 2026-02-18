use std::path::Path;
use std::process::Command;

use kafu_config::KafuConfig;
use kafu_kustomize::{KustomizeCommandBuilder, generate_manifest};

fn kustomize_cmd() -> KustomizeCommandBuilder {
    Box::new(|path: &Path| {
        let mut cmd = Command::new("kustomize");
        cmd.arg("build").arg(path);
        cmd
    })
}

#[test]
fn test_generate_manifest() {
    let config = KafuConfig::load("tests/fixtures/basic-input.yaml").unwrap();
    let manifest = generate_manifest(&config, kustomize_cmd(), None, None).unwrap();
    assert_eq!(manifest, include_str!("fixtures/basic-output.yaml"));
}

#[test]
fn test_generate_manifest_with_instance_id() {
    let config = KafuConfig::load("tests/fixtures/basic-input.yaml").unwrap();
    let manifest = generate_manifest(&config, kustomize_cmd(), None, Some("staging")).unwrap();
    assert_eq!(manifest, include_str!("fixtures/with-instance-output.yaml"));
}
