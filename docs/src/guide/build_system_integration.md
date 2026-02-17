# Integration into Build Systems

Kafu supports building applications with any build system, such as CMake.

All you need to do is set `$KAFU_SDK_PATH/libexec/kafu_clang` as the C/C++ compiler that your build system uses.

## Example: Makefile

You can compile a program consisting of `main.c` as follows:

```makefile
CC = $(KAFU_SDK_PATH)/libexec/kafu_clang

all: main.wasm

main.wasm: main.c
	$(CC) $< -o $@

clean:
	rm -f *.wasm

.PHONY: all clean
```
