# dependaconf

`dependaconf` generates a grouped Dependabot configuration for an npm project.
It uses the project's `package-lock.json` to group direct dependencies that
share installed peer dependencies, and groups remaining scoped dependencies
together.

## Install

Download and install the matching binary below. The install commands do not
change your shell configuration: you can run the binary from its install
location, or optionally add that location to your `PATH` in the next section.
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

To run `dependaconf` without typing its full path in new terminals, append the
PATH export for your install directory to your shell's startup file. Run the
block for your platform and shell.

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

On Windows, run this in PowerShell to add `~/bin` to your user `Path` and to
the current window:

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

## Usage

With the binary on your `PATH`, run `dependaconf` from the root of an npm
project containing both `package.json` and `package-lock.json`. Or run the
binary by its full path if you did not add it to `PATH`.

```sh
cd path/to/your-npm-project
dependaconf
```

The lockfile must use version 2 or 3. The program creates
`.github/dependabot.yml`, creating the `.github` directory if needed. Running
it again replaces that file. The generated configuration enables weekly npm
updates and groups dependencies by installed peer dependencies and, for
remaining packages, npm scope.
For npm workspaces, direct dependencies from the root and each workspace package
are considered separately. Dependabot update entries and groups are scoped to
each package directory so dependencies from different workspaces are not grouped
together.

For a monorepo where you want one Dependabot update entry at the repository
root, run:

```sh
dependaconf --combine-workspaces
```

This combines matching dependency groups across the root package and all npm
workspaces into that single entry.

Use `dependaconf --help` to display command-line help, or `dependaconf --version`
to print the version.
