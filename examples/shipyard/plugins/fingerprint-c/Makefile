WASI_SDK ?= $(if $(WASI_SDK_PATH),$(WASI_SDK_PATH),$(firstword $(wildcard /opt/wasi-sdk $(HOME)/.local/wasi-sdk $(USERPROFILE)/.local/wasi-sdk)))
ADAPTER ?= $(if $(WASI_PREVIEW1_ADAPTER),$(WASI_PREVIEW1_ADAPTER),$(firstword $(wildcard $(HOME)/.cargo/registry/src/*/wasi-preview1-component-adapter-provider-*/artefacts/wasi_snapshot_preview1.reactor.wasm $(USERPROFILE)/.cargo/registry/src/*/wasi-preview1-component-adapter-provider-*/artefacts/wasi_snapshot_preview1.reactor.wasm)))
ifeq ($(OS),Windows_NT)
EXE := .exe
else
EXE :=
endif
CC = $(WASI_SDK)/bin/clang$(EXE)
CFLAGS = --target=wasm32-wasip1 --sysroot=$(WASI_SDK)/share/wasi-sysroot -O2
PLUGIN = fingerprint-c
NAME = fingerprint
WIT_DIR ?= wit
OUT ?= $(PLUGIN).component.wasm

ifeq ($(strip $(WASI_SDK)),)
$(error Set WASI_SDK_PATH to a wasi-sdk install)
endif
ifeq ($(strip $(ADAPTER)),)
$(error Set WASI_PREVIEW1_ADAPTER to wasi_snapshot_preview1.reactor.wasm)
endif

.PHONY: all clean bindings

all:
	wit-bindgen c $(WIT_DIR) --out-dir gen
	$(CC) $(CFLAGS) -Igen -c gen/$(NAME).c -o gen/$(NAME).o
	$(CC) $(CFLAGS) -Igen -c src/impl.c -o gen/impl.o
	$(CC) $(CFLAGS) gen/$(NAME)_component_type.o gen/$(NAME).o gen/impl.o -o $(NAME).core.wasm -mexec-model=reactor
	wasm-tools component new $(NAME).core.wasm --adapt wasi_snapshot_preview1=$(ADAPTER) -o $(OUT)

clean:
	rm -rf gen $(NAME).core.wasm $(OUT)
