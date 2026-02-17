# Kafu Config

The Kafu configuration file (`kafu-config.yaml`) defines the structure and deployment settings for a Kafu service. It specifies the WebAssembly binary to execute, the nodes in the distributed cluster, and various runtime parameters.

## File Format

The configuration file is written in YAML format. By default, Kafu looks for a file named `kafu-config.yaml` in the current directory, but you can use any filename by passing it explicitly to Kafu commands.

## Configuration Structure

### Top-Level Fields

- **`name`** (required): A string identifying the Kafu service. This name must be non-empty and is used for service identification.

- **`app`** (required): Application configuration that defines the WebAssembly binary and its execution parameters.

- **`cluster`** (optional): Cluster-wide behavior configuration. If omitted, defaults are used (backward compatible).

- **`nodes`** (required): A map of node configurations. Each key is a node ID (identifier), and the value is a node configuration. At least one node must be specified. The first node in the map is the node that starts the execution of the WebAssembly binary.

### App Configuration

The `app` section contains the following fields:

- **`path`** (optional): A file path to the WebAssembly binary. If a relative path is specified, it is resolved relative to the directory where the `kafu-config.yaml` file is located. Either `path` or `url` must be specified, but not both.

- **`url`** (optional): A URL from which to download the WebAssembly binary. Either `path` or `url` must be specified, but not both. This is useful for Kubernetes deployments where the binary is stored in object storage.

- **`args`** (optional): A list of strings representing command-line arguments to pass to the WebAssembly binary. Defaults to an empty list if not specified.

- **`preopened_dir`** (optional): A directory path that will be preopened for the WebAssembly binary, allowing file system access. If a relative path is specified, it is resolved relative to the directory where the `kafu-config.yaml` file is located.

### Node Configuration

The first node in the `nodes` map is responsible for starting the execution of the WebAssembly binary. Each node in the `nodes` map must have the following fields:

- **`address`** (required): A string representing the IP address or hostname of the node. This must be non-empty. For Kubernetes deployments, you can use service names (e.g., `kafu-server-edge.default.svc.cluster.local`).

- **`port`** (required): An integer (u16) representing the port number on which the node listens for Kafu runtime communication.

- **`placement`** (optional): A string representing a logical placement group for this node when integrating with orchestrators such as Kubernetes. The core Kafu runtime does not use this field directly, but tools like `kafu kustomize` map it to platform-specific concepts (e.g., Kubernetes node labels). When omitted, such tools should fall back to using the node ID as the placement key, preserving the existing 1:1 behavior between node ID and physical node.

### Cluster Configuration

The optional `cluster` section controls cluster-level behavior.

#### Heartbeat Configuration

`cluster.heartbeat` controls heartbeat (health-check) based liveness management:

- **`follower_on_coordinator_lost`** (optional, default: `shutdown_self`): What non-coordinator nodes should do when the coordinator becomes unreachable for long enough.
  - `shutdown_self`: Shut down this node.
  - `ignore`: Do nothing.

- **`interval_ms`** (optional, default: `1000`): Heartbeat interval in milliseconds (used for both peer monitoring and coordinator monitoring).

#### Migration Configuration

`cluster.migration` controls migration-related options:

- **`memory_compression`** (optional, default: `true`): Compress main memory with LZ4 when sending, reducing transfer size. This applies to both the delta path (compress changed pages) and the full snapshot path (compress full main memory blob).

- **`memory_migration`** (optional, default: `delta`): Memory migration strategy.
  - `delta`: Send only changed 64KB pages when the receiver has the baseline; otherwise fall back to full.
  - `full`: Always send full main memory (no delta).

## Example Configuration

### Local Development

```yaml
# Name of the Kafu service.
name: kafu-basic-example
app:
  # Path to the Wasm binary to run.
  path: ./main.wasm
  args: [960, 540]
  preopened_dir: .
# Node list in the Kafu cluster.
# The first node (cloud1) starts the execution of the WebAssembly binary.
nodes:
  cloud1: # Node ID (this node starts the Wasm execution)
    # IP address and port of the node.
    address: 127.0.0.1
    # Port of the node.
    port: 50051
  edge1:
    address: 127.0.0.1
    port: 50052

# (Optional) Cluster behavior.
cluster:
  heartbeat:
    # Behavior for non-coordinator nodes when coordinator is lost.
    follower_on_coordinator_lost: shutdown_self   # ignore | shutdown_self
    # Heartbeat interval (ms).
    interval_ms: 1000
  migration:
    # Memory migration strategy: delta | full
    memory_migration: delta
    # Compress memory when sending.
    memory_compression: true
```
