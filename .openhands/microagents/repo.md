# OpenAI Codex CLI Repository

## Project Description

OpenAI Codex CLI is a local coding agent that runs on your computer, providing AI-powered assistance for development tasks. It's part of OpenAI's Codex ecosystem, designed to work alongside your existing development workflow. The CLI can be installed globally via npm or Homebrew and integrates with your ChatGPT plan for authentication.

## File Structure

This is a monorepo containing both Rust and TypeScript/Node.js components:

- **`codex-rs/`** - Core Rust implementation containing multiple workspace crates:
  - `cli/` - Main CLI application
  - `core/` - Core functionality
  - `tui/` - Terminal user interface
  - `mcp-*` - Model Context Protocol implementation
  - `login/`, `exec/`, `file-search/` - Various utility crates
- **`codex-cli/`** - Node.js/TypeScript wrapper and npm package
- **`docs/`** - Comprehensive documentation including getting started, configuration, and advanced usage
- **`scripts/`** - Build and maintenance scripts
- **Root files** - Configuration for pnpm workspace, Nix flake, and project metadata

## Development Commands

### Building from Source
```bash
# Navigate to Rust workspace
cd codex-rs

# Install Rust toolchain (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt clippy

# Build the project
cargo build

# Run with a sample prompt
cargo run --bin codex -- "explain this codebase to me"
```

### Testing and Code Quality
```bash
# Run tests
cargo test

# Format code
cargo fmt -- --config imports_granularity=Item

# Run linter
cargo clippy --tests

# Format JavaScript/JSON/Markdown files
pnpm format:fix
```

### Package Management
The project uses pnpm for JavaScript dependencies and Cargo for Rust. Node.js 22+ and pnpm 9+ are required.

## Getting Started for New Developers

1. **Prerequisites**: Ensure you have Rust toolchain, Node.js 22+, and pnpm 9+ installed
2. **Clone and setup**: The repository includes comprehensive documentation in the `docs/` folder
3. **Read contributing guidelines**: Check `docs/contributing.md` for development workflow and PR requirements
4. **Authentication**: The CLI supports ChatGPT login or API key authentication (see `docs/authentication.md`)
5. **Configuration**: User preferences are stored in `~/.codex/config.toml` (see `docs/config.md`)

The project is under active development with a focus on bug fixes and security improvements from external contributors. New features require prior approval via GitHub issues.