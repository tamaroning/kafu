# Kafu Singlenode

The `kafu singlenode` command runs a Kafu service on a single node by emulating a multi-node environment. This is useful for local development and testing without setting up a distributed cluster.

## Usage

```sh
kafu singlenode serve <CONFIG_PATH>
```

## Commands

### `serve`

Starts the Kafu service on a single node using the first node defined in the configuration file.

```sh
kafu singlenode serve kafu-config.yaml
```

## Arguments

- `<CONFIG_PATH>` (required): Path to the Kafu configuration file.

## Examples

```sh
# Run a Kafu service in singlenode mode
kafu singlenode serve kafu-config.yaml
```

## Configuration

The singlenode command uses the same configuration format as `kafu serve`.

For details about the configuration format, see [Kafu Config](../kafu-config.md).

## Environment

- `RUST_LOG` (optional): Controls logging level (e.g., `info`, `debug`, `warn`, `error`).

## See also

- [Kafu Config](../kafu-config.md)
