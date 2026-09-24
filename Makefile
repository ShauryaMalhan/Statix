# ============================================================
# FinOps eBPF Agent — Build & Run
# ============================================================

WORKSPACE_ROOT := $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
EBPF_DIR       := $(WORKSPACE_ROOT)/statix-ebpf
AGENT_DIR      := $(WORKSPACE_ROOT)/statix

export PATH := $(HOME)/.cargo/bin:$(PATH)

EBPF_OUT_NAME  := statix-ebpf
EBPF_TARGET    := bpfel-unknown-none
BPF_BUNDLE_DIR := $(WORKSPACE_ROOT)/target/bpf
EBPF_RELEASE   := $(EBPF_DIR)/target/$(EBPF_TARGET)/release/$(EBPF_OUT_NAME)

.PHONY: deps deps-check build-ebpf build-agent build-user build-gateway build-api build run run-gateway run-api stop-gateway stop-api compose-up compose-down phase3-up check clean fmt verify verify-btf verify-phase14-cpu enterprise-check wal-test wal-faultfs

COMPOSE := docker compose -f $(WORKSPACE_ROOT)/docker-compose.yml

enterprise-check: build check
	@echo "==> Enterprise gate OK (build + check). Update docs/adr/skills if you changed architecture."

STATIX_INGEST_URL ?= http://127.0.0.1:3000/ingest

# bpf-linker MUST stay pinned. 0.11.0 dropped `aya-rustc-llvm-proxy`, so it can no
# longer build against rustc's bundled LLVM and needs a system LLVM 21+ (ADR 062).
# Keep in sync with .github/workflows/ebpf-ci.yml and deploy/docker/Dockerfile.statix.
BPF_LINKER_VERSION := 0.10.4

# Installs the toolchain from scratch on a clean Linux machine. Idempotent.
deps:
	@echo "==> Statix toolchain setup"
	@if [ "$$(uname -s)" != "Linux" ]; then \
		echo "ERROR: the eBPF toolchain only builds on Linux; this is $$(uname -s)."; \
		echo "       Run it inside the VM instead:"; \
		echo "         colima ssh -- bash -lc 'cd $(WORKSPACE_ROOT) && make deps'"; \
		exit 1; \
	fi
	@echo "--> [1/3] system packages (needs sudo)"
	@sudo apt-get update -qq
	@sudo apt-get install -y --no-install-recommends \
		clang llvm libelf-dev zlib1g-dev pkg-config build-essential curl ca-certificates
	@echo "--> [2/3] rust: stable + nightly + rust-src + rustfmt"
	@command -v rustup >/dev/null 2>&1 || \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	@rustup toolchain list | grep -q '^nightly' || rustup toolchain install nightly --profile minimal
	@rustup component list --toolchain nightly | grep -q 'rust-src (installed)' \
		|| rustup component add rust-src --toolchain nightly
	@rustup component list --toolchain nightly | grep -q 'rustfmt.*(installed)' \
		|| rustup component add rustfmt --toolchain nightly
	@echo "--> [3/3] bpf-linker, pinned to $(BPF_LINKER_VERSION)"
	@if [ "$$(bpf-linker --version 2>/dev/null | awk '{print $$2}')" != "$(BPF_LINKER_VERSION)" ]; then \
		echo "    installing bpf-linker $(BPF_LINKER_VERSION) (compiles, takes a few minutes)"; \
		cargo install bpf-linker --locked --version $(BPF_LINKER_VERSION) --force; \
	else \
		echo "    already at $(BPF_LINKER_VERSION)"; \
	fi
	@$(MAKE) --no-print-directory deps-check

# Read-only verification. Also checks the two kernel prerequisites the agent
# needs at runtime, which nothing else validated before.
deps-check:
	@echo "==> Toolchain check"
	@rc=0; \
	for tool in rustc cargo clang bpf-linker make gcc; do \
		if path=$$(command -v $$tool 2>/dev/null); then \
			printf '    %-12s %s\n' "$$tool" "$$path"; \
		else \
			printf '    %-12s MISSING\n' "$$tool"; rc=1; \
		fi; \
	done; \
	ver=$$(bpf-linker --version 2>/dev/null | awk '{print $$2}'); \
	if [ -n "$$ver" ] && [ "$$ver" != "$(BPF_LINKER_VERSION)" ]; then \
		printf '    %-12s version %s, expected $(BPF_LINKER_VERSION)\n' "bpf-linker" "$$ver"; rc=1; \
	fi; \
	if [ "$$(uname -s)" = "Linux" ]; then \
		if [ -r /sys/kernel/btf/vmlinux ]; then \
			printf '    %-12s present\n' "BTF"; \
		else \
			printf '    %-12s MISSING (/sys/kernel/btf/vmlinux) - agent cannot load\n' "BTF"; rc=1; \
		fi; \
		if [ "$$(stat -fc %T /sys/fs/cgroup 2>/dev/null)" = "cgroup2fs" ]; then \
			printf '    %-12s unified\n' "cgroup v2"; \
		else \
			printf '    %-12s NOT unified - attribution will fail\n' "cgroup v2"; rc=1; \
		fi; \
	fi; \
	if [ $$rc -eq 0 ]; then echo "==> All dependencies present"; \
	else echo "==> MISSING DEPENDENCIES (see above)"; exit 1; fi

# STATIX_RING_BUF_BYTES at compile time (statix-ebpf/build.rs) → three ELFs in target/bpf/
build-ebpf:
	@echo "==> [1/3] Compiling eBPF variants (small 512KB / large 4MB / xlarge 8MB)..."
	@mkdir -p "$(BPF_BUNDLE_DIR)"
	@set -e; \
	cd "$(EBPF_DIR)"; \
	for pair in "statix-ebpf-small:524288" "statix-ebpf-large:4194304" "statix-ebpf-xlarge:8388608"; do \
		name=$${pair%%:*}; bytes=$${pair##*:}; \
		echo "    $$name ($$bytes bytes)"; \
		STATIX_RING_BUF_BYTES=$$bytes cargo +nightly build --release \
			-Z build-std=core --target $(EBPF_TARGET); \
		cp "$(EBPF_RELEASE)" "$(BPF_BUNDLE_DIR)/$$name"; \
	done
	@echo "==> eBPF bundle: $(BPF_BUNDLE_DIR)/statix-ebpf-{small,large,xlarge}"

EBPF_BIN ?= $(BPF_BUNDLE_DIR)/statix-ebpf-small

# Back-compat alias (removed crate name finops-user).
build-user: build-agent

build-agent:
	@echo "==> [2/3] Compiling statix..."
	cd $(WORKSPACE_ROOT) && cargo build -p statix --release --bins
	@echo "==> Agent build complete."

build-gateway:
	@echo "==> [3/3] Compiling statix-gateway (ingest)..."
	cd $(WORKSPACE_ROOT) && cargo build -p statix-gateway --release
	@echo "==> Gateway build complete."

# Back-compat alias.
build-api: build-gateway

build: build-ebpf build-agent build-gateway
	@echo ""
	@echo "Build complete."
	@echo "  eBPF bytecode : $(EBPF_BIN)"
	@echo "  Agent binary  : $(WORKSPACE_ROOT)/target/release/statix"
	@echo "  Gateway binary: $(WORKSPACE_ROOT)/target/release/statix-gateway"
	@echo ""
	@echo "Phase 2: make run  (stdout only)"
	@echo "Phase 5 dev: make compose-up  then  STATIX_INGEST_URL=$(STATIX_INGEST_URL) sudo -E make run"

run: build
	@if [ ! -d "$(BPF_BUNDLE_DIR)" ] || [ -z "$$(ls -A '$(BPF_BUNDLE_DIR)' 2>/dev/null)" ]; then \
		echo "ERROR: eBPF bundle missing. Run 'make build-ebpf' first."; \
		exit 1; \
	fi
	@echo "==> Starting agent (Ctrl+C to stop)..."
	@echo "==> eBPF bundle: $(BPF_BUNDLE_DIR) (auto-pick by CPU count; override: STATIX_EBF_PATH)"
	@echo "==> Ingest: $(STATIX_INGEST_URL) (stdout-only: STATIX_INGEST_URL= make run)"
	RUST_LOG=info STATIX_BPF_DIR=$(BPF_BUNDLE_DIR) STATIX_INGEST_URL=$(STATIX_INGEST_URL) \
		$(WORKSPACE_ROOT)/target/release/statix

run-gateway: build-gateway
	@if $(COMPOSE) ps statix-gateway 2>/dev/null | grep -q '3000->3000'; then \
		echo "ERROR: Docker statix-gateway is already on :3000."; \
		echo "  Use the stack: make compose-up  (skip make run-gateway)"; \
		echo "  Or tear down first: make compose-down"; \
		exit 1; \
	fi
	@if ss -tlnp 2>/dev/null | grep -q ':3000 '; then \
		echo "ERROR: port 3000 is in use. Run: make stop-gateway"; \
		exit 1; \
	fi
	@echo "==> Starting statix-gateway on host..."
	@echo "    Prefer Docker stack: make compose-up"
	RUST_LOG=info,clickhouse=warn \
		$(WORKSPACE_ROOT)/target/release/statix-gateway

# Back-compat aliases.
run-api: run-gateway
stop-gateway: stop-api

# Kill only host statix-gateway binaries — never `fuser -k 3000` (that breaks Docker port-forward)
stop-api:
	@if curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "==> Gateway already healthy on :3000 — leaving it running."; \
		exit 0; \
	fi
	@echo "==> Stopping host statix-gateway (not Docker)..."
	@for exe in "$(WORKSPACE_ROOT)/target/release/statix-gateway" \
		"$(WORKSPACE_ROOT)/target/debug/statix-gateway"; do \
		[ -x "$$exe" ] || continue; \
		for pid in $$(pgrep -x statix-gateway 2>/dev/null); do \
			[ "$$(readlink -f /proc/$$pid/exe 2>/dev/null)" = "$$exe" ] \
				&& kill "$$pid" 2>/dev/null || true; \
		done; \
	done
	@sleep 1

compose-down:
	@echo "==> Stopping statix stack (compose)..."
	$(COMPOSE) down

# Legacy alias — same as compose-up (Phases 3–4 stack; Phase 5 adds auth).
phase3-up: compose-up

compose-up: stop-api
	@command -v docker >/dev/null 2>&1 || ( \
		echo "ERROR: docker not found. Install on Ubuntu:"; \
		echo "  sudo apt-get update && sudo apt-get install -y docker.io docker-compose-v2"; \
		echo "  sudo systemctl start docker"; \
		exit 127; \
	)
	@echo "==> Starting ClickHouse + statix-gateway (statix-net)..."
	$(COMPOSE) up -d
	@sleep 2
	@if ! curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "==> Recreating statix-gateway (:3000 not responding — e.g. after fuser on port 3000)..."; \
		$(COMPOSE) rm -sf statix-gateway; \
		$(COMPOSE) up -d statix-gateway; \
		sleep 4; \
	fi
	@if ! curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "ERROR: http://127.0.0.1:3000/health failed. Logs: docker compose logs statix-gateway"; \
		exit 1; \
	fi
	@echo "==> Stack ready. API: http://127.0.0.1:3000/health (OK)"
	@echo "==> Agent: export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest && sudo -E make run"

check:
	cd $(WORKSPACE_ROOT) && cargo check -p statix-common
	cd $(EBPF_DIR) && cargo +nightly check \
		-Z build-std=core --target $(EBPF_TARGET)
	cd $(WORKSPACE_ROOT) && cargo check -p statix-wire
	cd $(WORKSPACE_ROOT) && cargo check -p statix
	cd $(WORKSPACE_ROOT) && cargo check -p statix-infra
	cd $(WORKSPACE_ROOT) && cargo check -p statix-gateway

# Phase 11 — WAL spillway transactional-integrity tests (frame CRC, torn-tail
# recovery, segment rotation, hard-cap drop-oldest, crash replay, circuit breaker).
wal-test:
	cd $(WORKSPACE_ROOT) && cargo test -p statix wal

# Phase 11 — WAL disk-degradation (ENOSPC) test on a size-limited tmpfs (root).
wal-faultfs:
	sudo $(WORKSPACE_ROOT)/scripts/wal-faultfs.sh

# Phase 14 — CPU priming, conservation, soft miss (unit) + optional E2E (STATIX_PHASE14_E2E=1).
verify-phase14-cpu:
	$(WORKSPACE_ROOT)/scripts/verify-phase14-cpu.sh

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

clean:
	cd $(EBPF_DIR) && cargo clean
	cd $(WORKSPACE_ROOT) && cargo clean

fmt:
	cd $(WORKSPACE_ROOT) && cargo fmt --all
	cd $(EBPF_DIR) && cargo +nightly fmt
