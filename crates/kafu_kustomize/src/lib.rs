use std::path::Path;
use std::process::Command;

use anyhow::{Error, anyhow};
use kafu_config::KafuConfig;
use serde::Serialize;
use tinytemplate::TinyTemplate;

/// Type of the function that builds a kustomize command for a given directory path.
pub type KustomizeCommandBuilder = Box<dyn Fn(&Path) -> Command>;

#[derive(Serialize)]
struct Context {
    /// Logical node ID (key of the nodes map in KafuConfig).
    node_id: String,
    /// Placement group for this node used when targeting Kubernetes.
    placement: String,
    /// Sanitized config name for use in labels (kafu-service-name).
    config_name: String,
    /// ConfigMap resource name. Must match both the emitted ConfigMap metadata.name and the Pod volume configMap.name.
    configmap_name: String,
    /// Sanitized instance ID. Empty when not set; template uses "{{ if instance_id }}" for optional namePrefix and kafu-instance label.
    instance_id: String,
}

pub fn generate_manifest(
    config: &KafuConfig,
    kustomize_cmd: KustomizeCommandBuilder,
    image_override: Option<&str>,
    instance_id: Option<&str>,
) -> Result<String, Error> {
    let configmap_name = instance_id
        .map(|id| format!("kafu-config-{}", sanitize_label_value(id)))
        .unwrap_or_else(|| "kafu-config".to_string());
    let instance_id_sanitized = instance_id.map(sanitize_label_value).unwrap_or_default();

    // When instance_id is set, update node addresses in the config to match the prefixed Service names.
    let config_for_cm = if let Some(instance_id_val) = instance_id {
        let mut config_clone = config.clone();
        for (node_id, node_config) in config_clone.nodes.iter_mut() {
            // Update address: kafu-server-<node-id> -> <instance-id>-kafu-server-<node-id>
            // Also handle FQDN: kafu-server-<node-id>.<namespace>.svc.cluster.local -> <instance-id>-kafu-server-<node-id>.<namespace>.svc.cluster.local
            let prefix = format!("kafu-server-{}", node_id);
            if node_config.address.starts_with(&prefix) {
                let suffix = &node_config.address[prefix.len()..];
                node_config.address =
                    format!("{}-kafu-server-{}{}", instance_id_val, node_id, suffix);
            }
        }
        config_clone
    } else {
        config.clone()
    };

    // Serialize the config to YAML and build a ConfigMap resource.
    let config_yaml = serde_yaml::to_string(&config_for_cm)?;
    let config_name_sanitized = sanitize_label_value(&config.name);
    let configmap_yaml = build_configmap_yaml(
        &config_yaml,
        &configmap_name,
        &config_name_sanitized,
        &instance_id_sanitized,
    )?;

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
            placement: node_config
                .placement
                .clone()
                .unwrap_or_else(|| node_id.clone()),
            config_name: sanitize_label_value(&config.name),
            configmap_name: configmap_name.clone(),
            instance_id: instance_id_sanitized.clone(),
        };

        // render
        let node_dir = overlays_dir.join(node_id);
        std::fs::create_dir_all(&node_dir)?;
        std::fs::write(
            node_dir.join("kustomization.yaml"),
            tt.render("kustomization", &context)?,
        )?;
        std::fs::write(node_dir.join("patch.yaml"), tt.render("patch", &context)?)?;

        // Invoke kustomize build (standalone) or kubectl kustomize (path-only).
        let output = kustomize_cmd(node_dir.as_path()).output()?;

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

/// Sanitize a string for use as a Kubernetes label value (max 63 chars, alphanumeric at start/end).
fn sanitize_label_value(s: &str) -> String {
    const MAX_LEN: usize = 63;
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out = out
        .trim_matches(|c| c == '-' || c == '_' || c == '.')
        .to_string();
    if out.len() > MAX_LEN {
        out.truncate(MAX_LEN);
        out = out
            .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_string();
    }
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

/// Build a ConfigMap YAML resource embedding the given kafu config content.
/// Labels match Pod labels except kafu-node-id (component, kafu-service-name, and optionally kafu-instance).
fn build_configmap_yaml(
    config_yaml: &str,
    configmap_name: &str,
    config_name: &str,
    instance_id: &str,
) -> Result<String, Error> {
    let mut config_content: String = config_yaml
        .lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_string()
            } else {
                format!("    {}\n", line)
            }
        })
        .collect();
    if config_content.ends_with('\n') {
        config_content.pop();
    }
    let mut tt = TinyTemplate::new();
    tt.add_template("configmap", include_str!("../templates/configmap.yaml.tpl"))?;
    #[derive(Serialize)]
    struct ConfigMapContext<'a> {
        configmap_name: &'a str,
        config_content: &'a str,
        config_name: &'a str,
        instance_id: &'a str,
    }
    let context = ConfigMapContext {
        configmap_name,
        config_content: &config_content,
        config_name,
        instance_id,
    };
    Ok(tt.render("configmap", &context)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_label_value_valid() {
        assert_eq!(sanitize_label_value("basic"), "basic");
        assert_eq!(sanitize_label_value("my-app"), "my-app");
        assert_eq!(sanitize_label_value("a"), "a");
    }

    #[test]
    fn sanitize_label_value_invalid_chars() {
        assert_eq!(sanitize_label_value("my app"), "my-app");
        assert_eq!(sanitize_label_value("foo/bar"), "foo-bar");
    }

    #[test]
    fn sanitize_label_value_empty_fallback() {
        assert_eq!(sanitize_label_value(""), "default");
        assert_eq!(sanitize_label_value("---"), "default");
    }

    #[test]
    fn sanitize_label_value_truncate() {
        let long = "a".repeat(70);
        let got = sanitize_label_value(&long);
        assert!(got.len() <= 63);
        assert!(!got.is_empty());
    }
}
