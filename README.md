# quell

[![CI](https://github.com/FocusriteGroup/quell/actions/workflows/ci.yml/badge.svg)](https://github.com/FocusriteGroup/quell/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![macOS](https://img.shields.io/badge/platform-macOS-000000?logo=apple)](https://github.com/FocusriteGroup/quell/releases)
[![Windows](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows)](https://github.com/FocusriteGroup/quell/releases)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org/)

**Terminal proxy that eliminates scroll-jumping for AI CLI tools. macOS and Windows.**

When Claude Code streams long responses, your terminal's scroll position jumps to the top of the visible output on every update — making it impossible to read anything while new content arrives. quell sits between your terminal and Claude Code, keeping your scroll position stable.

## The Problem

Claude Code streams output through VT escape sequences. On every full redraw, it emits clear-screen + cursor-home inside synchronized update blocks, and the terminal resets the scroll position to the top. This is [the #1 complaint](https://github.com/anthropics/claude-code/issues/1208) — hundreds of upvotes, hundreds of comments.

## How It Works

```
Your Terminal  <-->  quell (proxy)  <-->  PTY  <-->  AI CLI tool
```

quell intercepts the child process output via a pseudo-terminal (ConPTY on Windows, `forkpty` on macOS/Linux), processes VT escape sequences, filters dangerous sequences, and forwards clean output to your terminal. Your scroll position stays exactly where you left it.

## Features

- **Scroll stability** — eliminates scroll-jumping and scrollback accumulation; read earlier output while new content streams in
- **Shift+Enter support** — inserts newline in Claude Code via [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) (Windows Terminal 1.25+)
- **Security filtering** — blocks clipboard access (OSC 52), dangerous URL schemes (ssh://, javascript://), terminal query attacks, and C1 control characters
- **Full Unicode** — emoji, CJK, box-drawing, mathematical symbols all render correctly
- **Tool-agnostic** — built for Claude Code, works with any terminal program
- **Zero config** — just prefix your command with `quell`
- **No network, no telemetry** — the binary makes zero network connections

## Quick Start

### macOS

```bash
brew install FocusriteGroup/tap/quell
quell -- claude
```

### Windows

Download `quell.exe` from [Releases](https://github.com/FocusriteGroup/quell/releases), place it on your PATH, and run:

```bash
quell -- claude
```

See [INSTALL.md](INSTALL.md) for full installation options (curl script, build from source, aliases, configuration).

### Usage

```bash
# Run Claude Code through quell
quell -- claude

# Pass flags to Claude Code
quell -- claude --dangerously-skip-permissions

# Verbose output for troubleshooting
quell --verbose -- claude
```

### Troubleshooting

See [INSTALL.md](INSTALL.md) for troubleshooting steps. If the issue persists, [open an issue](https://github.com/FocusriteGroup/quell/issues).

## Configuration

quell works out of the box with no configuration. Optional settings can be placed in `~/.config/quell/config.toml` (macOS/Linux) or `%APPDATA%\quell\config.toml` (Windows):

```toml
render_delay_ms = 5        # Normal output coalescing (ms)
sync_delay_ms = 50         # Sync block coalescing (ms)
history_lines = 100000     # Scrollback buffer size
log_level = "info"         # trace, debug, info, warn, error
log_file = "C:\\logs\\quell.log"  # Optional — logs to stderr if omitted
```

CLI flags override config file values. See `quell --help` for all options.

## Security

AI-generated output is untrusted. quell classifies every VT escape sequence and blocks known attack vectors:

| Category | Action | Examples |
|----------|--------|----------|
| **Blocked** | Stripped entirely | Clipboard access (OSC 52), font queries (OSC 50), terminal device queries |
| **Filtered** | Sanitized before forwarding | Window titles (control chars stripped), hyperlinks (URL scheme whitelist) |
| **Allowed** | Passed through | Cursor movement, colors, screen management, sync markers |

The URL scheme whitelist allows `http`, `https`, and `file` only — blocking schemes used in real CVEs ([CVE-2023-46321](https://nvd.nist.gov/vuln/detail/CVE-2023-46321), [CVE-2023-46322](https://nvd.nist.gov/vuln/detail/CVE-2023-46322)).

See [SECURITY.md](SECURITY.md) for the full threat model.

## Requirements

- **macOS** — Apple Silicon (aarch64). Intel Macs can build from source.
- **Windows 10 1809+** (ConPTY support required)
- **Windows Terminal 1.25+** for Shift+Enter support (older terminals still work, Alt+Enter remains available)

## Known Limitations

- **Cursor-home viewport shift.** In some terminals, cursor-home sequences (`ESC[H`) during screen repaints can cause a minor viewport shift. This is caused by an [upstream Windows Terminal bug](https://github.com/microsoft/terminal/issues/14774) where `SetConsoleCursorPosition` always snaps the viewport to the cursor. Phase 2 (standalone terminal) will eliminate this entirely by controlling the rendering surface directly.
- **Emoji picker (WIN+.)** and **IME input** may not work through quell. This is a ConPTY limitation. Workaround: copy-paste emoji via Ctrl+V.

## Roadmap

- **Phase 1: CLI proxy** — scroll stability, security filtering, Shift+Enter, startup banner, friendly errors, `--verbose` diagnostics
- **Phase 2:** Standalone terminal (Tauri + xterm.js) with structured output, collapsible sections, tabs, accessibility
- **Phase 3:** Session persistence, split panes, search, community release

## License

[MIT](LICENSE)
