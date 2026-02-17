CC = $(KAFU_SDK_PATH)/libexec/kafu_clang

all: main.wasm

main.wasm: main.c
	$(CC) $< -o $@ -O3

clean:
	rm -f *.wasm

.PHONY: all clean
