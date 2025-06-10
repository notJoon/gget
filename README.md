# gget

[![Build Status](https://github.com/notJoon/gget/actions/workflows/main.yml/badge.svg)](https://github.com/notJoon/gget/actions/workflows/main.yml)
[![GitHub release (latest by date)](https://img.shields.io/github/v/release/notJoon/gget)](https://github.com/notJoon/gget/releases)

A package manager for downloading and managing packages through Gno.land RPC endpoints.

## Core Features

- Package downloading

TODO
- Package dependency management
- Package version management
- Local cache management

## Installation

### Binary Download

TODO

Download the appropriate binary for your operating system from the latest release:

```bash
# Linux
curl -L https://github.com/notJoon/gget/releases/latest/download/gget-linux -o gget
chmod +x gget

# macOS
curl -L https://github.com/notJoon/gget/releases/latest/download/gget-macos -o gget
chmod +x gget
```

### Build from Source

```bash
git clone https://github.com/notJoon/gget.git
cd gget
cargo build --release
```

## Usage

### Download Package

```bash
gget download gno.land/p/demo/avl
```

### List Package Contents

TODO

```bash
gget list gno.land/p/demo/avl
```

## License

See the [LICENSE](LICENSE) file for details.
