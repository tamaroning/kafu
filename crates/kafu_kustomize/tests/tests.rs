use std::process::Command;

use kafu_config::KafuConfig;
use kafu_kustomize::generate_manifest;

#[test]
fn test_generate_manifest() {
    let config = KafuConfig::load("tests/fixtures/basic-input.yaml").unwrap();
    let manifest =
        generate_manifest(&config, Box::new(|| Command::new("kustomize")), None).unwrap();
    assert_eq!(manifest, include_str!("fixtures/basic-output.yaml"));
}
