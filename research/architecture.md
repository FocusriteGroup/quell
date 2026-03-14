# Recommended Architecture & Implementation Roadmap

**Source:** Initial project research, 2026-03-13

---

## Overview

```
+---------------------------------------------------+
|                  Tauri Window                       |
|  +-----------------------------------------------+ |
|  |              xterm.js (WebGL)                  | |
|  |   - Renders diff output only                   | |
|  |   - Custom scroll lock UI overlay              | |
|  |   - Theme engine                               | |
|  |   - Selection / copy-paste                     | |
|  +----------------------+------------------------+ |
|                         | IPC (Tauri commands)      |
|  +----------------------+------------------------+ |
|  |           Rust Backend (proxy core)            | |
|  |                                                | |
|  |  +-------------+  +----------------------+    | |
|  |  | Sync Block  |  |   VT100 Emulator     |    | |
|  |  | Detector    |->|   (vt100 crate)      |    | |
|  |  | (memchr)    |  |   + Screen Diffing   |    | |
|  |  +-------------+  +----------+-----------+    | |
|  |                               |               | |
|  |  +----------------------+     |               | |
|  |  | History Buffer       |     |               | |
|  |  | (100K lines, filtered|     |               | |
|  |  |  for safe replay)    |     |               | |
|  |  +----------------------+     |               | |
|  |                               |               | |
|  |  +----------------------------+-----------+   | |
|  |  |        Render Coalescer                |   | |
|  |  |  5ms normal / 50ms sync / 60fps cap    |   | |
|  |  +----------------------------+-----------+   | |
|  |                               |               | |
|  |  +----------------------------+-----------+   | |
|  |  |     ConPTY Manager (portable-pty)      |   | |
|  |  |  - Input thread (stdin -> ConPTY)      |   | |
|  |  |  - Output thread (ConPTY -> proxy)     |   | |
|  |  |  - Resize handling                     |   | |
|  |  +----------------------------------------+   | |
|  +-----------------------------------------------+ |
|                     | ConPTY pipes                  |
|            +--------------------+                   |
|            | Claude Code (node) |                   |
|            +--------------------+                   |
+---------------------------------------------------+
```

## Component Details

### 1. ConPTY Manager
- Uses `portable-pty` crate for ConPTY session management
- Separate threads for input/output pipes (deadlock prevention)
- Handles resize via `ResizePseudoConsole()`
- May ship custom OpenConsole binary for consistency

### 2. Sync Block Detector
- SIMD-accelerated byte search (`memchr::memmem`) for BSU/ESU markers
- Accumulates sync block content in 1MB buffer
- Detects full-screen redraws (CLEAR_SCREEN + CURSOR_HOME)

### 3. VT100 Emulator
- `vt100::Parser` maintains virtual screen state
- ALL child output feeds through emulator (sync or not)
- `contents_diff(prev_screen)` computes minimal ANSI to update display
- Screen clone for prev/current state comparison

### 4. Render Coalescer
- 5ms delay for normal output (allows batching)
- 50ms delay inside sync blocks (more data incoming)
- 60fps cap to prevent overwhelming the frontend
- `vt_render_pending` flag tracks dirty state

### 5. History Buffer
- 100K-line circular buffer (`VecDeque<Vec<u8>>`)
- Two-layer filtering for safe replay:
  - Byte-level: strips terminal queries
  - Parse-level: whitelist-based sequence classification

### 6. xterm.js Frontend
- WebGL-accelerated rendering
- Receives only diff output (minimal data per frame)
- Custom scroll lock overlay (freeze display while generation continues)
- Theme engine (light/dark, custom colors)
- Clean copy/paste (strip unwanted formatting)
- IME input handling

## Key Features

### Scroll Lock / Pause Output
When the user scrolls up or presses a hotkey:
1. Frontend enters "scroll lock" mode
2. Display freezes at current position
3. Child output continues being processed by the VT emulator
4. A "New output available" indicator appears
5. On unlock, diff from frozen state to current state is rendered

### Lookback Mode
1. Configurable hotkey triggers lookback
2. History buffer content is replayed (filtered for safety)
3. User can scroll through full session history
4. All child output during lookback is cached and replayed on exit

### Smart Resize
1. Detect window resize
2. Call `ResizePseudoConsole()` to update ConPTY
3. Resize VT100 emulator
4. Force full render (clear prev_screen) since layout changed

---

## Implementation Roadmap

### Phase 1: Proof of Concept — Proxy-Only (Weeks 1-3)

**Goal:** A CLI proxy that runs in any Windows terminal and eliminates scroll-jumping.

- ConPTY session creation and child process spawning
- Dual-thread I/O (input + output pipes)
- Sync block detection (BSU/ESU markers)
- VT100 emulator integration (`vt100` crate)
- Differential rendering (`contents_diff()`)
- Render coalescing (5ms/50ms delays)
- Raw mode input forwarding
- Window resize handling
- Basic CLI (`quell -- claude` or auto-detect)

**Deliverable:** Single binary, runs Claude Code with zero scroll-jumping in Windows Terminal.

### Phase 2: Standalone Terminal — Tauri + xterm.js (Weeks 4-8)

**Goal:** A standalone Windows terminal application purpose-built for Claude Code.

- Tauri project scaffolding with xterm.js
- Rust <-> xterm.js IPC for terminal data
- Scroll lock mode with UI overlay
- Lookback mode with history buffer
- Theme engine (dark/light/custom)
- Clean copy/paste
- Settings UI (fonts, colors, keybindings)
- Auto-launch Claude Code on startup
- Windows installer (MSI or NSIS)

### Phase 3: Polish & Community Features (Weeks 9-12)

**Goal:** Address remaining community pain points and polish.

- IME input optimization
- Multi-session tabs
- Session persistence (reconnect to running Claude Code)
- Performance profiling and optimization
- ConPTY noise filtering
- Custom OpenConsole binary bundling
- Auto-update mechanism
- Documentation and README
- Community release

### Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| ConPTY escape sequence swallowing breaks Claude Code features | Medium | High | Ship custom OpenConsole, test extensively |
| ConPTY throughput insufficient | Low | Medium | ~2 MiB/s is 10x Claude's peak output |
| xterm.js IPC overhead causes stutter | Low | Medium | Batch IPC calls, can migrate to wgpu later |
| `vt100` crate doesn't handle all of Claude Code's sequences | Low | High | `vt100` is mature; fallback to `wezterm-term` |
| Claude Code changes rendering approach (breaking our assumptions) | Medium | Low | Our proxy is additive; worst case it's a no-op |
| Windows Terminal adds native scroll-lock | Low | Low | Our tool offers more features; complementary |
