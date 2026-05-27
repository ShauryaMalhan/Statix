# ============================================================
# FinOps eBPF Agent — Build & Run
# ============================================================
#
# Two distinct build steps:
#   1. finops-ebpf  : compiled with nightly Rust → BPF bytecode (ELF)
#                     target: bpfel-unknown-none (little-endian BPF, no OS)
#   2. finops-user  : compiled with stable Rust  → native x86_64 binary
#                     target: x86_64-unknown-linux-gnu (your server)
#
# These two steps CANNOT share a single `cargo build` because they target
# completely different CPU instruction sets.

WORKSPACE_ROOT := $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
EBPF_DIR       := $(WORKSPACE_ROOT)/finops-ebpf
USER_DIR       := $(WORKSPACE_ROOT)/finops-user

# Ensure cargo and rustup are always on PATH, even when make is invoked
# from a shell that hasn't sourced ~/.cargo/env (e.g. /bin/sh, cron, CI).
export PATH := $(HOME)/.cargo/bin:$(PATH)

# Where cargo puts the compiled BPF ELF.
# CARGO_TARGET_DIR may be overridden by the environment (e.g. CI, sandbox).
# We let cargo decide; then find the output with `find`.
EBPF_OUT_NAME  := finops-ebpf
EBPF_TARGET    := bpfel-unknown-none

.PHONY: deps build-ebpf build-user build run check clean fmt verify

# ──────────────────────────────────────────────────────────────
deps:
	@echo "==> Checking toolchain..."
	@rustc --version || (echo "Install Rust: https://rustup.rs" && exit 1)
	@rustup toolchain list | grep -q nightly || rustup toolchain install nightly
	@rustup component list --toolchain nightly | grep -q "rust-src (installed)" \
		|| rustup component add rust-src --toolchain nightly
	@which bpf-linker || cargo install bpf-linker
	@clang --version > /dev/null || (echo "Install: apt install clang" && exit 1)
	@echo "==> All dependencies present"

# ──────────────────────────────────────────────────────────────
# Compile finops-ebpf to BPF bytecode.
#
# Flags:
#   +nightly          → requires nightly (for -Z build-std unstable feature)
#   --release         → optimize bytecode (smaller = fewer verifier steps)
#   -Z build-std=core → compile core from source for bpfel-unknown-none
#                       (no prebuilt artifacts exist for this tier-3 target)
#   --target ...      → output BPF ELF, not x86_64 ELF
build-ebpf:
	@echo "==> [1/2] Compiling eBPF kernel program..."
	cd $(EBPF_DIR) && cargo +nightly build --release \
		-Z build-std=core \
		--target $(EBPF_TARGET)
	@echo "==> eBPF build complete."

# Resolve where cargo put the BPF ELF.
#
# IMPORTANT: use = (lazy/recursive), NOT := (immediate).
# := evaluates once at Makefile parse time — before build-ebpf has run,
# so find returns nothing and the variable stays empty forever.
# = re-evaluates every time the variable is referenced, which means the
# find runs AFTER build-ebpf has produced the binary.
EBPF_BIN = $(shell \
	find "$(EBPF_DIR)/target" \
		-path "*/$(EBPF_TARGET)/release/$(EBPF_OUT_NAME)" \
		-not -name "*.d" \
		-type f 2>/dev/null | head -1)

# ──────────────────────────────────────────────────────────────
# Compile finops-user (the Rust daemon) with stable toolchain.
build-user:
	@echo "==> [2/2] Compiling user-space agent..."
	cd $(WORKSPACE_ROOT) && cargo build -p finops-user --release
	@echo "==> Agent build complete."

# ──────────────────────────────────────────────────────────────
build: build-ebpf build-user
	@echo ""
	@echo "Build complete."
	@echo "  eBPF bytecode : $(EBPF_BIN)"
	@echo "  Agent binary  : $(USER_DIR)/target/release/finops-user"
	@echo ""
	@echo "Run with: make run"

# ──────────────────────────────────────────────────────────────
# Run the agent.
# Requires root / CAP_BPF + CAP_PERFMON.
#
# FINOPS_EBF_PATH is picked up by main.rs at startup.
# The agent reads the eBPF ELF from this path and loads it into the kernel.
run: build
	@if [ -z "$(EBPF_BIN)" ]; then \
		echo "ERROR: Could not find compiled eBPF binary."; \
		echo "Run 'make build-ebpf' first."; \
		exit 1; \
	fi
	@echo "==> Starting agent (Ctrl+C to stop)..."
	@echo "==> eBPF program: $(EBPF_BIN)"
	RUST_LOG=info FINOPS_EBF_PATH=$(EBPF_BIN) \
		$(WORKSPACE_ROOT)/target/release/finops-user

# ──────────────────────────────────────────────────────────────
# Quick syntax check (much faster than full build)
check:
	cd $(WORKSPACE_ROOT) && cargo check -p finops-common
	cd $(EBPF_DIR) && cargo +nightly check \
		-Z build-std=core --target $(EBPF_TARGET)
	cd $(WORKSPACE_ROOT) && cargo check -p finops-user

# ──────────────────────────────────────────────────────────────
# Show the BPF bytecode disassembly — what the verifier sees.
# Educational: you can see how your Rust code maps to BPF instructions.
verify: build-ebpf
	@echo "==> BPF program sections:"
	llvm-objdump -d $(EBPF_BIN) | head -80
	@echo ""
	@echo "==> BPF map definitions:"
	llvm-readelf -S $(EBPF_BIN) 2>/dev/null | grep -E "Name|maps|kprobe" || true

# ──────────────────────────────────────────────────────────────
clean:
	cd $(EBPF_DIR) && cargo clean
	cd $(WORKSPACE_ROOT) && cargo clean

fmt:
	cd $(WORKSPACE_ROOT) && cargo fmt --all
	cd $(EBPF_DIR) && cargo +nightly fmt
