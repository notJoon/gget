.PHONY: build test bench clean update-deps

build:
	cargo build --release

test:
	cargo test --release

bench:
	cargo bench

update-deps:
	cargo update

update-deps-major:
	cargo update --aggressive

clean:
	cargo clean

fmt:
	cargo fmt

lint:
	cargo clippy

help:
	@echo "Available commands:"
	@echo "  make build          - build in release mode"
	@echo "  make test           - run tests"
	@echo "  make bench          - run benchmarks"
	@echo "  make bench-report   - create html report"
	@echo "  make update-deps    - update dependencies"
	@echo "  make update-deps-major - update dependencies (major version included)"
	@echo "  make clean          - clean build files"
	@echo "  make fmt            - format code"
	@echo "  make lint           - lint code"
	@echo "  make help           - show this help"
