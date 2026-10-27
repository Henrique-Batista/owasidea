WASI_SDK ?= $(WASI_SDK_PATH)
ADAPTER ?= $(WASI_PREVIEW1_ADAPTER)
ifeq ($(OS),Windows_NT)
EXE := .exe
else
EXE :=
endif
ifndef WASI_SDK
$(error Set WASI_SDK or WASI_SDK_PATH)
endif
ifndef ADAPTER
$(error Set WASI_PREVIEW1_ADAPTER to the reactor adapter wasm)
endif
CC = $(WASI_SDK)/bin/{{compiler}}{{EXE}}

all:
\twit-bindgen {{lang}} wit/{{name}}.wit --out-dir src
\t$(CC) --target=wasm32-wasip1 {{stdflag}} -O2 -c {{source}} -o src/{{name}}.o
\t$(CC) --target=wasm32-wasip1 {{stdflag}} -O2 -c src/impl.{{ext}} -o src/impl.o
\t$(CC) --target=wasm32-wasip1 {{stdflag}} -O2 src/{{name}}_component_type.o src/{{name}}.o src/impl.o -o {{name}}.wasm -mexec-model=reactor
\twasm-tools component new {{name}}.wasm --adapt $(ADAPTER) -o {{name}}.component.wasm
