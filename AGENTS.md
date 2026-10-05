# Project guidance

## Overview

`dependaconf` is a Rust CLI that analyzes JavaScript project dependencies and
generates or updates `.github/dependabot.yml` with grouped dependency update
rules. It reads npm lockfile versions 2 and 3 and Yarn Classic v1 or modern
lockfiles, including workspaces, and reports the Dependabot ecosystem as `npm`.

## Project structure

- `src/main.rs` defines the CLI and connects project detection to config writing.
- `src/ecosystems.rs` detects supported package ecosystems and coordinates
  analysis.
- `src/ecosystems/javascript.rs` handles JavaScript project analysis, selects
  the lockfile source, and groups dependencies for Dependabot's `npm`
  ecosystem.
- `src/ecosystems/javascript/npm.rs` and
  `src/ecosystems/javascript/yarn.rs` read lockfile metadata; the adjacent
  `report.rs` renders JavaScript dependency debug reports.
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
