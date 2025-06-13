# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**gget** is a package manager for downloading and managing packages through Gno.land RPC endpoints. It's written in Rust and provides CLI functionality for fetching packages from gno.land with dependency resolution capabilities.

## Key Commands

### Building and Testing
```bash
# Build the project
cargo build --release  # or: make build

# Run tests
cargo test           # Run all tests
cargo test --release # or: make test
cargo test -- --nocapture  # Show test output

# Run benchmarks
cargo bench          # or: make bench

# Format code
cargo fmt            # or: make fmt

# Lint code
cargo clippy         # or: make lint
```

### Running the Application
```bash
# Basic usage - download a package
./target/release/gget add gno.land/p/demo/avl

# With options
./target/release/gget add gno.land/p/demo/avl --output ./packages --resolve-deps
```

## Architecture Overview

### Core Components

1. **Package Fetching** (`src/fetch.rs`)
   - Connects to Gno.land RPC endpoints (default: `https://rpc.gno.land:443`)
   - Downloads package content via ABCI queries
   - Supports atomic downloads to prevent partial files

2. **Dependency Resolution** (`src/dependency.rs`)
   - Uses tree-sitter to parse Go import statements
   - Recursively resolves package dependencies
   - Builds dependency graphs for complex packages

3. **Cache Management** (`src/cache.rs`)
   - Implements in-memory caching with Moka
   - Stores downloaded packages locally
   - Prevents redundant downloads

4. **CLI Interface** (`src/main.rs`)
   - Built with clap for argument parsing
   - Subcommand structure (currently `add` command)
   - Supports various flags for controlling behavior

### Key Design Patterns

- **Async Architecture**: Uses Tokio for concurrent operations
- **Error Handling**: Custom error types with thiserror
- **Testing**: Integration tests use real RPC endpoints (some marked with `#[ignore]` for CI)
- **Validation**: Package validation through RPC queries before download

### Important Notes

- The project uses tree-sitter with a custom Go grammar for parsing dependencies
- Tests against real RPC endpoints may be slow or fail if the endpoint is unavailable
- The default cache directory is `~/.cache/gget/packages`
- Force download (`--force`) bypasses cache to re-fetch packages