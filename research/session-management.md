# Session Management — Tabs, Panes, Forking, Naming

**Source:** UX feature research, 2026-03-13

---

## Multi-Window & Session Management

### How the Best Terminals Do It

**Warp** — Windows > Tabs > Split Panes. Each pane is its own session. "Launch Configurations" let users save named layouts (windows, tabs, panes) and reopen them per-project. Session restoration saves to SQLite on quit, including the last few command/output blocks.

**WezTerm** — Windows > Tabs > Panes in a binary tree (arbitrary splits). A central `mux` crate manages all state. Supports "workspaces" (like tmux sessions). Can run headless as `wezterm-mux-server` for remote access. Session restoration via plugin, saving every 15 minutes.

**Kitty** — OS Windows > Tabs > Kitty Windows. Configurable layouts (stack, tall, fat, grid, splits). Session files define arrangements declaratively. Powerful remote control IPC for scripting.

**Zellij** — Sessions > Tabs > Panes. KDL layout files for both manual workspace definition AND session resurrection (same format — elegant). Auto-saves every 1 second. WASM plugin system.

### What This Means for quell

Phase 2 should support:
- **Tabs** for multiple AI sessions (Claude Code, Copilot, Gemini — each in its own tab)
- **Purpose-based tab naming** with color coding (the UX consensus from tmux, Warp, Kitty)
- **Split panes** within tabs (WezTerm's binary tree model is the gold standard in Rust)
- **Layout persistence** — save/restore tab arrangement on close/open
- **Named workspace profiles** — "my Claude Code project," "debugging session," etc.

Phase 1 (proxy) is single-session by design. Multi-window is a Phase 2 concern.

---

## Session Forking (Branching a Terminal with Context)

### How Claude Code Does It Today
- `/fork` branches the conversation — the fork gets a new session ID, the original continues unchanged
- `--fork-session` flag for CLI-based forking
- What's preserved: conversation history (summarized if needed), user requests, key code snippets
- What's NOT preserved: the filesystem is shared (both sessions edit the same files)
- Older tool outputs are cleared first, then the conversation is summarized if needed
- Best practice: put persistent rules in `CLAUDE.md` rather than relying on conversation history

### Cross-Tool Session Handoff
- **`cli-continues`** (open source) grabs sessions from one AI tool and hands off to another, bringing conversation history, file changes, and working state. Supports 14 CLI tools.

### What This Means for quell

Session forking at the terminal level is different from Claude Code's `/fork`:
- **Claude's fork** branches the AI conversation context
- **Terminal fork** would duplicate the terminal state — scrollback, environment, working directory

For Phase 2, we could support both:
1. **Quick-fork button/shortcut** that opens a new tab in the same directory with the same environment
2. **Integration with Claude's `/fork`** — detect when the user forks a Claude session and automatically open it in a new tab
3. **"Clone tab"** — duplicate the current tab's environment (like browser "duplicate tab")

The hard problem: scrollback context. A forked tab should probably NOT carry scrollback (it would be confusing), but it SHOULD carry the working directory, environment variables, and any terminal configuration.

---

## Terminal Naming

### Industry Patterns
- **Default:** Tabs show profile name, current working directory, or running process
- **Manual rename:** Right-click or keyboard shortcut (universal across Warp, Windows Terminal, Kitty, IDEs)
- **Warp:** Custom title + ANSI color per tab
- **Kitty:** `tab_title_template` with variables (`{title}`, `{index}`, `{layout_name}`)
- **tmux:** Named windows and sessions. Community consensus: name by **purpose** ("api-server", "database") not by process ("node", "bash")

### UX Best Practices
- Purpose-based names ("claude-refactor", "copilot-debug", "gemini-review") over process names
- Short, scannable labels — truncate long paths
- Visual differentiation: color + icon + text when many tabs are open
- Auto-name from the AI tool being run ("Claude Code", "Copilot", "Gemini") with manual override
- Show the AI tool's current state in the tab title (idle, thinking, writing code, etc.)

### Recommendation

Auto-naming strategy:
```
[Tool Icon/Color] [Tool Name] — [Project Directory] [Status]
  Claude Code — quell (thinking...)
  Copilot CLI — quell (idle)
  Gemini CLI — api-project (writing code)
```
- Tool detected from the child process command
- Project directory from CWD
- Status parsed from VT output patterns (spinner detection, prompt detection)
- Always allow manual rename override
