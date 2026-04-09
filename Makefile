PRIVATE := target/.private
TEST_FLAGS := LLVM_PROFILE_FILE=$(PRIVATE)/profile/cargo-test-%p-%m.profraw

RUSTFLAGS := -C instrument-coverage
ENV_FLAGS := $(TEST_FLAGS)

# Coverage flags
RUSTFLAGS_COV := -C instrument-coverage \
                 -C link-dead-code \
                 -C opt-level=0 \
                 -C debuginfo=2

.PHONY: all
all: $(PRIVATE)
	$(ENV_FLAGS) RUSTFLAGS="$(RUSTFLAGS)" cargo build --workspace

.PHONY: dist
dist:
	cargo package --workspace --allow-dirty

.PHONY: archive
archive: archive.tar.zst
archive.tar.zst: README.md CHANGELOG.md Cargo.toml $(shell find src -name "*.rs") $(shell find man -type f)
	tar --zstd -cvf $@ \
        README.md \
        CHANGELOG.md \
        Cargo.toml \
        docs \
        man \
        src \
        tests

.PHONY: clean
clean:
	cargo clean

.PHONY: check
check: test

$(PRIVATE):
	mkdir -p $@


#.PHONY: test
#test:
#	rm -rf $(PRIVATE)/profile
#	$(ENV_FLAGS) RUST_BACKTRACE=1 RUSTFLAGS="$(RUSTFLAGS)" cargo test --workspace --all-features --verbose

.PHONY: test-unit
test-unit:
	RUST_BACKTRACE=1 cargo test --workspace --all-features \
	    --lib --bins \
	    $(if $(VERBOSE),--verbose)

.PHONY: test-integration
test-integration: $(MWCCPSP) $(ALLEGREX)
	RUST_BACKTRACE=1 cargo test --workspace --all-features \
	    --test '*' \
	    $(if $(VERBOSE),--verbose)

.PHONY: test
test: test-unit test-integration

.PHONY: lint
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-features -- -D warnings

.PHONY: fmt
fmt:
	cargo fmt --all


# coverage
# Install llvm-cov if not present
.PHONY: coverage-install
coverage-install:
	@echo "Installing coverage tools..."
	rustup component add llvm-tools-preview
	cargo install cargo-llvm-cov || true

# Run tests with coverage
.PHONY: coverage
coverage: coverage-clean
	@echo "Running tests with coverage instrumentation..."
	@mkdir -p $(PRIVATE)/coverage
	RUST_BACKTRACE=1 cargo llvm-cov --all-features --workspace --lcov --output-path $(PRIVATE)/coverage/lcov.info

# Generate HTML report (human-readable)
.PHONY: coverage-html
coverage-html: coverage
	@echo "Generating HTML coverage report..."
	cargo llvm-cov --all-features --workspace --html
	@echo "Coverage report generated at: target/llvm-cov/html/index.html"
	@echo "Open with: open target/llvm-cov/html/index.html (macOS) or xdg-open target/llvm-cov/html/index.html (Linux)"

# Generate JSON report (machine-readable)
.PHONY: coverage-json
coverage-json: coverage
	@echo "Generating JSON coverage report..."
	@mkdir -p $(PRIVATE)/coverage
	cargo llvm-cov --all-features --workspace --json --output-path $(PRIVATE)/coverage/coverage.json

# Generate text report (terminal output)
.PHONY: coverage-text
coverage-text: coverage
	@echo "Generating text coverage report..."
	cargo llvm-cov --all-features --workspace

# Generate all formats
.PHONY: coverage-all
coverage-all: coverage-html coverage-json coverage-text
	@echo "Coverage reports generated:"
	@echo "  - LCOV:  $(PRIVATE)/coverage/lcov.info"
	@echo "  - JSON:  $(PRIVATE)/coverage/coverage.json"
	@echo "  - HTML:  target/llvm-cov/html/index.html"

# Upload to codecov.io
.PHONY: coverage-upload
coverage-upload: coverage
	@echo "Uploading coverage to codecov.io..."
	@if [ -z "$$CODECOV_TOKEN" ]; then \
		echo "Error: CODECOV_TOKEN environment variable not set"; \
		echo "Get token from: https://codecov.io/gh/ttkb-oss/psy-k/settings"; \
		exit 1; \
	fi
	curl -Os https://uploader.codecov.io/latest/linux/codecov
	chmod +x codecov
	./codecov -t $$CODECOV_TOKEN -f $(PRIVATE)/coverage/lcov.info
	rm -f codecov

# Clean coverage artifacts
.PHONY: coverage-clean
coverage-clean:
	@echo "Cleaning coverage artifacts..."
	rm -rf $(PRIVATE)/coverage
	rm -rf target/llvm-cov
	cargo llvm-cov clean --workspace || true


BIN_DIR         := $(PRIVATE)/bin
WIBO            := $(BIN_DIR)/wibo
ALLEGREX 		:= $(BIN_DIR)/allegrex-as
MWCCPSP         := $(BIN_DIR)/mwccpsp.exe
.PHONY: dependencies dependencies-pspeu dependencies-debian
dependencies: dependencies-pspeu dependencies-debian
dependencies-debian:
	sudo apt-get install -y curl clang lld binutils-mipsel-linux-gnu
dependencies-pspeu: $(ALLEGREX) $(MWCCPSP)
$(WIBO):
	mkdir -p $(BIN_DIR)
	curl -sSfL -o $@ https://github.com/decompals/wibo/releases/download/0.6.13/wibo
	# $(muffle)sha256sum --check $(WIBO).sha256
	chmod +x $(WIBO)
$(MWCCPSP): $(WIBO) $(BIN_DIR)/mwccpsp_219
$(BIN_DIR)/mwccpsp_219:
	mkdir -p $(BIN_DIR)
	wget -O $(PRIVATE)/mwccpsp_219.tar.gz https://github.com/Xeeynamo/sotn-decomp/releases/download/cc1-psx-26/mwccpsp_219.tar.gz
	cd $(BIN_DIR) ; tar -xvzf ../mwccpsp_219.tar.gz
$(ALLEGREX):
	mkdir -p $(BIN_DIR)
	wget -O $(PRIVATE)/allegrex-as.tar.gz https://github.com/Xeeynamo/sotn-decomp/releases/download/cc1-psx-26/allegrex-as.tar.gz
	cd $(BIN_DIR) ; tar -xvzf ../allegrex-as.tar.gz



# produce a mwccgap object for comparison
.PHONY: mwccgap
mwccgap: # $(WIBO) $(MWCCPSP)
	time ../mwccgap/mwccgap.py \
        tests/data/compiler.c \
        compiler.o \
        --mwcc-path target/.private/bin/mwccpsp.exe \
        --use-wibo --wibo-path target/.private/bin/wibo \
        --src-dir . \
        --asm-dir . \
        --macro-inc-path tests/data/macro.inc \
        -Itests/data

.PHONY: mwccgap
mwccgap-rodata: # $(WIBO) $(MWCCPSP)
	time ../sotn-decomp/.venv/bin/python3 ../sotn-decomp/tools/mwccgap/mwccgap.py \
        tests/data/only_rodata.c \
        only_rodata.o \
        --mwcc-path target/.private/bin/mwccpsp.exe \
        --use-wibo --wibo-path target/.private/bin/wibo \
        --src-dir . \
        --asm-dir . \
        --macro-inc-path tests/data/macro.inc \
        -Itests/data


.PHONY: mw
mw: # $(WIBO) $(MWCCPSP)
	time cargo run --release -- \
        tests/data/compiler.c \
        compiler.o \
        --mwcc-path target/.private/bin/mwccpsp.exe \
        --use-wibo --wibo-path target/.private/bin/wibo \
        --src-dir . \
        --asm-dir . \
        --macro-inc-path tests/data/macro.inc

.PHONY: mw
mw-rodata: # $(WIBO) $(MWCCPSP)
	time cargo run --release -- \
        -o only_rodata.o \
        --mwcc-path target/.private/bin/mwccpsp.exe \
        --use-wibo --wibo-path target/.private/bin/wibo \
        --src-dir tests/data \
        --asm-dir . \
        --macro-inc-path tests/data/macro.inc \
        -Itests/data \
        tests/data/only_rodata.c \


.PHONY: sotn-mwccgap
sotn-mwccgap:
	cd ../sotn-decomp; \
        VERSION=pspeu tools/sotn_str/target/release/sotn_str process -p -f src/main_psp/31178.c | \
        time .venv/bin/python3 tools/mwccgap/mwccgap.py \
         ../metrowrap/31178.c.mwccgap.o \
         --src-dir src/main_psp --mwcc-path bin/mwccpsp.exe --use-wibo --wibo-path bin/wibo --as-path tools/pspas/target/release/pspas --asm-dir-prefix asm/pspeu --target-encoding utf8 --macro-inc-path include/macro.inc -gccinc -Iinclude -Iinclude/pspsdk -D_internal_version_pspeu -DSOTN_STR  -c -lang c -sdatathreshold 0 -char unsigned -fl divbyzerocheck -Op -opt nointrinsics

.PHONY: sotn-mw
sotn-mw:
	cargo build --release
	cd ../sotn-decomp; \
    VERSION=pspeu tools/sotn_str/target/release/sotn_str process -p -f src/main_psp/36174.c | \
    ~/.cargo/bin/flamegraph -o ../metrowrap/36174.c.svg -- time ../metrowrap/target/release/mw \
        -o ../metrowrap/36174.c.mw.o \
        --src-dir src/main_psp \
        --mwcc-path bin/mwccpsp.exe \
        --use-wibo --wibo-path bin/wibo \
        --as-path tools/pspas/target/release/pspas \
        --asm-dir asm/pspeu \
        --macro-inc-path include/macro.inc \
        -gccdep \
        -MMD \
        -gccinc \
        -Iinclude \
        -Iinclude/pspsdk \
        -D_internal_version_pspeu -DSOTN_STR  -c -lang c -sdatathreshold 0 -char unsigned \
        -fl divbyzerocheck -Op \
        -opt nointrinsics \
        -


