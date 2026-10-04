# Project guidance

## Overview

`dependaconf` is a Rust CLI that detects npm projects and generates or updates
`.github/dependabot.yml` with grouped dependency update rules. The CLI supports
npm lockfile versions 2 and 3 and npm workspaces.

## Project structure

- `src/main.rs` defines the CLI and connects project detection to config writing.
- `src/ecosystems.rs` detects supported package ecosystems.
- `src/ecosystems/npm.rs` reads npm lockfiles and groups dependencies.
- `src/dependabot.rs` serializes and merges Dependabot YAML.
- `tests/fixtures/` contains project fixtures used by tests.
- `examples/` contains sample projects and generated configurations.

## Development

- Run the test suite with `cargo test`.
- Check formatting with `cargo fmt --check`.
- Build with `cargo build`.

Add or update focused tests when changing dependency grouping, workspace
handling, YAML generation, or merging. Preserve existing user-defined
Dependabot settings when updating a config file. Avoid unrelated changes to
example projects and generated files.
