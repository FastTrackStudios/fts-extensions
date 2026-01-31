# Justfile - Convenient commands for roam-test
# Install just: cargo install just
# Run commands: just <recipe-name>

# Default recipe - show help
_default:
    @just --list

# Run tracey dashboard server
@tracey:
    cargo xtask tracey check

# Generate traceability matrix
@tracey-matrix:
    cargo xtask tracey matrix

# Extract rules from specs
@tracey-rules:
    cargo xtask tracey rules

# Show impact analysis
@tracey-impact:
    cargo xtask tracey impact

# Build spec documentation
@dodeca:
    cargo xtask dodeca build

# Serve spec documentation locally
@dodeca-serve:
    cargo xtask dodeca serve

# Watch and rebuild spec documentation
@dodeca-watch:
    cargo xtask dodeca watch

# Run all tests
@test:
    cargo xtask test

# Build all cells
@build:
    cargo xtask build

# Run DAW standalone cell
@run:
    cargo xtask run

# Quick development workflow: build and test
@dev:
    just build
    just test

# Clean build artifacts
@clean:
    cargo clean
    cd reference/tracey && cargo clean || true
    cd reference/dodeca && cargo clean || true
    cd reference/roam && cargo clean || true

# Full check: build, test, tracey
@check:
    just build
    just test
    just tracey

# Aliases for convenience
alias t := test
alias b := build
alias r := run
alias dc := dodeca
alias tr := tracey