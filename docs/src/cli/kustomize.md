# Kafu Kustomize

The `kafu kustomize` command generates Kubernetes manifests from a Kafu configuration file.

> **Warning**: This feature is experimental and may be broken or behave unexpectedly.

## Prerequisites

Install [Kustomize](https://github.com/kubernetes-sigs/kustomize) and make it available from your terminal.

Kustomize is typically included with `kubectl`. To verify:

```sh
kubectl kustomize version
```

Alternatively, you can install Kustomize as a standalone tool:

```sh
curl -s "https://raw.githubusercontent.com/kubernetes-sigs/kustomize/master/hack/install_kustomize.sh" | bash
```

## Usage

```sh
kafu kustomize build <CONFIG_PATH>
```

## Commands

### `build`

Generate Kubernetes manifests to stdout.

## Arguments

- `<CONFIG_PATH>` (required): Path to the Kafu configuration file.

## Output

The command outputs a Kubernetes manifest to stdout. You can redirect it to a file:

```sh
kafu kustomize build kafu-config.yaml > ./kafu-manifest.yaml
```

Or pipe it directly to `kubectl`:

```sh
kafu kustomize build kafu-config.yaml | kubectl apply -f -
```

## What it generates

For each node defined in the config, the generated manifests include:

- A **Pod** to run the node
- A **Service** for node-to-node communication

## Configuration

The command uses the same configuration format as other Kafu commands. For details, see [Kafu Config](../kafu-config.md).

For Kubernetes deployments, it's recommended to use `app.url` to download WASM modules from a URL (e.g., from object storage like S3 or GCS) rather than `app.path`.

## Examples

```sh
# Generate manifest and save to file
kafu kustomize build kafu-config.yaml > kafu-manifest.yaml

# Apply to Kubernetes cluster
kubectl apply -f kafu-manifest.yaml
```

## See also

- [Kafu Config](../kafu-config.md)
