# Installation

To build WebAssembly services, you need to install Kafu SDK by running the following command:

**Supported platforms:** Linux on x86-64 and aarch64 only.

```sh
curl -fsSL "https://raw.githubusercontent.com/tamaroning/kafu/refs/heads/main/install.sh" | sh
```

## Check Installation

Check if Kafu SDK is installed properly.

By default, the installer appends environment variable exports to your shell profile (for example, `~/.bashrc` or `~/.zshrc`). Open a new shell session (or source your profile) so that `KAFU_SDK_PATH`, `WASI_SDK_PATH`, and `PATH` updates take effect.

If you installed with `--no-modify-path`, set `KAFU_SDK_PATH`, `WASI_SDK_PATH`, and update `PATH` manually.

```sh
$ kafu --version
Kafu CLI 0.1.0
```
