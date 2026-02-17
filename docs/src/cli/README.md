# Kafu Command Line Tools (Kafu CLI)

Kafu CLI is a set of tools for building, deploying, and running WebAssembly modules with Kafu.

## Usage

```sh
kafu <subcommand> [args...]
```

When you run `kafu <subcommand>`, the CLI looks up `$KAFU_SDK_PATH/libexec/kafu_<subcommand>` and executes it, forwarding all remaining arguments.

## Subcommands

- [Kafu Clang (`clang`)](./clang.md): C/C++ compiler wrapper that produces WebAssembly modules
- [Kafu Serve (`serve`)](./serve.md): Run a Kafu node (gRPC server + WebAssembly runtime)
- [Kafu Kustomize (`kustomize`)](./kustomize.md): Generate Kubernetes manifests from a Kafu config
- [Kafu Singlenode (`singlenode`)](./singlenode.md): Run a Kafu service locally in a single-node mode
