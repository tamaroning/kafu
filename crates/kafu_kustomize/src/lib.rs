use std::process::Command;

use anyhow::{Error, anyhow};
use kafu_config::KafuConfig;
use serde::Serialize;
use tinytemplate::TinyTemplate;

#[derive(Serialize)]
struct Context {
    /// Logical node ID (key of the nodes map in KafuConfig).
    node_id: String,
    /// Placement group for this node used when targeting Kubernetes.
    ///
    /// This is typically mapped to a Kubernetes node label so that multiple logical
    /// Kafu nodes can share the same physical Kubernetes node.
    placement: String,
}

pub fn generate_manifest(
    config: &KafuConfig,
    kustomize_cmd: Box<dyn Fn() -> Command>,
    image_override: Option<&str>,
) -> Result<String, Error> {
    // Serialize the config to YAML and build a ConfigMap resource.
    let config_yaml = serde_yaml::to_string(config)?;
    let configmap_yaml = build_configmap_yaml(&config_yaml);

    // Prepare a temporary directory.
    let temp_dir = tempfile::tempdir()?;
    // Create a directory named `bases` under the temporary directory.
    let base_dir = temp_dir.path().join("bases");
    std::fs::create_dir_all(&base_dir)?;

    // Create the base directory with the following files:
    // - kustomization.yaml
    // - kafu-server-pod.yaml
    // - kafu-server-service.yaml
    let kustomization_yaml = include_bytes!("../templates/bases/kustomization.yaml");
    let kafu_server_pod_yaml_bytes = include_bytes!("../templates/bases/kafu-server-pod.yaml");
    let kafu_server_service_yaml = include_bytes!("../templates/bases/kafu-server-service.yaml");

    // Optionally override the container image used in the base Pod manifest.
    let mut kafu_server_pod_yaml = String::from_utf8_lossy(kafu_server_pod_yaml_bytes).into_owned();
    if let Some(image) = image_override {
        kafu_server_pod_yaml = override_pod_image(&kafu_server_pod_yaml, image)?;
    }

    std::fs::write(base_dir.join("kustomization.yaml"), kustomization_yaml)?;
    std::fs::write(base_dir.join("kafu-server-pod.yaml"), kafu_server_pod_yaml)?;
    std::fs::write(
        base_dir.join("kafu-server-service.yaml"),
        kafu_server_service_yaml,
    )?;

    // Create a directory named `overlays`.
    let overlays_dir = temp_dir.path().join("overlays");
    std::fs::create_dir_all(&overlays_dir)?;

    // Emit the shared ConfigMap once at the top of the manifest.
    let mut manifest = configmap_yaml;

    for (node_id, node_config) in config.nodes.iter() {
        let mut tt = TinyTemplate::new();
        tt.add_template(
            "kustomization",
            include_str!("../templates/overlays/kustomization.yaml.tpl"),
        )?;
        tt.add_template(
            "patch",
            include_str!("../templates/overlays/patch.yaml.tpl"),
        )?;

        let context = Context {
            node_id: node_id.clone(),
            // If placement is not specified, fall back to node_id so that existing
            // configs behave exactly as before (1:1 mapping between node ID and
            // Kubernetes node label).
            placement: node_config
                .placement
                .clone()
                .unwrap_or_else(|| node_id.clone()),
        };

        // render
        let node_dir = overlays_dir.join(node_id);
        std::fs::create_dir_all(&node_dir)?;
        std::fs::write(
            node_dir.join("kustomization.yaml"),
            tt.render("kustomization", &context)?,
        )?;
        std::fs::write(node_dir.join("patch.yaml"), tt.render("patch", &context)?)?;

        // invoke kustomize build
        let output = kustomize_cmd().arg("build").arg(&node_dir).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to run kustomize build for node {}: {}",
                node_id,
                stderr
            ));
        }

        manifest.push_str("---\n");
        manifest.push_str(&String::from_utf8_lossy(&output.stdout));
    }

    Ok(manifest)
}

/// Build a ConfigMap YAML resource embedding the given kafu config content.
fn build_configmap_yaml(config_yaml: &str) -> String {
    let mut result = String::new();
    result.push_str("apiVersion: v1\n");
    result.push_str("kind: ConfigMap\n");
    result.push_str("metadata:\n");
    result.push_str("  name: kafu-config\n");
    result.push_str("data:\n");
    result.push_str("  kafu-config.yaml: |\n");
    for line in config_yaml.lines() {
        if line.is_empty() {
            result.push('\n');
        } else {
            result.push_str("    ");
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

/// Override the container image field in the base Pod template.
fn override_pod_image(pod_yaml: &str, image: &str) -> Result<String, Error> {
    let mut result = String::new();
    let mut replaced = false;

    for line in pod_yaml.lines() {
        if !replaced {
            let trimmed = line.trim_start();
            if trimmed.starts_with("image:") {
                // Preserve original indentation.
                let indent_len = line.len() - trimmed.len();
                let indent = &line[..indent_len];
                result.push_str(indent);
                result.push_str("image: ");
                result.push_str(image);
                result.push('\n');
                replaced = true;
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }

    if !replaced {
        return Err(anyhow!(
            "Failed to find image field in kafu-server-pod.yaml to override"
        ));
    }

    Ok(result)
}
