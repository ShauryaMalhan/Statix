# ============================================================
# FinOps eBPF Agent — Build & Run
# ============================================================

WORKSPACE_ROOT := $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
EBPF_DIR       := $(WORKSPACE_ROOT)/finops-ebpf
USER_DIR       := $(WORKSPACE_ROOT)/finops-user

export PATH := $(HOME)/.cargo/bin:$(PATH)

EBPF_OUT_NAME  := finops-ebpf
EBPF_TARGET    := bpfel-unknown-none

.PHONY: deps build-ebpf build-user build-api build run run-api stop-api compose-up compose-down phase3-up check clean fmt verify verify-btf enterprise-check

COMPOSE := docker compose -f $(WORKSPACE_ROOT)/docker-compose.yml

enterprise-check: build check
	@echo "==> Enterprise gate OK (build + check). Update docs/adr/skills if you changed architecture."

FINOPS_INGEST_URL ?= http://127.0.0.1:3000/ingest

deps:
	@echo "==> Checking toolchain..."
	@rustc --version || (echo "Install Rust: https://rustup.rs" && exit 1)
	@rustup toolchain list | grep -q nightly || rustup toolchain install nightly
	@rustup component list --toolchain nightly | grep -q "rust-src (installed)" \
		|| rustup component add rust-src --toolchain nightly
	@which bpf-linker || cargo install bpf-linker
	@clang --version > /dev/null || (echo "Install: apt install clang" && exit 1)
	@echo "==> All dependencies present"

build-ebpf:
	@echo "==> [1/3] Compiling eBPF kernel program..."
	cd $(EBPF_DIR) && cargo +nightly build --release \
		-Z build-std=core \
		--target $(EBPF_TARGET)
	@echo "==> eBPF build complete."

EBPF_BIN = $(shell \
	find "$(EBPF_DIR)/target" \
		-path "*/$(EBPF_TARGET)/release/$(EBPF_OUT_NAME)" \
		-not -name "*.d" \
		-type f 2>/dev/null | head -1)

build-user:
	@echo "==> [2/3] Compiling user-space agent..."
	cd $(WORKSPACE_ROOT) && cargo build -p finops-user --release
	@echo "==> Agent build complete."

build-api:
	@echo "==> [3/3] Compiling finops-api (ingest)..."
	cd $(WORKSPACE_ROOT) && cargo build -p finops-api --release
	@echo "==> API build complete."

build: build-ebpf build-user build-api
	@echo ""
	@echo "Build complete."
	@echo "  eBPF bytecode : $(EBPF_BIN)"
	@echo "  Agent binary  : $(WORKSPACE_ROOT)/target/release/finops-user"
	@echo "  API binary    : $(WORKSPACE_ROOT)/target/release/finops-api"
	@echo ""
	@echo "Phase 2: make run"
	@echo "Phase 3: make compose-up  (stack)  then  FINOPS_INGEST_URL=$(FINOPS_INGEST_URL) sudo -E make run"

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

run-api: build-api
	@if $(COMPOSE) ps finops-api 2>/dev/null | grep -q '3000->3000'; then \
		echo "ERROR: Docker finops-api is already on :3000."; \
		echo "  Use the stack: make compose-up  (skip make run-api)"; \
		echo "  Or tear down first: make compose-down"; \
		exit 1; \
	fi
	@if ss -tlnp 2>/dev/null | grep -q ':3000 '; then \
		echo "ERROR: port 3000 is in use. Run: make stop-api"; \
		exit 1; \
	fi
	@echo "==> Starting finops-api on host (KAFKA_BROKERS=localhost:9092)..."
	@echo "    Prefer Phase 3 Docker API: make compose-up"
	RUST_LOG=info KAFKA_BROKERS=localhost:9092 \
		$(WORKSPACE_ROOT)/target/release/finops-api

# Kill only host finops-api binaries — never `fuser -k 3000` (that breaks Docker port-forward)
stop-api:
	@if curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "==> API already healthy on :3000 — leaving it running."; \
		exit 0; \
	fi
	@echo "==> Stopping host finops-api (not Docker)..."
	@for exe in "$(WORKSPACE_ROOT)/target/release/finops-api" \
		"$(WORKSPACE_ROOT)/target/debug/finops-api"; do \
		[ -x "$$exe" ] || continue; \
		for pid in $$(pgrep -x finops-api 2>/dev/null); do \
			[ "$$(readlink -f /proc/$$pid/exe 2>/dev/null)" = "$$exe" ] \
				&& kill "$$pid" 2>/dev/null || true; \
		done; \
	done
	@sleep 1

compose-down:
	@echo "==> Stopping Phase 3 stack..."
	$(COMPOSE) down

# Default Phase 3: Docker stack (API in compose). Auto-frees :3000 and fixes broken API containers.
phase3-up: compose-up

compose-up: stop-api
	@command -v docker >/dev/null 2>&1 || ( \
		echo "ERROR: docker not found. Install on Ubuntu:"; \
		echo "  sudo apt-get update && sudo apt-get install -y docker.io docker-compose-v2"; \
		echo "  sudo systemctl start docker"; \
		exit 127; \
	)
	@echo "==> Starting Kafka + ClickHouse + finops-api (finops-net)..."
	$(COMPOSE) up -d
	@sleep 2
	@if ! curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "==> Recreating finops-api (:3000 not responding — e.g. after fuser on port 3000)..."; \
		$(COMPOSE) rm -sf finops-api; \
		$(COMPOSE) up -d finops-api; \
		sleep 4; \
	fi
	@if ! curl -sf -o /dev/null http://127.0.0.1:3000/health 2>/dev/null; then \
		echo "ERROR: http://127.0.0.1:3000/health failed. Logs: docker compose logs finops-api"; \
		exit 1; \
	fi
	@echo "==> Stack ready. API: http://127.0.0.1:3000/health (OK)"
	@echo "==> Agent: export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest && sudo -E make run"

check:
	cd $(WORKSPACE_ROOT) && cargo check -p finops-common
	cd $(EBPF_DIR) && cargo +nightly check \
		-Z build-std=core --target $(EBPF_TARGET)
	cd $(WORKSPACE_ROOT) && cargo check -p finops-user
	cd $(WORKSPACE_ROOT) && cargo check -p finops-api

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
