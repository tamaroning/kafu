# Snapify

## Overview

Snapify is a [Binaryen](https://github.com/tamaroning/binaryen/tree/snapify) (a.k.a. wasm-opt) Pass that enables checkpoint/restore functionality for Wasm modules. When used in combination with Asyncify, it allows saving and restoring the state of a running Wasm program.

You can find the source code at <https://github.com/tamaroning/binaryen/blob/snapify/src/passes/Snapify.cpp>.

Snapify is installed as `$KAFU_SDK_PATH/libexec/kafu_wasm-opt`.

## Usage

Apply the Snapify pass to a Wasm module, then apply Asyncify:

```bash
$(WASM_OPT) foo.wasm -O1 --enable-multimemory --snapify \
  --pass-arg=policy@always -o foo2.wasm
$(WASM_OPT) foo2.wasm -O1 --asyncify \
  --pass-arg=asyncify-memory@snapify_memory \
  --enable-multimemory -o output.wasm
```

## How It Works

Snapify Pass inserts migration points at both the beginning and the end of functions. These migration points check whether a checkpoint is needed and coordinate with Asyncify to unwind/rewind the stack.
