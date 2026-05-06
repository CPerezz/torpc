# TorPC Makefile for Testing
# Manages service lifecycle intelligently - only stops services it started

# Color codes for output
RED := \033[0;31m
GREEN := \033[0;32m
YELLOW := \033[1;33m
BLUE := \033[0;34m
NC := \033[0m # No Color

# Service detection
SERVICES_STARTED_BY_MAKEFILE := .makefile_started_services

# Default test target — runs the FAST suite only (no daemons required).
# Phase 6 marked every service-dependent test with `#[ignore]`, so this
# target is now safe in CI and on a fresh clone. Use `make test-with-services`
# to additionally exercise the ignored set.
.PHONY: test
test:
	@echo "$(BLUE)Running fast tests across the whole workspace (no services required)...$(NC)"
	@RUST_LOG=warn cargo test --workspace --tests || \
		(echo "$(RED)✗ Fast tests failed$(NC)" && exit 1)
	@echo "$(BLUE)Running lib unit tests...$(NC)"
	@RUST_LOG=warn cargo test --workspace --lib || \
		(echo "$(RED)✗ Lib tests failed$(NC)" && exit 1)
	@echo "$(GREEN)✓ Fast test suite passed$(NC)"

# Full test target — same as before. Brings up daemons, runs everything
# including #[ignore]'d tests serially because some still mutate global env.
.PHONY: test-with-services
test-with-services: check-services run-tests-with-services cleanup-if-needed

# Check if services are already running
.PHONY: check-services
check-services:
	@echo "$(BLUE)Checking service status...$(NC)"
	@rm -f $(SERVICES_STARTED_BY_MAKEFILE)
	@NEED_START=0; \
	if ! pgrep -f "geth.*--dev" > /dev/null; then \
		echo "  $(YELLOW)Geth not running$(NC)"; \
		NEED_START=1; \
	else \
		echo "  $(GREEN)✓ Geth is running$(NC)"; \
	fi; \
	if ! pgrep -f "tor -f configs/torrc" > /dev/null; then \
		echo "  $(YELLOW)Tor not running$(NC)"; \
		NEED_START=1; \
	else \
		echo "  $(GREEN)✓ Tor is running$(NC)"; \
	fi; \
	if ! pgrep -f "target/release/torpc" > /dev/null; then \
		echo "  $(YELLOW)TorPC not running$(NC)"; \
		NEED_START=1; \
	else \
		echo "  $(GREEN)✓ TorPC is running$(NC)"; \
	fi; \
	if [ $$NEED_START -eq 1 ]; then \
		echo "$(YELLOW)Some services need to be started$(NC)"; \
		$(MAKE) start-services; \
	else \
		echo "$(GREEN)All services are already running$(NC)"; \
	fi

# Start services if needed
.PHONY: start-services
start-services:
	@echo "$(BLUE)Starting services via start-all-dev.sh...$(NC)"
	@./scripts/start-all-dev.sh
	@if [ $$? -eq 0 ]; then \
		echo "$(GREEN)✓ Services started successfully$(NC)"; \
		touch $(SERVICES_STARTED_BY_MAKEFILE); \
		echo "$(YELLOW)Waiting 10 seconds for services to stabilize...$(NC)"; \
		sleep 10; \
	else \
		echo "$(RED)✗ Failed to start services$(NC)"; \
		exit 1; \
	fi

# Run the FULL suite, including service-dependent #[ignore]'d tests.
# Tests run with --test-threads=1 because some still call std::env::set_var
# (which is process-global). Phase 6 follow-up will parameterise those out
# and let this run in parallel.
.PHONY: run-tests-with-services
run-tests-with-services:
	@echo "$(BLUE)Running full test suite (including service-dependent tests)...$(NC)"
	@echo "==============================="
	@RUST_LOG=info cargo test --tests --lib -- --include-ignored --test-threads=1 --nocapture || \
		(echo "$(RED)✗ Full suite failed$(NC)" && exit 1)
	@echo "==============================="
	@echo "$(GREEN)✓ Full suite passed$(NC)"

# Cleanup - only stop services if we started them
.PHONY: cleanup-if-needed
cleanup-if-needed:
	@if [ -f $(SERVICES_STARTED_BY_MAKEFILE) ]; then \
		echo "$(BLUE)Stopping services started by Makefile...$(NC)"; \
		./scripts/stop-all.sh; \
		rm -f $(SERVICES_STARTED_BY_MAKEFILE); \
	else \
		echo "$(BLUE)Services were already running - leaving them up$(NC)"; \
	fi

# Force stop all services (manual target)
.PHONY: stop-all
stop-all:
	@echo "$(BLUE)Force stopping all services...$(NC)"
	@./scripts/stop-all.sh
	@rm -f $(SERVICES_STARTED_BY_MAKEFILE)

# Clean build artifacts
.PHONY: clean
clean:
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	@cargo clean
	@rm -f $(SERVICES_STARTED_BY_MAKEFILE)

# Build the project
.PHONY: build
build:
	@echo "$(BLUE)Building TorPC...$(NC)"
	@cargo build --release

# Run unit tests only (no services needed)
.PHONY: test-unit
test-unit:
	@echo "$(BLUE)Running unit tests...$(NC)"
	@cargo test --lib --bins

# Run integration tests with existing services (assumes services are running)
.PHONY: test-integration-only
test-integration-only:
	@echo "$(BLUE)Running integration tests (assuming services are running)...$(NC)"
	@export RUST_LOG=info; \
	cargo test --test integration_tests -- --test-threads=1 --nocapture
	@echo "$(BLUE)Running MEV integration tests...$(NC)"
	@export RUST_LOG=info; \
	cargo test --test mev_integration_tests -- --test-threads=1 --nocapture

# Check service health
.PHONY: health-check
health-check:
	@echo "$(BLUE)Performing health check...$(NC)"
	@echo -n "  Geth RPC: "
	@curl -s -X POST -H "Content-Type: application/json" \
		--data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
		http://127.0.0.1:8545 > /dev/null && echo "$(GREEN)✓ Responding$(NC)" || echo "$(RED)✗ Not responding$(NC)"
	@echo -n "  TorPC HTTP: "
	@curl -s http://127.0.0.1:8080/ > /dev/null && echo "$(GREEN)✓ Responding$(NC)" || echo "$(RED)✗ Not responding$(NC)"
	@echo -n "  TorPC RPC: "
	@curl -s -X POST -H "Content-Type: application/json" \
		--data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
		http://127.0.0.1:8080/rpc > /dev/null && echo "$(GREEN)✓ Responding$(NC)" || echo "$(RED)✗ Not responding$(NC)"
	@if [ -f data/tor/torpc/hostname ]; then \
		ONION=$$(cat data/tor/torpc/hostname); \
		echo "  Tor hidden service: $(YELLOW)$$ONION$(NC)"; \
	else \
		echo "  Tor hidden service: $(YELLOW)Not configured$(NC)"; \
	fi

# Show logs
.PHONY: logs
logs:
	@echo "$(BLUE)Showing service logs (Ctrl+C to exit)...$(NC)"
	@echo "==============================="
	@tail -f data/geth-dev/geth.log data/tor/tor.log data/torpc.log

# Help target
.PHONY: help
help:
	@echo "$(BLUE)TorPC Makefile - Available targets:$(NC)"
	@echo ""
	@echo "  $(YELLOW)make test$(NC)              - Run integration tests (starts services if needed)"
	@echo "  $(YELLOW)make test-unit$(NC)         - Run unit tests only (no services required)"
	@echo "  $(YELLOW)make test-integration-only$(NC) - Run integration tests (assumes services running)"
	@echo "  $(YELLOW)make build$(NC)             - Build the TorPC binary"
	@echo "  $(YELLOW)make health-check$(NC)      - Check if services are responding"
	@echo "  $(YELLOW)make logs$(NC)              - Tail all service logs"
	@echo "  $(YELLOW)make stop-all$(NC)          - Force stop all services"
	@echo "  $(YELLOW)make clean$(NC)             - Clean build artifacts"
	@echo "  $(YELLOW)make help$(NC)              - Show this help message"
	@echo ""
	@echo "$(GREEN)Smart service management:$(NC)"
	@echo "  - Services are only started if not already running"
	@echo "  - Services are only stopped if the Makefile started them"
	@echo "  - Use 'make stop-all' to force stop all services"

# Default if no target specified
.DEFAULT_GOAL := help