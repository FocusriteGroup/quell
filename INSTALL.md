# Installing quell

## What It Does

quell is a terminal proxy that fixes the scroll-jumping problem in Claude Code. When Claude streams long responses, your terminal's scroll position jumps around on every update, making output unreadable while it's still arriving. quell sits between your terminal and Claude Code, keeping your scroll position exactly where you left it.

## macOS Installation

### Quick install (recommended)

Run this in your terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/FocusriteGroup/quell/main/scripts/install.sh | sh
```

This downloads the latest release binary and puts it in `~/.local/bin`.

### Homebrew

```bash
brew install FocusriteGroup/tap/quell
```

### Build from source

Requires [Rust](https://rustup.rs/) (stable toolchain).

```bash
git clone https://github.com/FocusriteGroup/quell.git
cd quell
cargo build --release
cp target/release/quell /usr/local/bin/
```

## Windows Installation

### Download a release binary

1. Go to [GitHub Releases](https://github.com/FocusriteGroup/quell/releases).
2. Download `quell-windows-x86_64.exe`.
3. Rename it to `quell.exe` and place it in a directory on your PATH (e.g. `C:\Users\YOU\.local\bin`).
4. Open a new terminal to pick up the change.

### Build from source

Requires [Rust](https://rustup.rs/) (stable toolchain).

```bash
git clone https://github.com/FocusriteGroup/quell.git
cd quell
cargo build --release
```

The binary will be at `target\release\quell.exe`. Copy it to a directory on your PATH.

## Basic Usage

```bash
quell -- claude
```

Everything after `--` is passed to the child process. A few examples:

```bash
# Run Claude Code through quell
quell -- claude

# Pass flags to Claude Code
quell -- claude --dangerously-skip-permissions

# Enable verbose output from quell itself
quell --verbose -- claude
```

quell shows a startup banner, launches the child process behind it, and keeps your scroll position stable while output streams in.

## Making It Permanent (Aliases)

Once you know quell works, set up an alias so you can just type `claude` as normal.

### macOS / Linux

Add this to your `~/.zshrc` (or `~/.bashrc`):

```bash
alias claude='quell -- claude'
```

Then reload your shell:

```bash
source ~/.zshrc
```

### Windows (PowerShell)

Add this to your PowerShell profile (`$PROFILE`):

```powershell
function claude { quell -- claude @args }
```

Then reload:

```powershell
. $PROFILE
```

After this, just type `claude` and quell handles the rest transparently.

## Configuration (Optional)

quell works out of the box with no configuration. If you want to tune things, create a config file:

- **macOS / Linux:** `~/.config/quell/config.toml`
- **Windows:** `%APPDATA%\quell\config.toml`

```toml
render_delay_ms = 5        # Normal output coalescing (ms)
sync_delay_ms = 50         # Sync block coalescing (ms)
history_lines = 100000     # Scrollback buffer size
log_level = "info"         # trace, debug, info, warn, error
```

The defaults are fine for most users. See the [README](README.md) for the full list of options.

## Using with Handover Scripts (Optional)

> Most users can skip this section. It only applies if you have a startup script that runs before Claude Code, such as a handover context loader.

Some workflows source a shell script before launching Claude Code. You can wrap that entire sequence with quell so the scroll fix still applies.

### macOS / Linux

```bash
alias claude='quell -- bash -c "source ~/.claude-handover.sh && claude"'
```

### Windows (PowerShell)

```powershell
function claude {
    quell -- powershell -NoProfile -Command ". $HOME\.claude-handover.ps1; claude @args"
}
```

Replace the script paths with your own. The key idea is that quell wraps the whole pipeline, not just the `claude` command.

## Troubleshooting

**`quell: command not found` / `not recognized as an internal or external command`**
quell is not on your PATH. Check where you placed the binary and make sure that directory is in your PATH. On Windows, PATH entries must be directories, not files. Open a new terminal after changing PATH.

**Scroll still jumps**
Make sure you are on the latest version. Run `quell --verbose -- claude` and check the output for clues. If the issue persists, [open an issue](https://github.com/FocusriteGroup/quell/issues) with the verbose log.

**How do I update?**
Re-run the install script, re-download the release binary, or rebuild from source. The install script always fetches the latest release.

**`failed to spawn process`**
The child command (e.g. `claude`) is not installed or not on your PATH. Check with `which claude` (macOS/Linux) or `where claude` (Windows).
