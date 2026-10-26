WASI_SDK ?= $(if $(WASI_SDK_PATH),$(WASI_SDK_PATH),$(firstword $(wildcard /opt/wasi-sdk $(HOME)/.local/wasi-sdk $(USERPROFILE)/.local/wasi-sdk)))
ADAPTER ?= $(if $(WASI_PREVIEW1_ADAPTER),$(WASI_PREVIEW1_ADAPTER),$(firstword $(wildcard $(HOME)/.cargo/registry/src/*/wasi-preview1-component-adapter-provider-*/artefacts/wasi_snapshot_preview1.reactor.wasm $(USERPROFILE)/.cargo/registry/src/*/wasi-preview1-component-adapter-provider-*/artefacts/wasi_snapshot_preview1.reactor.wasm)))
ifeq ($(OS),Windows_NT)
EXE := .exe
else
EXE :=
endif
CXX = $(WASI_SDK)/bin/clang++$(EXE)
CXXFLAGS = --target=wasm32-wasip1 --sysroot=$(WASI_SDK)/share/wasi-sysroot -std=c++20 -fno-exceptions -O2
PLUGIN = metrics-cpp
NAME = metrics

ifeq ($(strip $(WASI_SDK)),)
$(error Set WASI_SDK_PATH to a wasi-sdk install)
endif
ifeq ($(strip $(ADAPTER)),)
$(error Set WASI_PREVIEW1_ADAPTER to wasi_snapshot_preview1.reactor.wasm)
endif

.PHONY: all clean

all:
	wit-bindgen cpp wit/$(NAME).wit --out-dir gen
	$(CXX) $(CXXFLAGS) -Igen -c gen/$(NAME).cpp -o gen/$(NAME).o
	$(CXX) $(CXXFLAGS) -Igen -c src/impl.cpp -o gen/impl.o
	$(CXX) $(CXXFLAGS) gen/$(NAME)_component_type.o gen/$(NAME).o gen/impl.o -o $(NAME).core.wasm -mexec-model=reactor
	wasm-tools component new $(NAME).core.wasm --adapt wasi_snapshot_preview1=$(ADAPTER) -o $(PLUGIN).component.wasm

clean:
	rm -rf gen $(NAME).core.wasm $(PLUGIN).component.wasm
