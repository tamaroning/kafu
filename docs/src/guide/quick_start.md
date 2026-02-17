# Quick Start

## Installation

To install Kafu SDK on your system, please follow the instructions in [Installation](./installation.md).

## Build a Simple Distributed Service

Create `main.c` with the following content:

```c
#include <stdio.h>
#include "kafu.h"

void f();

int main() {
  // Starts the program on the cloud node.
  printf("Hello, from cloud!\n");
  fflush(stdout);
}

KAFU_DEST(f, "edge")
KAFU_EXPORT(f)
void f() {
  printf("Hello, from edge!\n");
  fflush(stdout);
}
```

This service consists of two nodes: `cloud` and `edge`.
When the `cloud` node executes `main` first, execution switches to the `edge` node when calling `f()`.

Compile it to a Wasm module using `kafu clang`:

```sh
kafu clang main.c -o main.wasm
```

Create `kafu-config.yaml` in the same directory as `main.wasm` with the following content:

```yaml
# Name of the Kafu service.
name: kafu-basic-example
app:
  # Path to the Wasm binary to run.
  path: ./main.wasm
  args: []
  preopened_dir: .
# Node list in the Kafu cluster.
nodes:
  cloud: # Node ID
    # IP address and port of the node.
    address: 127.0.0.1
    # Port of the node.
    port: 50051
  edge:
    address: 127.0.0.1
    port: 50052
```

## Run the Service on a Single Node

You can run Kafu programs on a single node using `kafu_singlenode`.

```sh
$ kafu singlenode serve kafu-config.yaml 
2026-02-17T08:03:53.896071Z  INFO Program is starting on cloud1
Hello, from cloud
2026-02-17T08:03:53.926603Z  INFO Migration cloud1 -> edge1 (Entering g)
Hello, from edge
2026-02-17T08:03:53.926623Z  INFO Migration edge1 -> cloud1 (Returning from g)
Hello, from cloud!
2026-02-17T08:03:53.926638Z  INFO Program finished on cloud1
```


If you want to deploy the service on multiple nodes using Kubernetes, see [Kubernetes Integration](./kubernetes_integration.md).


