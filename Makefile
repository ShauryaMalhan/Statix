# ============================================================
# FinOps eBPF Agent — Build & Run
# ============================================================

WORKSPACE_ROOT := $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
EBPF_DIR       := $(WORKSPACE_ROOT)/finops-ebpf
USER_DIR       := $(WORKSPACE_ROOT)/finops-user

export PATH := $(HOME)/.cargo/bin:$(PATH)

EBPF_OUT_NAME  := finops-ebpf
EBPF_TARGET    := bpfel-unknown-none

.PHONY: deps build-ebpf build-user build-api build run run-api compose-up check clean fmt verify verify-btf enterprise-check

enterprise-check: build check
	@echo "==> Enterprise gate OK (build + check). Update docs/adr/skills if you changed architecture."

FINOPS_INGEST_URL ?= http://localhost:3000/ingest

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
	@echo "Phase 3: make compose-up && make run-api  (terminal) && FINOPS_INGEST_URL=$(FINOPS_INGEST_URL) sudo -E make run"

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
