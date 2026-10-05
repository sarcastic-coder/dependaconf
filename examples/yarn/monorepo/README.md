# Yarn workspaces monorepo example

This modern Yarn workspace example has two apps sharing a local UI package and
React dependencies. The checked-in
[`.github/dependabot.yml`](.github/dependabot.yml) is generated from the Yarn
lockfile and contains separate update entries for the root and each workspace.
Yarn projects use Dependabot's `npm` ecosystem.

From the repository root, regenerate the workspace entries with:

```sh
cd examples/yarn/monorepo
cargo run --manifest-path ../../../Cargo.toml
```

Or combine them into one root entry, with dependencies shared by both apps in a
`shared-dependencies` group:

```sh
cargo run --manifest-path ../../../Cargo.toml -- --combine-workspaces
```

No Yarn install is needed to generate the configuration.
