# Claude Code Terminal for Windows — Research Report

**Date:** 2026-03-13
**Status:** Research Phase Complete

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Community Pain Points](#2-community-pain-points)
3. [Root Cause Analysis](#3-root-cause-analysis)
4. [Existing Solutions (Unix-Only)](#4-existing-solutions-unix-only)
5. [claude-chill Architecture Deep Dive](#5-claude-chill-architecture-deep-dive)
6. [Windows ConPTY API](#6-windows-conpty-api)
7. [DEC Mode 2026 (Synchronized Output)](#7-dec-mode-2026-synchronized-output)
8. [UI Framework Evaluation](#8-ui-framework-evaluation)
9. [Recommended Architecture](#9-recommended-architecture)
10. [Implementation Roadmap](#10-implementation-roadmap)

---

## 1. Problem Statement

Claude Code is Anthropic's CLI tool — a TUI built with React/Ink that streams AI responses in the terminal. It has a severe, well-documented scroll-jumping problem: when Claude generates output, the terminal viewport jumps to the top of the buffer, making it impossible to read responses mid-generation.

This affects every major terminal on Windows (VS Code integrated terminal, Windows Terminal) and has been reported extensively with **~1,860+ combined upvotes** across the top scroll/flicker issues — second only to the AGENTS.md feature request (3,201 upvotes).

**The problem has persisted for 9+ months** with repeated fix/regression cycles, confirming the underlying architecture (Ink library + full-screen redraws) is fundamentally at odds with smooth terminal scrolling.

**Windows has zero solutions.** All existing fixes (claude-chill, tmux configs, Ghostty) are Unix-only.

---

## 2. Community Pain Points

### Scroll/Rendering Issues (Terminal-Solvable)

| Issue | Upvotes | Description | Custom Terminal Fix? |
|-------|---------|-------------|---------------------|
| #826 | 582 | Console scrolling to top when Claude adds text | **YES** |
| #3648 | 694 | Terminal scrolling uncontrollably | **YES** |
| #1913 | 307 | Terminal flickering | **YES** |
| #769 | 280 | In-progress call causes screen flickering | **YES** |
| #1547 | 228 | IME input causes performance issues | **YES** |
| #4851 | 89 | Scrollback buffer rewind lag in tmux+VSCode | **YES** |
| #9935 | 47 | 4,000-6,700 scroll events/sec in multiplexers | **PARTIALLY** |
| #18299 | 19 | Scroll position jumps on focus change | **YES** |
| #33367 | 6 | Scroll position jumps during streaming | **YES** |

### UX Issues (Partially Terminal-Solvable)

| Issue | Upvotes | Description | Custom Terminal Fix? |
|-------|---------|-------------|---------------------|
| #2990 | 174 | Automatic light/dark theme selection | **YES** |
| #18170 | 97 | Copy/paste includes unwanted formatting | **YES** |
| #1302 | 97 | Custom terminal themes | **YES** |
| #3412 | 158 | View/edit pasted text before submission | **PARTIALLY** |

### Issues Requiring Claude Code Changes (NOT Terminal-Solvable)

| Issue | Upvotes | Description |
|-------|---------|-------------|
| #6235 | 3,201 | Support AGENTS.md |
| #16157 | 542 | Usage limit issues |
| #1455 | 297 | XDG Base Directory spec |
| #2511 | 241 | Connect to Claude projects |
| #21151 | 179 | No indication of WHICH file for READ tool |
| #8477 | 153 | Option to always show thinking |

### Key Finding

**A custom terminal can fully solve 7 of the top 9 scroll/rendering bugs and partially solve the remaining 2.** It can also address theme, copy/paste, and IME issues. The total addressable pain is ~2,500+ upvotes worth of community complaints.

---

## 3. Root Cause Analysis

### The Rendering Pipeline Problem

Claude Code uses React/Ink as its TUI framework. The rendering pipeline:

1. React constructs a scene graph of components
2. Ink lays out elements (flexbox-like)
3. Rasterizes to a 2D character grid
4. Diffs against previous frame
5. Generates ANSI escape sequences
6. Wraps in DEC 2026 synchronized output markers
7. Writes ~5,000 lines of ANSI per frame (even when only ~20 visible lines changed)

### The Numbers (from Issue #9935)

Through microsecond-precision instrumentation:

- **4,000-6,700 scroll events/second** (vs. 10-50 for vim, 100-500 for `cat`)
- **94.7%** of scrolls occur in sub-millisecond bursts
- **~189 KB/second** of ANSI codes alone
- Full-screen redraws on every streaming chunk

### Why Existing Fixes Keep Regressing

Anthropic rewrote their renderer (~85% flicker reduction) and contributes DEC 2026 patches upstream (VS Code, tmux). But the fundamental issue is architectural: React/Ink's rendering model produces full-screen redraws. Each "diff" still contains the entire visible screen because Ink re-renders the whole component tree. The ~16ms frame budget with ~5ms for React-to-ANSI conversion leaves no room for true incremental updates.

---

## 4. Existing Solutions (Unix-Only)

### claude-chill (Rust PTY Proxy)

**Architecture:** Sits between the real terminal and Claude Code via Unix PTY. Intercepts synchronized output blocks, feeds all output through an internal VT100 emulator, and sends only differential cell changes to the real terminal.

**Results:** ~100-1000x reduction in bytes sent to terminal per frame. Eliminates scroll-jumping and flicker.

**Limitation:** Unix-only (depends on `openpty()`, `poll()`, Unix signals).

### tmux-claude-code (tmux Configuration)

Optimized tmux settings for scroll handling. Linux/macOS only.

### Ghostty Terminal

Native DEC 2026 support with proper synchronized output handling. "Zero flicker." Linux/macOS only.

---

## 5. claude-chill Architecture Deep Dive

### Data Flow

```
Real Terminal ←stdin/stdout→ claude-chill Proxy ←PTY master/slave→ Claude Code
```

### Core Algorithm

1. **Sync Block Detection**: Uses SIMD-accelerated byte search (`memchr::memmem`) to find `\x1b[?2026h` (BSU) and `\x1b[?2026l` (ESU) markers in the output stream.

2. **VT100 Emulation**: ALL child output is fed to `vt100::Parser` which maintains a virtual screen buffer. Nothing from sync blocks goes directly to the real terminal.

3. **Differential Rendering**: When rendering:
   - Calls `screen().contents_diff(prev_screen)` — only changed cells are emitted
   - Wraps diff output in its own sync markers for atomic display
   - Clones current screen as new `prev_screen`

4. **Render Coalescing**: 5ms delay for normal output, 50ms for sync blocks. Batches rapid updates into single frames.

5. **History Management**: Maintains a 100K-line circular buffer with two-layer filtering:
   - Byte-level filter strips terminal query sequences
   - Parsed-level whitelist filter classifies every escape sequence as safe/unsafe for replay

### Key Source Files

| File | Purpose | Lines |
|------|---------|-------|
| `proxy.rs` | Core proxy loop, VT diffing, sync detection | ~700 |
| `escape_filter.rs` | Byte-level terminal query stripping | ~200 |
| `history_filter.rs` | Whitelist-based safe history replay | ~300 |
| `line_buffer.rs` | Circular history buffer | ~100 |
| `key_parser.rs` | Hotkey detection (legacy + Kitty protocol) | ~150 |
| `redraw_throttler.rs` | Time-based render coalescing | ~50 |

### Rust Crate Dependencies

| Crate | Purpose | Windows-Compatible? |
|-------|---------|-------------------|
| `vt100` | VT100 terminal emulator (screen state, diffing) | **YES** (pure Rust) |
| `termwiz` | Escape sequence parser | **YES** (pure Rust) |
| `memchr` | SIMD byte search for sync markers | **YES** |
| `nix` | Unix PTY, signals, poll | **NO** (Unix-only) |
| `libc` | Low-level syscalls | **NO** (Unix-only) |
| `clap` | CLI args | **YES** |
| `serde`/`toml` | Config file | **YES** |

**Key insight:** The core algorithm (VT emulation + diff + sync detection) is 100% portable. Only the PTY/IO/signal layer (~15% of code) needs Windows-specific replacement.

### Platform Mapping for Windows Port

| Unix (claude-chill) | Windows Equivalent |
|---|---|
| `openpty()` | `CreatePseudoConsole()` |
| `poll()` on FDs | `WaitForMultipleObjects()` or async I/O |
| `SIGWINCH` | `ResizePseudoConsole()` + console event monitoring |
| `SIGINT`/`SIGTERM` via `kill()` | `GenerateConsoleCtrlEvent()` |
| `cfmakeraw()` | `SetConsoleMode()` with `ENABLE_VIRTUAL_TERMINAL_INPUT` |
| `setsid()`/`TIOCSCTTY` | ConPTY handles implicitly |
| Non-blocking FD via `O_NONBLOCK` | Overlapped I/O or separate threads |

---

## 6. Windows ConPTY API

### Architecture

ConPTY (Windows 10 1809+) provides a pipe-based pseudoconsole. Unlike Unix PTYs (transparent byte pipes), ConPTY maintains an **internal screen buffer** and re-encodes all output through a parse-render-reparse cycle.

```
Real Terminal (Windows Terminal / conhost)
    ↕
[Proxy Process] — reads/writes VT sequences via pipes
    ↕
ConPTY (internal conhost instance with screen buffer)
    ↕
Claude Code (node.exe child process)
```

### Core API (3 functions)

```c
CreatePseudoConsole(COORD size, HANDLE hInput, HANDLE hOutput, DWORD dwFlags, HPCON* phPC)
ResizePseudoConsole(HPCON hPC, COORD size)
ClosePseudoConsole(HPCON hPC)
```

### Setup Sequence

1. `CreatePipe()` × 2 (input pair + output pair)
2. `CreatePseudoConsole(size, inputRead, outputWrite, 0, &hPC)`
3. `InitializeProcThreadAttributeList()` + `UpdateProcThreadAttribute(PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE)`
4. `CreateProcessW()` with `EXTENDED_STARTUPINFO_PRESENT`
5. Close child-side pipe ends
6. **Separate threads** for reading output pipe and writing input pipe (mandatory — same-thread causes deadlocks)

### Performance

| Platform | Throughput |
|----------|-----------|
| Linux native PTY | ~23 MiB/s |
| WinPTY (legacy) | ~7.7-12.5 MiB/s |
| **ConPTY** | **~1.8-2.2 MiB/s** |

ConPTY is 6-12x slower than Unix PTY due to the parse-render-reparse cycle. However, for Claude Code's text output (~189 KB/s peak), this is more than adequate.

### Known Issues Affecting Our Proxy

1. **Escape sequence swallowing**: Unrecognized DCS sequences are dropped entirely
2. **Out-of-order delivery**: OSC sequences can arrive interleaved with text incorrectly
3. **Color mangling**: `ESC[39m`/`ESC[49m` → `ESC[m` (resets bold, underline too)
4. **Spurious output**: ConPTY generates its own cursor/title sequences
5. **Growing cursor gap**: Cursor positioning can drift over long sessions

### Mitigation Strategies

- **Ship custom OpenConsole binary** (like WezTerm does) to avoid OS-version dependencies
- **Filter ConPTY noise** in the proxy layer before VT diffing
- **Consider ConPTY bypass** for WSL sessions (pipe directly to Linux PTY)
- **Use `portable-pty`** crate which includes ConPTY quirk workarounds

### Rust Crate Ecosystem

| Crate | Description | Recommendation |
|-------|-------------|----------------|
| `portable-pty` | Cross-platform PTY (Unix + ConPTY). Battle-tested in WezTerm. | **Primary choice** |
| `conpty` | Lightweight ConPTY wrapper | Backup option |
| `vte` | VT sequence parser | For sequence-level interception |
| `vt100` | Full VT100 emulator with screen diffing | **Core of proxy** |
| `termwiz` | Escape sequence parser + terminal abstraction | For history filtering |

---

## 7. DEC Mode 2026 (Synchronized Output)

### Protocol

| Operation | Sequence | Description |
|-----------|----------|-------------|
| Begin Synchronized Update (BSU) | `\x1b[?2026h` | Terminal batches updates, holds previous frame |
| End Synchronized Update (ESU) | `\x1b[?2026l` | Terminal atomically renders new frame |
| Query support | `\x1b[?2026$p` | DECRPM query for terminal capability |

### How It Works

Conceptually identical to double-buffering in graphics:
1. App emits BSU → terminal holds current display
2. App writes all content (cursor moves, text, colors)
3. App emits ESU → terminal atomically swaps to new frame

### Terminal Support Status

| Terminal | DEC 2026 Support |
|----------|-----------------|
| Windows Terminal | **YES (since v1.24, stable)** |
| Ghostty | YES |
| WezTerm | YES |
| iTerm2 | YES |
| Kitty | YES |
| Alacritty | YES |
| VS Code terminal | In progress (Anthropic contributing patches) |
| tmux | YES (Anthropic contributed patches) |

### Relevance to Our Project

Claude Code already wraps output in BSU/ESU markers. The problem isn't missing sync support — it's that each sync block contains a **full-screen redraw** (~5,000 lines). Even with atomic rendering, the terminal still processes thousands of lines per frame, causing:
- Scrollback buffer flooding
- High CPU usage parsing ANSI sequences
- Memory pressure from buffered content

**Our proxy intercepts these sync blocks and replaces them with minimal diffs** — the same approach as claude-chill.

---

## 8. UI Framework Evaluation

### Decision: Proxy-Only vs. Full Terminal Emulator

There are two architectural paths:

**Path A: Proxy-Only (Terminal-Agnostic)**
Run as a PTY proxy inside the user's existing terminal. Like claude-chill but for Windows.
- Pros: Works with any terminal, simple, focused
- Cons: Limited to what the host terminal supports, no custom UI features

**Path B: Full Terminal Emulator (Standalone App)**
Build a custom terminal with integrated proxy logic.
- Pros: Full rendering control, custom features (scroll lock, themes, etc.)
- Cons: Much more work, must compete with established terminals

**Recommendation: Both.** Start with Path A (proxy) for immediate value, then build Path B using the proxy as the backend.

### Framework Comparison for Path B

| Criterion | Tauri+xterm.js | Rust+wgpu | Fork WezTerm | Electron | Win32/D3D |
|-----------|---------------|-----------|---------------|----------|-----------|
| Rendering perf | Good | Excellent | Excellent | Good | Excellent |
| VT handling | Excellent | Good | Excellent | Excellent | Excellent |
| Binary size | ~5-10MB | ~5-15MB | ~10-20MB | ~150MB+ | ~5-10MB |
| Dev effort | Low-Medium | High | Medium | Low | Very High |
| Windows feel | Good | Moderate | Good | Poor | Perfect |
| Installation | Simple (WebView2 preinstalled) | Simple | Simple | Heavy | N/A |

### Recommendation

**Phase 1: Tauri + xterm.js** for the standalone terminal.

Rationale:
- **Fastest to production** — proof-of-concept exists (tauri-terminal)
- **xterm.js is battle-tested** — powers VS Code's terminal, handles VT comprehensively
- **Small binary** (~5-10MB) with native feel via WebView2
- **Rust backend** for all proxy/interception logic (no JavaScript in the hot path)
- **WebView2 is pre-installed** on Windows 10/11 — zero runtime dependencies
- **xterm.js WebGL renderer** provides GPU-accelerated text rendering

The architecture cleanly separates concerns:
- **Rust layer**: ConPTY management, VT interception, sync block detection, diff computation
- **WebView layer**: xterm.js renders the final diff output, handles scroll/selection/themes

If xterm.js IPC overhead becomes a bottleneck (unlikely for Claude Code's output volume), we can later migrate to Rust+wgpu or fork WezTerm.

---

## 9. Recommended Architecture

### Overview

```
┌─────────────────────────────────────────────────┐
│                  Tauri Window                     │
│  ┌─────────────────────────────────────────────┐ │
│  │              xterm.js (WebGL)                │ │
│  │   - Renders diff output only                 │ │
│  │   - Custom scroll lock UI overlay            │ │
│  │   - Theme engine                             │ │
│  │   - Selection / copy-paste                   │ │
│  └──────────────────┬──────────────────────────┘ │
│                     │ IPC (Tauri commands)        │
│  ┌──────────────────┴──────────────────────────┐ │
│  │           Rust Backend (proxy core)          │ │
│  │                                              │ │
│  │  ┌─────────────┐  ┌──────────────────────┐  │ │
│  │  │ Sync Block  │  │   VT100 Emulator     │  │ │
│  │  │ Detector    │→ │   (vt100 crate)      │  │ │
│  │  │ (memchr)    │  │   + Screen Diffing   │  │ │
│  │  └─────────────┘  └──────────┬───────────┘  │ │
│  │                              │               │ │
│  │  ┌──────────────────────┐   │               │ │
│  │  │ History Buffer       │   │               │ │
│  │  │ (100K lines, filtered│   │               │ │
│  │  │  for safe replay)    │   │               │ │
│  │  └──────────────────────┘   │               │ │
│  │                              │               │ │
│  │  ┌──────────────────────────┴─────────────┐ │ │
│  │  │        Render Coalescer                 │ │ │
│  │  │  5ms normal / 50ms sync / 60fps cap     │ │ │
│  │  └──────────────────────────┬─────────────┘ │ │
│  │                              │               │ │
│  │  ┌──────────────────────────┴─────────────┐ │ │
│  │  │     ConPTY Manager (portable-pty)       │ │ │
│  │  │  - Input thread (stdin → ConPTY)        │ │ │
│  │  │  - Output thread (ConPTY → proxy)       │ │ │
│  │  │  - Resize handling                      │ │ │
│  │  └────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────┘ │
│                     ↕ ConPTY pipes                │
│            ┌────────────────────┐                 │
│            │ Claude Code (node) │                 │
│            └────────────────────┘                 │
└─────────────────────────────────────────────────┘
```

### Component Details

#### 1. ConPTY Manager
- Uses `portable-pty` crate for ConPTY session management
- Separate threads for input/output pipes (deadlock prevention)
- Handles resize via `ResizePseudoConsole()`
- May ship custom OpenConsole binary for consistency

#### 2. Sync Block Detector
- SIMD-accelerated byte search (`memchr::memmem`) for BSU/ESU markers
- Accumulates sync block content in 1MB buffer
- Detects full-screen redraws (CLEAR_SCREEN + CURSOR_HOME)

#### 3. VT100 Emulator
- `vt100::Parser` maintains virtual screen state
- ALL child output feeds through emulator (sync or not)
- `contents_diff(prev_screen)` computes minimal ANSI to update display
- Screen clone for prev/current state comparison

#### 4. Render Coalescer
- 5ms delay for normal output (allows batching)
- 50ms delay inside sync blocks (more data incoming)
- 60fps cap to prevent overwhelming the frontend
- `vt_render_pending` flag tracks dirty state

#### 5. History Buffer
- 100K-line circular buffer (`VecDeque<Vec<u8>>`)
- Two-layer filtering for safe replay:
  - Byte-level: strips terminal queries
  - Parse-level: whitelist-based sequence classification

#### 6. xterm.js Frontend
- WebGL-accelerated rendering
- Receives only diff output (minimal data per frame)
- Custom scroll lock overlay (freeze display while generation continues)
- Theme engine (light/dark, custom colors)
- Clean copy/paste (strip unwanted formatting)
- IME input handling

### Key Features

#### Scroll Lock / Pause Output
When the user scrolls up or presses a hotkey:
1. Frontend enters "scroll lock" mode
2. Display freezes at current position
3. Child output continues being processed by the VT emulator
4. A "New output available ↓" indicator appears
5. On unlock, diff from frozen state to current state is rendered

#### Lookback Mode
Like claude-chill's approach:
1. Configurable hotkey triggers lookback
2. History buffer content is replayed (filtered for safety)
3. User can scroll through full session history
4. All child output during lookback is cached and replayed on exit

#### Smart Resize
1. Detect window resize
2. Call `ResizePseudoConsole()` to update ConPTY
3. Resize VT100 emulator
4. Force full render (clear prev_screen) since layout changed

---

## 10. Implementation Roadmap

### Phase 1: Proof of Concept — Proxy-Only (Weeks 1-3)

**Goal:** A CLI proxy (`claude-terminal.exe`) that runs in any Windows terminal and eliminates scroll-jumping.

- [ ] ConPTY session creation and child process spawning
- [ ] Dual-thread I/O (input + output pipes)
- [ ] Sync block detection (BSU/ESU markers)
- [ ] VT100 emulator integration (`vt100` crate)
- [ ] Differential rendering (`contents_diff()`)
- [ ] Render coalescing (5ms/50ms delays)
- [ ] Raw mode input forwarding
- [ ] Window resize handling
- [ ] Basic CLI (`claude-terminal -- claude` or auto-detect)

**Deliverable:** Single binary, runs Claude Code with zero scroll-jumping in Windows Terminal.

### Phase 2: Standalone Terminal — Tauri + xterm.js (Weeks 4-8)

**Goal:** A standalone Windows terminal application purpose-built for Claude Code.

- [ ] Tauri project scaffolding with xterm.js
- [ ] Rust ↔ xterm.js IPC for terminal data
- [ ] Scroll lock mode with UI overlay
- [ ] Lookback mode with history buffer
- [ ] Theme engine (dark/light/custom)
- [ ] Clean copy/paste
- [ ] Settings UI (fonts, colors, keybindings)
- [ ] Auto-launch Claude Code on startup
- [ ] Windows installer (MSI or NSIS)

**Deliverable:** Installable Windows app that launches Claude Code in an optimized terminal.

### Phase 3: Polish & Community Features (Weeks 9-12)

**Goal:** Address remaining community pain points and polish.

- [ ] IME input optimization
- [ ] Multi-session tabs
- [ ] Session persistence (reconnect to running Claude Code)
- [ ] Performance profiling and optimization
- [ ] ConPTY noise filtering
- [ ] Custom OpenConsole binary bundling
- [ ] Auto-update mechanism
- [ ] Documentation and README
- [ ] Community release

### Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ConPTY escape sequence swallowing breaks Claude Code features | Medium | High | Ship custom OpenConsole, test extensively |
| ConPTY throughput insufficient | Low | Medium | ~2 MiB/s is 10x Claude's peak output |
| xterm.js IPC overhead causes stutter | Low | Medium | Batch IPC calls, can migrate to wgpu later |
| `vt100` crate doesn't handle all of Claude Code's sequences | Low | High | `vt100` is mature; fallback to `wezterm-term` |
| Claude Code changes rendering approach (breaking our assumptions) | Medium | Low | Our proxy is additive; worst case it's a no-op |
| Windows Terminal adds native scroll-lock | Low | Low | Our tool offers more features; complementary |

---

## Appendix: Source References

### GitHub Issues Analyzed
- anthropics/claude-code: #826, #769, #1302, #1413, #1547, #1913, #2990, #3412, #3648, #4851, #6235, #8477, #9935, #10656, #16157, #18170, #18299, #21151, #25682, #33367
- microsoft/vscode: #224750, #249058

### Technical References
- [Windows ConPTY Blog Post](https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/)
- [ConPTY API Documentation](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
- [DEC Mode 2026 Spec](https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036)
- [Windows Terminal DEC 2026 PR #18826](https://github.com/microsoft/terminal/pull/18826)
- [claude-chill Repository](https://github.com/davidbeesley/claude-chill)
- [claude-chill HN Discussion](https://news.ycombinator.com/item?id=46699072)
- [ConPTY Performance Benchmarks](https://kichwacoders.com/2021/05/24/conpty-performance-in-eclipse-terminal/)
- [Warp: Building on Windows](https://www.warp.dev/blog/building-warp-on-windows)
- [portable-pty crate](https://docs.rs/portable-pty)
- [vt100 crate](https://crates.io/crates/vt100)
- [tauri-terminal PoC](https://github.com/marc2332/tauri-terminal)
- [xterm.js](https://github.com/xtermjs/xterm.js/)
