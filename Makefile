# Multisig Program — Quick Commands
#
# Prerequisites:
#   - Rust + risc0 toolchain installed
#   - wallet CLI installed (`cargo install --path wallet` from logos-execution-zone repo)
#   - Sequencer running locally
#   - wallet setup done (`wallet setup`)
#
# Quick start:
#   make build deploy
#   multisig create --threshold 2 --member <ID1> --member <ID2> --member <ID3>
#
# State is saved in .multisig-state so you don't have to re-enter IDs.

SHELL := /bin/bash
STATE_FILE := .multisig-state
PROGRAMS_DIR := target/riscv32im-risc0-zkvm-elf/docker

# Token program binary — set this to point to your logos-execution-zone build
# e.g. LSSA_DIR=../logos-execution-zone
LSSA_DIR ?=
TOKEN_BIN := $(LSSA_DIR)/artifacts/program_methods/token.bin

MULTISIG_BIN := $(PROGRAMS_DIR)/multisig.bin

# ── Helpers ──────────────────────────────────────────────────────────────────

-include $(STATE_FILE)

define save_var
	@grep -v '^$(1)=' $(STATE_FILE) 2>/dev/null > $(STATE_FILE).tmp || true
	@echo '$(1)=$(2)' >> $(STATE_FILE).tmp
	@mv $(STATE_FILE).tmp $(STATE_FILE)
endef

define require_state
	@if [ -z "$($(1))" ]; then echo "ERROR: $(1) not set. Run the required step first or set it manually."; exit 1; fi
endef

# ── Targets ──────────────────────────────────────────────────────────────────

# ── Code Generation ───────────────────────────────────────────────────────────
# The IDL and FFI client are generated files — do not edit them manually.
# Source of truth: multisig_program/src/lib.rs (Rust macro annotations)
# Pipeline: lib.rs → multisig_idl.json → multisig.rs

SPEL_FW_GIT  := https://github.com/logos-co/spel.git
SPEL_FW_TAG  := v0.2.0-rc.5
IDL_JSON    := lez-multisig-ffi/src/multisig_idl.json
FFI_RS      := lez-multisig-ffi/src/multisig.rs
HEADER_H    := lez-multisig-ffi/include/lez_multisig.h
GENERATE_IDL_BIN := methods/guest/Cargo.toml

.PHONY: generate generate-idl generate-ffi generate-header check-generated install-tools

install-tools: ## Install spel-client-gen + cbindgen (required for generate/generate-header)
	source ~/.cargo/env && cargo install --git $(SPEL_FW_GIT) --tag $(SPEL_TAG) spel-client-gen --locked 2>/dev/null || \
		cargo install --git $(SPEL_FW_GIT) --tag $(SPEL_TAG) spel-client-gen
	source ~/.cargo/env && cargo install cbindgen --locked 2>/dev/null || true

generate-idl: ## Regenerate IDL from Rust annotations in lib.rs
	@echo "🔨 Generating IDL from multisig_program/src/lib.rs..."
	source ~/.cargo/env && cargo run -p lez-multisig-idl-gen > $(IDL_JSON)
	@echo "✅ IDL written to $(IDL_JSON)"

generate-ffi: ## Regenerate FFI client (multisig.rs) from IDL
	@echo "🔨 Generating FFI client from $(IDL_JSON)..."
	@mkdir -p /tmp/lez-ffi-gen
	source ~/.cargo/env && spel-client-gen --idl $(IDL_JSON) --out-dir /tmp/lez-ffi-gen || \
		(echo "ERROR: spel-client-gen not found. Run: make install-tools" && exit 1)
	@# Prepend generated-file header, then append spel-client-gen output
	@echo "// GENERATED FILE — do not edit manually. Run 'make generate' to regenerate from Rust annotations." > $(FFI_RS)
	@cat /tmp/lez-ffi-gen/multisig_program_ffi.rs >> $(FFI_RS)
	@# Fix type inference issues in generated code (spel-client-gen omits some annotations)
	@sed -i 's/let create_key = serde_json::from_value/let create_key: [u8; 32] = serde_json::from_value/g' $(FFI_RS)
	@echo "✅ FFI client written to $(FFI_RS)"

generate: ## Regenerate IDL, FFI client, and C header from Rust annotations (run after changing lib.rs)
	@echo "🔄 Regenerating all generated files..."
	$(MAKE) generate-idl
	$(MAKE) generate-ffi
	$(MAKE) generate-header
	@echo ""
	@echo "✅ Generation complete. Run 'cargo check' to verify."

generate-header: ## Generate C header from Rust FFI via cbindgen
	@echo "🔨 Generating C header from lez-multisig-ffi..."
	@mkdir -p lez-multisig-ffi/include
	cd lez-multisig-ffi && source ~/.cargo/env && cbindgen --config cbindgen.toml --output ../include/lez_multisig.h || \
		(echo "ERROR: cbindgen not found. Install with: cargo install cbindgen" && exit 1)
	@echo "✅ C header written to $(HEADER_H)"

check-generated: ## CI: regenerate and check for drift vs committed state
	@echo "🔍 Checking for generated file drift..."
	@$(MAKE) generate > /tmp/generate-output.txt 2>&1 || (cat /tmp/generate-output.txt && exit 1)
	@# Only the C header is tracked in git; IDL and FFI client are regenerated on every CI run
	@git diff --quiet HEAD -- $(HEADER_H) || \
		(echo "⚠️ Generated header differs from committed state. Run 'make generate-header' to update." && exit 1)
	@echo "✅ No drift detected"


.PHONY: help build build-cli deploy status clean test

help: ## Show this help
	@echo "Multisig Program — Make Targets"
	@echo ""
	@echo "  Code Generation (start here after changing lib.rs):"
	@echo "  make install-tools         Install spel-client-gen tool (first-time setup)"
	@echo "  make generate              Regen IDL + FFI client from lib.rs annotations"
	@echo "  make generate-idl          Regen IDL only"
	@echo "  make generate-ffi          Regen FFI client only (requires IDL)"
	@echo "  make check-generated       CI: regenerate and verify no drift"
	@echo ""
	@echo "  Build & Deploy:"
	@echo "  make build                 Build the guest binary (needs risc0 toolchain)"
	@echo "  make build-cli             Build the standalone multisig CLI"
	@echo "  make deploy                Deploy multisig + token programs to sequencer"
	@echo "  make test                  Run unit tests"
	@echo "  make status                Show saved state (account IDs, etc.)"
	@echo "  make clean                 Remove saved state"
	@echo ""
	@echo "Required env: LSSA_DIR=<path to logos-execution-zone repo>"

build: ## Build the multisig guest binary
	cargo risczero build --manifest-path methods/guest/Cargo.toml
	@echo ""
	@echo "✅ Guest binary built: $(MULTISIG_BIN)"
	@ls -la $(MULTISIG_BIN)

build-cli: ## Build the standalone multisig CLI
	cargo build --bin multisig -p multisig-cli
	@echo ""
	@echo "✅ CLI built: target/debug/multisig"

deploy: ## Deploy multisig and token programs to sequencer
	@test -f "$(MULTISIG_BIN)" || (echo "ERROR: Multisig binary not found. Run 'make build' first."; exit 1)
	@test -f "$(TOKEN_BIN)" || (echo "ERROR: Token binary not found at $(TOKEN_BIN). Set LSSA_DIR correctly."; exit 1)
	wallet deploy-program $(MULTISIG_BIN)
	wallet deploy-program $(TOKEN_BIN)
	@echo ""
	@echo "✅ Programs deployed"

test: ## Run unit tests
	cargo test -p multisig_program

status: ## Show saved state
	@echo "Multisig State (from $(STATE_FILE)):"
	@echo "──────────────────────────────────────"
	@if [ -f "$(STATE_FILE)" ]; then cat $(STATE_FILE); else echo "(no state saved)"; fi
	@echo ""
	@echo "Binaries:"
	@ls -la $(MULTISIG_BIN) 2>/dev/null || echo "  multisig.bin: NOT BUILT (run 'make build')"
	@ls -la $(TOKEN_BIN) 2>/dev/null || echo "  token.bin: NOT FOUND (check LSSA_DIR)"

clean: ## Remove saved state
	rm -f $(STATE_FILE) $(STATE_FILE).tmp
	@echo "✅ State cleaned"

# ── E2E Tests ─────────────────────────────────────────────────────────────────

.PHONY: test-e2e

test-e2e: ## Run full E2E tests (requires sequencer running + lssa artifacts)
	@test -n "$(LSSA_DIR)" || (echo "ERROR: Set LSSA_DIR=<path to logos-execution-zone repo>"; exit 1)
	@echo "🧪 Running E2E tests..."
	RISC0_SKIP_BUILD=1 SEQUENCER_URL=http://127.0.0.1:3040 	  MULTISIG_PROGRAM=$(PROGRAMS_DIR)/multisig.bin 	  TOKEN_PROGRAM=$(TOKEN_BIN) 	  cargo test -p lez-multisig-e2e --test e2e_multisig -- --nocapture
	@echo "✅ E2E tests passed"

# ── FFI .so Build ─────────────────────────────────────────────────────────────

.PHONY: build-ffi

build-ffi: generate ## Build the FFI .so (liblez_multisig_ffi.so) for use in Qt module
	@echo "🔨 Building FFI shared library..."
	source ~/.cargo/env && RISC0_SKIP_BUILD=1 cargo build --release -p lez-multisig-ffi
	@echo "✅ FFI .so built: target/release/liblez_multisig_ffi.so"
	@ls -lh target/release/liblez_multisig_ffi.so

# ── Headless Demo ─────────────────────────────────────────────────────────────

.PHONY: demo

LOGOSCORE ?= $(HOME)/logoscore-test/logoscore
MULTISIG_MODULE_DIR ?= $(HOME)/logos-lez-multisig-module/result/lib
REGISTRY_MODULE_DIR ?= $(HOME)/logos-lez-registry-module/build

demo: ## Run headless Logos Core demo (loads both modules via logoscore)
	@test -f "$(LOGOSCORE)" || (echo "ERROR: logoscore not found at $(LOGOSCORE)"; exit 1)
	@echo "🚀 Loading multisig module via Logos Core..."
	timeout 8 $(LOGOSCORE) --modules-dir $(MULTISIG_MODULE_DIR) 	  --load-modules lez_multisig_module 	  --call "lez_multisig_module.loadMultisigs()" || true
	@echo ""
	@echo "📋 Loading registry module via Logos Core..."
	timeout 8 $(LOGOSCORE) --modules-dir $(REGISTRY_MODULE_DIR) 	  --load-modules liblez_registry_module 	  --call "liblez_registry_module.listPrograms()" || true
