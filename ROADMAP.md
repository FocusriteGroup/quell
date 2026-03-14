# quell — Roadmap

> **Key decisions (2026-03-13):**
> - Proxy (Phase 1) and Tauri app (Phase 2) are **two separate products** — the proxy is permanent, not a stepping stone
> - Supports all AI CLI tools (Claude Code primary, Copilot CLI, Gemini CLI) — the core is tool-agnostic
> - Tool-awareness is a pluggable layer on top, not core architecture
> - TOML config is source of truth; Phase 2 GUI reads/writes the same file
> - Phase 2 uses structured single stream with blocks (not multi-pane), with raw-mode toggle for power users
> - Phase 1 pipeline includes extension points (event hooks, multi-instance ConPTY) to avoid Phase 2 rewrites

---

> **Live-test log (2026-03-13):** Proxy tested against real Claude Code — streaming, tool use, resize, and Ctrl+C scenarios validated.

---

## Phase 1: CLI Proxy

**Goal:** Single binary that runs in any Windows terminal and eliminates scroll-jumping for all AI CLI tools. Lightweight, fast, terminal-native.

**Target personas:** Alex (power user), Jordan (mid-level dev)

### Milestone 1.1: Foundation
- [x] Project structure and build system
- [x] Structured logging (`tracing`)
- [x] Configuration (CLI args + TOML file)
- [x] Sync block detector with tests
- [x] VT100 differential renderer with tests
- [x] Line buffer (history) with tests

### Milestone 1.2: ConPTY Integration ✓
- [x] ConPTY session creation (`CreatePseudoConsole`) via direct `windows` crate bindings
- [x] Child process spawning with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`
- [x] Input pipe thread (real stdin → ConPTY)
- [x] Output pipe thread (ConPTY → channel → main thread)
- [x] Resize handling (`ResizePseudoConsole` + event-driven `WINDOW_BUFFER_SIZE_EVENT`)
- [x] `ConPtySession` designed as self-contained struct (supports multiple instances for Phase 2)

### Milestone 1.3: Proxy Loop ✓
- [x] 4-thread model: input thread, output thread, child-exit watcher, main thread
- [x] Render coalescer (5ms normal, 50ms sync, 60fps cap — all configurable) — built, deferred to Phase 2
- [x] Transparent passthrough mode: ConPTY → stdout (diff rendering deferred to Phase 2 Tauri terminal)
- [x] Sync detector + history fed for metrics (does not affect output in Phase 1)
- [x] Input forwarding with raw mode (`SetConsoleMode` with save/restore, including panic hook)
- [x] Ctrl+C forwarding (ENABLE_PROCESSED_INPUT disabled, 0x03 passed through naturally)
- [x] Graceful shutdown (child-exit watcher → ClosePseudoConsole → pipe EOF → clean exit)
- [x] Event hook system: ProxyEvent channel (sync block complete, resize, child exit)
- [x] Live-proved through 6 rounds (LP1-LP6) against Claude Code

### Milestone 1.4: History, Filtering & Sequence Security ✓
- [x] Escape filter layer 1 (byte-level `OutputFilter`): strips OSC 52 (clipboard), OSC 50 (font query), C1 control bytes (0x80-0x9F), DA/DSR/DECRQM queries, kitty keyboard queries, DCS sequences — with chunk-boundary state machine
- [x] Escape filter layer 2 (parse-level `EscapeFilter` via termwiz): whitelist-based classification for history replay — keeps SGR, cursor, edit, safe OSC; strips mode changes, device queries, mouse/keyboard protocol, window ops, DCS, Sixel
- [x] Block terminal query responses: DA, DSR, DECRQM stripped before reaching stdout
- [x] Sanitize OSC 2 (window title): control characters stripped from title text before forwarding
- [x] History accumulation from sync blocks with event type tagging
- [x] Full-redraw detection inserts boundary markers in history
- [x] History entries include metadata (`HistoryEntry` with timestamp, `HistoryEventType`) for Phase 2 structured view
- [x] `trace!` level startup warning when content-level logging is enabled
- [x] Zero-copy hot-path: `OutputFilter.filter()` returns `&[u8]` reference, no allocation per chunk

### Milestone 1.5: Live Proving
- [ ] Test with Claude Code streaming responses (primary)
- [ ] Smoke test with Copilot CLI and Gemini CLI (verify no breakage)
- [ ] Measure scroll event reduction vs. raw terminal
- [ ] Measure compression ratio (bytes in vs. bytes out)
- [ ] Test with Windows Terminal, VS Code terminal, conhost
- [ ] Long session stability (hours of use)
- [ ] Test OSC 8 passthrough behavior through ConPTY (determines 1.6 feasibility)
- [ ] Fuzz VT processing pipeline with `cargo-fuzz` (crafted escape sequences, extreme parameters)
- [ ] Verify panic recovery: malformed sequences crash emulator state, not the proxy process

### Milestone 1.6: Keyboard & Link Security
- [ ] Lightweight tool detection from child process command string + `--tool` CLI flag
- [ ] Kitty protocol negotiation with outer terminal (probe support, active enable/restore)
- [ ] Shift+Enter translation: receive Kitty-encoded `CSI 13;2 u`, translate per tool profile
  - Claude Code: `ESC + CR` (0x1b 0x0d)
  - Gemini CLI: `Ctrl+J` (0x0a)
  - Copilot CLI: literal newline
  - Fallback: Alt+Enter still works without Kitty protocol
- [ ] OSC 8 URL scheme whitelist: allow `http`, `https`, `file` — strip/neutralize others
- [ ] `warn!` log for blocked URL schemes in child output
- [ ] All standard shortcuts pass through unmodified (Ctrl+C, Ctrl+D, Ctrl+L, Ctrl+R, etc.)

### Milestone 1.7: Release Readiness
- [ ] `cargo-audit` in CI (check against RustSec Advisory Database)
- [ ] `cargo-deny` in CI (license checking, crate banning, advisory flags)
- [ ] SHA256 checksums published alongside release binaries
- [ ] Enable GitHub Private Vulnerability Reporting on the repository
- [ ] SECURITY.md finalized with vulnerability reporting instructions
- [ ] Remove project-local config search path (only `--config` flag and `%APPDATA%`) or add warning on project-local config load
- [ ] Distribution: portable .exe + scoop manifest
- [ ] Distribution: winget manifest (if scoop adoption is good)
- [ ] LICENSE file (MIT)

### Milestone 1.8: Rename to "quell" ✓

Renamed from `terminal-exploration` to `quell` across all code and docs.

**Cargo.toml:**
- [x] `name = "quell"`
- [x] `[[bin]] name = "quell"`
- [x] `[[lib]] name = "quell"`
- [x] `repository = "https://github.com/FurbySoup/quell"`
- [x] Regenerate `Cargo.lock`

**Source code (src/):**
- [x] `src/main.rs` — log messages: "quell starting", "quell shutting down"
- [x] `src/main.rs` — default log filename: `quell.log`
- [x] `src/config/mod.rs` — `#[command(name = "quell")]`
- [x] `src/config/mod.rs` — help text / about string, env var `QUELL_LOG_FILE`
- [x] `src/config/settings.rs` — config dir: `p.join("quell")`

**Tests:**
- [x] `tests/integration/main.rs` — all `use terminal_exploration::` → `use quell::`

**Documentation:**
- [x] `CLAUDE.md` — project title, architecture diagram, log paths, all references
- [x] `ROADMAP.md` — title, config examples, log paths
- [x] `SECURITY.md` — project description, config file names, verification commands, GitHub PVR URL
- [x] `research/USER-PERSONAS.md` — all references updated
- [x] `research/UX-FEATURES.md` — all references updated
- [x] `research/PRIVACY-TRUST-MODEL.md` — config file references, comparison table
- [x] `research/session-management.md`, `progress-visualization.md`, `keyboard-input.md`, `architecture.md` — all references updated

**GitHub (manual):**
- [ ] Rename repository: `FurbySoup/terminal-exploration` → `FurbySoup/quell`
- [ ] Update repo description
- [ ] Reserve `quell` on crates.io (publish placeholder or first release)

**Config paths:**
- [x] `%APPDATA%\quell\config.toml`
- [x] `logs/quell.log`

### Phase 1 Config Surface
```toml
[proxy]
render_delay_ms = 5        # Normal output coalescing
sync_render_delay_ms = 50  # Sync block coalescing
max_fps = 60               # Frame rate cap

[history]
max_lines = 100_000        # Scrollback buffer size

[tool]
# Auto-detected from child command, override here
# name = "claude"           # claude | gemini | copilot
# shift_enter = "\x1b\x0d" # Custom key translation

[links]
allowed_schemes = ["http", "https", "file"]

[logging]
level = "info"
# file = "logs/quell.log"
```

---

## Phase 2: Standalone Terminal (Tauri + xterm.js)

**Goal:** A standalone Windows terminal application with structured output, tabs, accessibility, and visual polish. Built around the Phase 1 proxy engine.

**Target personas:** Sam (newcomer), Riley (AI-native), Morgan (accessibility), plus Jordan and Alex via raw mode

### Milestone 2.1: Shell
- [ ] Tauri project scaffolding with xterm.js + WebGL renderer
- [ ] Rust backend ↔ xterm.js IPC protocol (diffs + events + status + link metadata)
- [ ] Pluggable output sink in Rust proxy (stdout for CLI, Tauri IPC for app)
- [ ] Basic terminal functionality (type, run commands, launch AI tools)
- [ ] OS dark/light mode detection + Windows high contrast awareness

### Milestone 2.2: Structured Output
- [ ] Block model: one block per conversation turn (user prompt + AI response)
- [ ] Turn boundary detection (OSC 133 where available, heuristic fallback)
- [ ] Collapsible sections within blocks (reasoning, tool calls, diffs)
- [ ] Progressive disclosure defaults: reasoning + tool calls collapsed, final response expanded
- [ ] Configurable collapse presets: "standard" (collapsed), "expanded" (everything), "minimal" (response only)
- [ ] Raw mode toggle: full unstructured terminal passthrough for power users
- [ ] Status bar (bottom, toggleable): tool name, project dir, context %, elapsed time
- [ ] Stall detection: "Still working..." after 10s silence during active operation (configurable)

### Milestone 2.3: Tabs & Sessions
- [ ] Tab support: each tab = one AI session (or shell)
- [ ] Auto tab naming: [Tool Color] Tool Name — Project Dir (Status)
- [ ] Manual tab rename (right-click or shortcut)
- [ ] Clone tab: new tab with same CWD + environment
- [ ] Close tab confirmation if session is active

### Milestone 2.4: Accessibility & Theming
- [ ] Enable xterm.js `screenReaderMode` + `AccessibilityManager`
- [ ] Buffered announcement mode: announce complete responses, not streaming fragments
- [ ] 4 bundled color schemes: Default Dark, Default Light, High Contrast, CVD-Friendly (blue/orange)
- [ ] TOML theme file format with GUI theme picker
- [ ] Font picker GUI: family, size, weight, ligature toggle
- [ ] Ctrl+/-/0 zoom with ConPTY resize propagation
- [ ] WCAG 2.1 AA compliance for all UI chrome (tabs, status bar, settings, dialogs)
- [ ] Settings panel GUI that reads/writes the same TOML config file

### Milestone 2.5: Navigation & Links
- [ ] Conversation turn navigation: Ctrl+Up/Down between user prompts
- [ ] Ctrl+F search across conversation history (block-aware)
- [ ] Auto-follow toggle (freeze scroll during review, indicator for new output)
- [ ] Ctrl+click URL opening with mismatch detection
  - Matching URLs: open immediately (standard behavior)
  - Mismatched display text vs. actual URL: show warning dialog
  - Configurable: `confirm_urls = true` for always-confirm mode
- [ ] Ctrl+click file paths: open in `$EDITOR` at file:line:column
- [ ] File existence check before wrapping paths in OSC 8

### Milestone 2.6: Polish & Distribution
- [ ] Auto-launch configured AI tool on startup
- [ ] Keybinding configuration (all navigation shortcuts configurable)
- [ ] Code signing for Windows binaries (OV certificate or SignPath.io for open source)
- [ ] Windows installer (MSI or NSIS) — signed
- [ ] Single-binary download option alongside installer
- [ ] Tauri auto-updater with cryptographic signature verification

### Milestone 2.7: Session Security (if persistence enabled)
- [ ] Session persistence defaults to OFF — opt-in only
- [ ] Session data stored in `%APPDATA%` only (never in project directory)
- [ ] Encrypt session data at rest using Windows DPAPI
- [ ] Configurable retention period (default 30 days, auto-expiry)
- [ ] "Clear all history" command that reliably deletes all session data
- [ ] PRIVACY.md documenting what's stored, where, how to delete, encryption details

---

## Phase 3: Advanced & Community

**Goal:** Session persistence, search, full accessibility polish, and community release.

**Target personas:** All — polish for every persona

### Milestone 3.1: Session Management
- [ ] Session persistence: save tab layout + CWD on close, restore on open (SQLite)
- [ ] Full session resurrection with scrollback (opt-in, configurable)
- [ ] Split panes within tabs (WezTerm binary tree model)
- [ ] Named workspace profiles (save/load project-specific layouts)

### Milestone 3.2: Search & History
- [ ] Full-text search across session history with turn-boundary awareness
- [ ] Compaction-aware history: detect compaction, load pre-compaction turns from JSONL on demand
- [ ] Persistent bookmarks that survive session close
- [ ] Annotation/notes on conversation turns

### Milestone 3.3: Accessibility & Theming Polish
- [ ] User-tested CVD color schemes (recruit CVD participants, Bloomberg-style methodology)
- [ ] Semantic announcement improvements based on screen reader user feedback
- [ ] Additional bundled themes from community contributions
- [ ] Tab color coding by AI tool

### Milestone 3.4: Platform & Release
- [ ] ConPTY noise filtering (explicit, if live testing reveals artifacts beyond diff absorption)
- [ ] Custom OpenConsole binary bundling (if Windows version fragmentation causes issues)
- [ ] IME input optimization
- [ ] Performance profiling and optimization
- [ ] Auto-update mechanism
- [ ] Documentation and user guide
- [ ] Community release on GitHub

---

## Architecture Principles

1. **Core proxy is tool-agnostic.** The VT processing pipeline (sync detection, emulation, diffing) never depends on knowing which AI tool is running. It processes standard VT100 output.

2. **Tool-awareness is a pluggable layer.** Tool profiles (Claude Code, Copilot CLI, Gemini CLI) provide optional enhancement: key translation, status parsing, tab naming. Unknown tools get generic behavior.

3. **Two products, shared engine.** The CLI proxy is a permanent, lightweight tool for power users. The Tauri app is built around the same engine for a broader audience. Neither deprecates the other.

4. **Config file is source of truth.** TOML config drives all behavior. The GUI settings panel reads/writes the same file. Power users edit directly; newcomers use the GUI.

5. **Extension over rewrite.** Phase 1 pipeline includes event hooks and multi-instance support so Phase 2 extends rather than replaces the core.

6. **Security at the proxy layer.** URL scheme whitelisting and OSC 8 filtering happen in the Rust proxy, not just the frontend. The proxy is the trust boundary between AI output and the user.

7. **Child output is untrusted.** All VT sequences from the child process are classified (allow/filter/block) before forwarding. The proxy never responds to terminal queries, never allows clipboard access, and never executes content from the child. See SECURITY.md for the full classification table.

8. **No network, no telemetry, no surprises.** The proxy makes zero network connections. Default logging captures metadata only. Session persistence is opt-in and encrypted. Users can verify all claims independently.
