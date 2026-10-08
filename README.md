# dependaconf

Smarter Dependabot grouping for JavaScript projects using npm or Yarn.

If your project has a lot of dependencies, Dependabot can become noisy fast:
large update batches, mixed concerns, and a lot of review churn. `dependaconf`
helps you cut through that by generating a cleaner, more intentional dependency
strategy from your npm or Yarn lockfile.

It groups:

- direct dependencies that share installed peer dependencies
- remaining packages by npm scope
- the result into `.github/dependabot.yml`

The outcome is simpler review flow, fewer noisy PRs, and a much easier way to
keep the project secure and up to date without drowning in update noise.

## Install

Download the matching binary for your platform below. The install commands do
not change your shell configuration; you can run the binary from its install
location, or add that location to your `PATH` in the next section.

Release binaries are available for Apple Silicon macOS, ARM64 and x86_64 Linux,
and x86_64 Windows. Intel macOS is not currently supported.

### macOS (Apple Silicon)

Install to `~/bin`:

```sh
mkdir -p "$HOME/bin"
curl -fL https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-aarch64-apple-darwin \
  -o "$HOME/bin/dependaconf"
chmod +x "$HOME/bin/dependaconf"
"$HOME/bin/dependaconf" --version
```

### Linux (x86_64 or ARM64)

Install to `~/.local/bin`. The command detects the machine architecture and
selects the matching release binary:

```sh
mkdir -p "$HOME/.local/bin"
curl -fL "https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-$(uname -m | sed 's/arm64/aarch64/')-unknown-linux-gnu" \
  -o "$HOME/.local/bin/dependaconf"
chmod +x "$HOME/.local/bin/dependaconf"
"$HOME/.local/bin/dependaconf" --version
```

### Windows (x86_64, PowerShell)

Install to your user `bin` directory:

```powershell
$bin = Join-Path $HOME 'bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
Invoke-WebRequest `
  -Uri 'https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-x86_64-pc-windows-msvc.exe' `
  -OutFile (Join-Path $bin 'dependaconf.exe')
& (Join-Path $bin 'dependaconf.exe') --version
```

### Optional: Add the install directory to `PATH`

To run `dependaconf` without typing its full path in new terminals, add the
install directory to your shell startup file.

On macOS with zsh (`~/bin`):

```sh
echo 'export PATH="$HOME/bin:$PATH"' >> "$HOME/.zshrc"
export PATH="$HOME/bin:$PATH"
dependaconf --version
```

On Linux with Bash (`~/.local/bin`):

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
export PATH="$HOME/.local/bin:$PATH"
dependaconf --version
```

On Linux with zsh (`~/.local/bin`):

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.zshrc"
export PATH="$HOME/.local/bin:$PATH"
dependaconf --version
```

On Windows, run this in PowerShell to add `~/bin` to your user `Path` and the
current window:

```powershell
$bin = Join-Path $HOME 'bin'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $bin) {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $bin } else { "$userPath;$bin" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
}
$env:Path = "$bin;$env:Path"
dependaconf.exe --version
```

To build from source, install Rust and run:

```sh
cargo build --release
```

The resulting binary is `target/release/dependaconf` (or
`target/release/dependaconf.exe` on Windows).

## Upgrade

To upgrade an existing installation, reinstall the latest release binary for
your platform using the same install steps above. If you installed the binary in
`~/bin` or `~/.local/bin`, replace the existing file with the newer one and keep
it executable.

For example, on macOS:

```sh
curl -fL https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-aarch64-apple-darwin \
  -o "$HOME/bin/dependaconf"
chmod +x "$HOME/bin/dependaconf"
```

Or on Linux:

```sh
curl -fL "https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-$(uname -m | sed 's/arm64/aarch64/')-unknown-linux-gnu" \
  -o "$HOME/.local/bin/dependaconf"
chmod +x "$HOME/.local/bin/dependaconf"
```

Or on Windows PowerShell:

```powershell
$bin = Join-Path $HOME 'bin'
Invoke-WebRequest `
  -Uri 'https://github.com/sarcastic-coder/dependaconf/releases/latest/download/dependaconf-x86_64-pc-windows-msvc.exe' `
  -OutFile (Join-Path $bin 'dependaconf.exe')
```

If you built from source, upgrade by rebuilding locally:

```sh
cargo build --release
```

## Usage

Once the binary is available, run `dependaconf` from the root of an npm project
that contains `package.json` and either `package-lock.json` or `yarn.lock`.

```sh
cd path/to/your-npm-project
dependaconf
```

Example generated config:

```yaml
version: 2
updates:
  - package-ecosystem: "npm"
    directory: "/"
    schedule:
      interval: "weekly"
    groups:
      react-ecosystem:
        patterns:
          - "react"
          - "react-dom"
          - "@types/react"
      tooling:
        patterns:
          - "@babel/*"
          - "eslint*"
```

### Lockfiles and peer dependencies

`dependaconf` supports:

- npm lockfile versions 2 and 3
- Yarn Classic v1 lockfiles
- Modern Yarn (Berry) lockfiles

If both `package-lock.json` and `yarn.lock` are present, dependaconf uses the
npm lockfile. Yarn dependencies are resolved from `package.json` against
`yarn.lock`; workspace manifests are processed separately.

Peer-based grouping depends on the metadata available in the lockfile:

- npm and Yarn Berry provide peer dependency metadata for grouping packages
  with their installed peers.
- Yarn Classic records package versions, but not peer dependency metadata.
  Classic projects can still group packages by npm scope and pair matching
  `@types` packages.

### Generated config

The tool creates `.github/dependabot.yml` and the `.github` directory if needed.
It groups dependencies by available peer metadata, then groups remaining
packages by npm scope.

### Updating an existing config

Generated groups are merged into existing entries with the same ecosystem and
directory. A generated group matches an existing group when at least half of
the smaller group's dependency patterns overlap; pattern order does not matter.

Existing wildcard patterns (such as `@apollo/*`) are considered when matching
generated dependencies. On a match, dependaconf keeps the existing group name
and patterns, then adds generated patterns that are not already covered. If
there is no dependency match, a same-named group is replaced. Other groups and
settings are preserved.

Whitespace and comments outside updated groups are preserved. Missing update
entries are added automatically.

### Workspaces

The root package and each JavaScript workspace are processed separately.
Dependencies from different directories are not grouped together.

If you want a single repository-root Dependabot entry for a monorepo, run:

```sh
dependaconf --combine-workspaces
```

This combines matching dependency groups from the root package and all
workspaces into one update entry.

To inspect how dependencies were assigned to groups, print a tree showing each
workspace, group members, and peer-dependency links:

```sh
dependaconf --debug
```

The debug report is printed to stderr; configuration generation still proceeds
as usual.

Use `dependaconf --help` to display command-line help, or `dependaconf --version`
to print the version.

## Examples

Examples are grouped by package manager:

- npm: [Apollo monorepo](examples/npm/apollo-monorepo/README.md),
  [Apollo Server](examples/npm/apollo-server/README.md),
  [Express API](examples/npm/express-api/README.md),
  [Next.js](examples/npm/nextjs/README.md),
  [React + Vite](examples/npm/react-vite/README.md), and
  [Vue + Vite](examples/npm/vue-vite/README.md).
- Yarn: [React with Yarn Classic](examples/yarn/react/README.md) and a
  [Yarn workspaces monorepo](examples/yarn/monorepo/README.md).
