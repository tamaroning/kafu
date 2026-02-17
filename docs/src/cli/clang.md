# Kafu Clang

Kafu Clang is a C/C++ compiler that generates WebAssembly modules.

## Usage

```sh
kafu clang [clang args...]
```

The binary is located at `$KAFU_SDK_PATH/libexec/kafu_clang`.

## Arguments

- `clang args...` (optional): Arguments passed through to the underlying C/C++ compiler.

## Examples

```sh
# Build a single C file into a WebAssembly module (output: a.out by default)
kafu clang main.c

# Specify output name
kafu clang main.c -o main.wasm

# Add include directories
kafu clang main.c -I ./include -o main.wasm
```

## See also

- [Kafu SDK headers](../../library/kafu-h.md)
