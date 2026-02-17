CC = $(KAFU_SDK_PATH)/libexec/kafu_clang

all: main.wasm

main.wasm: main.cpp
	$(CC) $< -o $@ -lstdc++ -O3

clean:
	rm -f *.wasm

.PHONY: all clean
