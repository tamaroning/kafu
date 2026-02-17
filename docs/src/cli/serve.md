# Kafu Serve

The `kafu serve` command runs a Kafu node in a distributed cluster.

## Usage

```sh
kafu serve --node-id <NODE_ID> <CONFIG_PATH>
```

## Arguments

- `--node-id <NODE_ID>` (required): The ID of the node to run. This must match one of the node IDs defined in the configuration file.

- `<CONFIG_PATH>` (required): Path to the Kafu configuration file.

## Behavior

The node’s behavior depends on whether `<NODE_ID>` is the first entry in the config’s `nodes` section:

- **First node**: starts execution of the configured WebAssembly program.
- **Other nodes**: wait for incoming migration requests and resume execution when a migration arrives.

## Cluster liveness (heartbeat)

By default, `kafu serve` can perform periodic liveness monitoring between nodes and may shut down nodes depending on the configured policy. You can disable or tune this behavior via `cluster.heartbeat` in `kafu-config.yaml` (see [Kafu Config](../kafu-config.md)).

- **Peer monitoring (first node, optional)**:
  - When enabled, the first node periodically checks whether peers are reachable.
  - If a peer remains unreachable long enough, the first node requests a cluster-wide shutdown (best effort).

- **Coordinator loss handling (non-first nodes, optional)**:
  - When enabled, a non-first node monitors the coordinator (currently: the first node).
  - Monitoring starts after the coordinator begins program execution.
  - If the coordinator remains unreachable long enough, the node shuts itself down.
  - Controlled by `cluster.heartbeat.follower_on_coordinator_lost`.

## Configuration

The server requires a valid Kafu configuration file. For details, see [Kafu Config](../kafu-config.md).

## Examples

```sh
# Start all nodes with the same command
# The first node (cloud1) will automatically start execution
# Other nodes (edge1, edge2) will automatically wait for migration
kafu serve --node-id edge1 kafu-config.yaml
kafu serve --node-id edge2 kafu-config.yaml
kafu serve --node-id cloud1 kafu-config.yaml
```

## Environment

- `RUST_LOG` (optional): Controls logging level (e.g., `info`, `debug`, `warn`, `error`).

## See also

- [Kafu Config](../kafu-config.md)
