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

<<<<<<< HEAD
.PHONY: deps build-ebpf build-user build run check clean fmt verify verify-btf
=======
.PHONY: deps build-ebpf build-user build-api build run run-api compose-up check clean fmt verify verify-btf enterprise-check

# Skill-driven build gate (see .cursor/skills/finops-ebpf-agent/SKILL.md)
enterprise-check: build check
	@echo "==> Enterprise gate OK (build + check). Update docs/adr/skills if you changed architecture."

FINOPS_INGEST_URL ?= http://localhost:3000/ingest
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

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
<<<<<<< HEAD
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
=======
	@echo "==> [2/3] Compiling user-space agent..."
	cd $(WORKSPACE_ROOT) && cargo build -p finops-user --release
	@echo "==> Agent build complete."

build-api:
	@echo "==> Compiling finops-api (ingest)..."
	cd $(WORKSPACE_ROOT) && cargo build -p finops-api --release
	@echo "==> API build complete."

# ──────────────────────────────────────────────────────────────
build: build-ebpf build-user build-api
	@echo ""
	@echo "Build complete."
	@echo "  eBPF bytecode : $(EBPF_BIN)"
	@echo "  Agent binary  : $(WORKSPACE_ROOT)/target/release/finops-user"
	@echo "  API binary    : $(WORKSPACE_ROOT)/target/release/finops-api"
	@echo ""
	@echo "Phase 2: make run"
	@echo "Phase 3: make compose-up && make run-api  (terminal) && FINOPS_INGEST_URL=$(FINOPS_INGEST_URL) sudo -E make run"
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

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

<<<<<<< HEAD
=======
run-api: build-api
	@echo "==> Starting finops-api (KAFKA_BROKERS=localhost:9092)..."
	RUST_LOG=info KAFKA_BROKERS=localhost:9092 \
		$(WORKSPACE_ROOT)/target/release/finops-api

compose-up:
	@command -v docker >/dev/null 2>&1 || ( \
		echo "ERROR: docker not found. Install on Ubuntu:"; \
		echo "  sudo apt-get update && sudo apt-get install -y docker.io docker-compose-v2"; \
		echo "  sudo systemctl start docker"; \
		exit 127; \
	)
	@echo "==> Starting Kafka + Kafka UI + ClickHouse (finops-net)..."
	docker compose -f $(WORKSPACE_ROOT)/docker-compose.yml up -d

>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
# ──────────────────────────────────────────────────────────────
# Quick syntax check (much faster than full build)
check:
	cd $(WORKSPACE_ROOT) && cargo check -p finops-common
	cd $(EBPF_DIR) && cargo +nightly check \
		-Z build-std=core --target $(EBPF_TARGET)
	cd $(WORKSPACE_ROOT) && cargo check -p finops-user
<<<<<<< HEAD
=======
	cd $(WORKSPACE_ROOT) && cargo check -p finops-api
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

# ──────────────────────────────────────────────────────────────
# Show the BPF bytecode disassembly — what the verifier sees.
# Educational: you can see how your Rust code maps to BPF instructions.
verify: build-ebpf
	@echo "==> BPF program sections:"
	llvm-objdump -d $(EBPF_BIN) | head -80
	@echo ""
	@echo "==> BPF map definitions:"
	llvm-readelf -S $(EBPF_BIN) 2>/dev/null | grep -E "Name|maps|kprobe|tracepoint" || true

verify-btf: build-ebpf
	@test -r /sys/kernel/btf/vmlinux && echo "BTF: /sys/kernel/btf/vmlinux OK" \
		|| (echo "ERROR: BTF not available — CO-RE / portable loads may fail" && exit 1)
	@if [ -z "$(EBPF_BIN)" ]; then \
		echo "ERROR: eBPF binary not found (run make build-ebpf)"; exit 1; \
	fi
	@if llvm-readelf -S "$(EBPF_BIN)" 2>/dev/null | grep -qE '\.BTF'; then \
		echo "BTF: .BTF section present in $(EBPF_BIN)"; \
		bpftool btf dump file "$(EBPF_BIN)" | head -20; \
	else \
		echo "BTF: no .BTF in $(EBPF_BIN) (non-CO-RE build — OK for fixed-kernel deploys)"; \
	fi

# ──────────────────────────────────────────────────────────────
clean:
	cd $(EBPF_DIR) && cargo clean
	cd $(WORKSPACE_ROOT) && cargo clean

fmt:
	cd $(WORKSPACE_ROOT) && cargo fmt --all
	cd $(EBPF_DIR) && cargo +nightly fmt
