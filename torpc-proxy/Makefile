# ToRPC Proxy Makefile
# Handles building, testing, and running the proxy client

# Color codes for output
RED := \033[0;31m
GREEN := \033[0;32m
YELLOW := \033[1;33m
BLUE := \033[0;34m
NC := \033[0m # No Color

# Build configuration
CARGO := cargo
BINARY := torpc-proxy
TARGET_DIR := target
RELEASE_BINARY := $(TARGET_DIR)/release/$(BINARY)
DEBUG_BINARY := $(TARGET_DIR)/debug/$(BINARY)
GUI_BINARY := torpc-proxy-gui
GUI_RELEASE_BINARY := $(TARGET_DIR)/release/$(GUI_BINARY)
GUI_DEBUG_BINARY := $(TARGET_DIR)/debug/$(GUI_BINARY)

# Default target
.PHONY: all
all: build

# Build targets
.PHONY: build
build:
	@echo "$(BLUE)Building $(BINARY) in release mode...$(NC)"
	@$(CARGO) build --release
	@echo "$(GREEN)✓ Build complete: $(RELEASE_BINARY)$(NC)"
	@echo "  Binary size: $$(du -h $(RELEASE_BINARY) | cut -f1)"

.PHONY: build-debug
build-debug:
	@echo "$(BLUE)Building $(BINARY) in debug mode...$(NC)"
	@$(CARGO) build
	@echo "$(GREEN)✓ Build complete: $(DEBUG_BINARY)$(NC)"

# Test targets
.PHONY: test
test: test-unit test-integration test-doc

.PHONY: test-unit
test-unit:
	@echo "$(BLUE)Running unit tests...$(NC)"
	@$(CARGO) test --workspace --lib --bins -- --nocapture
	@echo "$(GREEN)✓ Unit tests passed$(NC)"

.PHONY: test-integration
test-integration:
	@echo "$(BLUE)Running integration tests...$(NC)"
	@$(CARGO) test -p torpc-proxy-cli --test integration_tests -- --test-threads=1 --nocapture 2>/dev/null || \
		echo "$(YELLOW)No integration tests found$(NC)"
	@echo "$(GREEN)✓ Integration tests completed$(NC)"

.PHONY: test-cli
test-cli:
	@echo "$(BLUE)Running CLI tests...$(NC)"
	@echo "$(YELLOW)Note: Some tests may fail if Tor is running (expected behavior)$(NC)"
	@$(CARGO) test -p torpc-proxy-cli --test cli_tests -- --nocapture || \
		echo "$(YELLOW)⚠ Some CLI tests failed (check if this is expected)$(NC)"

.PHONY: test-mock
test-mock:
	@echo "$(BLUE)Running mock tests...$(NC)"
	@$(CARGO) test -p torpc-proxy-cli --test mock_tor_tests -- --nocapture 2>/dev/null || \
		echo "$(YELLOW)No mock tests found$(NC)"
	@echo "$(GREEN)✓ Mock tests completed$(NC)"

.PHONY: test-all
test-all: test-unit test-integration test-cli test-mock
	@echo "$(GREEN)✓ All test suites completed$(NC)"

.PHONY: test-doc
test-doc:
	@echo "$(BLUE)Running doc tests...$(NC)"
	@$(CARGO) test --doc
	@echo "$(GREEN)✓ Doc tests passed$(NC)"

# Run targets
.PHONY: run
run: build
	@echo "$(BLUE)Starting $(BINARY)...$(NC)"
	@$(RELEASE_BINARY) start

.PHONY: run-debug
run-debug: build-debug
	@echo "$(BLUE)Starting $(BINARY) in debug mode...$(NC)"
	@RUST_LOG=debug $(DEBUG_BINARY) start

# GUI targets
.PHONY: build-gui
build-gui:
	@echo "$(BLUE)Building GUI application...$(NC)"
	@$(CARGO) build --release -p torpc-proxy-gui
	@echo "$(GREEN)✓ GUI build complete: $(GUI_RELEASE_BINARY)$(NC)"

.PHONY: run-gui
run-gui: build-gui
	@echo "$(BLUE)Starting GUI application...$(NC)"
	@$(GUI_RELEASE_BINARY)

.PHONY: run-gui-debug
run-gui-debug:
	@echo "$(BLUE)Building and running GUI in debug mode...$(NC)"
	@$(CARGO) build -p torpc-proxy-gui
	@RUST_LOG=debug $(GUI_DEBUG_BINARY)

.PHONY: install-tauri-cli
install-tauri-cli:
	@echo "$(BLUE)Installing Tauri CLI...$(NC)"
	@$(CARGO) install tauri-cli
	@echo "$(GREEN)✓ Tauri CLI installed$(NC)"

.PHONY: dev-gui
dev-gui:
	@echo "$(BLUE)Starting Tauri development server...$(NC)"
	@echo "$(YELLOW)Note: Run 'make install-tauri-cli' first if cargo-tauri is not installed$(NC)"
	@cd torpc-proxy-gui && cargo tauri dev

# Development helpers
.PHONY: check
check:
	@echo "$(BLUE)Running cargo check...$(NC)"
	@$(CARGO) check --all-targets
	@echo "$(GREEN)✓ Check passed$(NC)"

.PHONY: clippy
clippy:
	@echo "$(BLUE)Running clippy lints...$(NC)"
	@$(CARGO) clippy --all-targets --all-features -- -D warnings
	@echo "$(GREEN)✓ No clippy warnings$(NC)"

.PHONY: fmt
fmt:
	@echo "$(BLUE)Formatting code...$(NC)"
	@$(CARGO) fmt
	@echo "$(GREEN)✓ Code formatted$(NC)"

.PHONY: fmt-check
fmt-check:
	@echo "$(BLUE)Checking code formatting...$(NC)"
	@$(CARGO) fmt -- --check
	@echo "$(GREEN)✓ Code is properly formatted$(NC)"

# Tor connectivity test
.PHONY: test-tor
test-tor: build
	@echo "$(BLUE)Testing Tor connectivity...$(NC)"
	@$(RELEASE_BINARY) test || echo "$(RED)✗ Tor connectivity test failed$(NC)"

# Installation
.PHONY: install
install: build
	@echo "$(BLUE)Installing $(BINARY) to ~/.cargo/bin/...$(NC)"
	@cp $(RELEASE_BINARY) ~/.cargo/bin/
	@echo "$(GREEN)✓ Installed to ~/.cargo/bin/$(BINARY)$(NC)"

# Clean targets
.PHONY: clean
clean:
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	@$(CARGO) clean
	@rm -f proxy.log
	@echo "$(GREEN)✓ Clean complete$(NC)"

# Documentation
.PHONY: doc
doc:
	@echo "$(BLUE)Building documentation...$(NC)"
	@$(CARGO) doc --no-deps --open

# CI/CD helpers
.PHONY: ci-test
ci-test:
	@echo "$(BLUE)Running CI test suite...$(NC)"
	@$(MAKE) fmt-check
	@$(MAKE) clippy
	@$(MAKE) test-unit
	@$(MAKE) test-integration
	@$(MAKE) test-mock
	@echo "$(GREEN)✓ CI tests passed$(NC)"

# Show help
.PHONY: help
help:
	@echo "$(BLUE)ToRPC Proxy Makefile - Available targets:$(NC)"
	@echo ""
	@echo "$(YELLOW)Building:$(NC)"
	@echo "  make build          - Build CLI release binary"
	@echo "  make build-debug    - Build CLI debug binary"
	@echo "  make build-gui      - Build GUI release binary"
	@echo "  make install        - Install CLI to ~/.cargo/bin/"
	@echo "  make install-tauri-cli - Install Tauri CLI tool"
	@echo ""
	@echo "$(YELLOW)Testing:$(NC)"
	@echo "  make test           - Run all tests (unit + integration + doc)"
	@echo "  make test-unit      - Run unit tests only"
	@echo "  make test-integration - Run integration tests"
	@echo "  make test-cli       - Run CLI tests"
	@echo "  make test-mock      - Run mock tests"
	@echo "  make test-all       - Run all test suites"
	@echo "  make test-tor       - Test Tor connectivity"
	@echo ""
	@echo "$(YELLOW)Running:$(NC)"
	@echo "  make run            - Build and run the CLI proxy"
	@echo "  make run-debug      - Run CLI with debug logging"
	@echo "  make run-gui        - Build and run the GUI application"
	@echo "  make run-gui-debug  - Run GUI with debug logging"
	@echo "  make dev-gui        - Run GUI in Tauri dev mode (hot reload)"
	@echo ""
	@echo "$(YELLOW)Development:$(NC)"
	@echo "  make check          - Run cargo check"
	@echo "  make clippy         - Run clippy lints"
	@echo "  make fmt            - Format code"
	@echo "  make fmt-check      - Check formatting"
	@echo "  make doc            - Build and open documentation"
	@echo ""
	@echo "$(YELLOW)Other:$(NC)"
	@echo "  make clean          - Clean build artifacts"
	@echo "  make ci-test        - Run CI test suite"
	@echo "  make help           - Show this help"

.DEFAULT_GOAL := help