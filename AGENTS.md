# Agent guidance

## Working rules
- Be concise, technical, and task-focused.
- Inspect relevant files and call sites before editing; expand reads only as needed.
- Make the smallest change that solves the request. Avoid unrelated refactors or generated/example-file changes unless required.
- Ask only when missing information blocks progress; otherwise proceed using reasonable assumptions and state material ones.
- Keep useful comments and documentation; avoid redundant commentary.
- Return only relevant diffs/functions when requested. Explain non-obvious decisions briefly when useful.

## Project
`dependaconf` is a Rust CLI that analyses JavaScript dependencies and creates or updates `.github/dependabot.yml` with grouped update rules. It supports npm lockfiles v2/v3 and Yarn Classic v1 and modern lockfiles, including workspaces. Dependabot's ecosystem is `npm`.

Key paths:
- `src/main.rs`: CLI entry point and config-writing flow.
- `src/ecosystems.rs`: ecosystem detection and analysis coordination.
- `src/ecosystems/javascript.rs`: JavaScript analysis and grouping.
- `src/ecosystems/javascript/{npm.rs,yarn.rs}`: lockfile metadata; `report.rs`: debug reports.
- `src/dependabot.rs`: Dependabot YAML serialization and merging.
- `tests/fixtures/`: test fixtures; `examples/`: sample projects/configurations.

## Correctness
- Preserve existing user-defined Dependabot settings when updating configuration.
- Add or update focused tests for changes to grouping, workspace handling, YAML generation, or merging.
- Run relevant tests after changes. Before finishing, run `cargo fmt --check` and appropriate tests; run `cargo build` when build validation is relevant.
- Use authoritative documentation for API/tool behaviour and focused web searches when external information is needed.
