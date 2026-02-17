# Build from Source

## Prerequisites

- Rust

```sh
sudo apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev openssl protobuf-compiler
```

## Build

Build all crates:

```sh
cargo build --workspace
```
