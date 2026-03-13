# UX Feature Research

**Date:** 2026-03-13
**Scope:** Research for terminal-exploration features across all AI CLI tools (Claude Code, Copilot CLI, Gemini CLI)

---

## 1. Multi-Window & Session Management

### How the Best Terminals Do It

**Warp** — Windows > Tabs > Split Panes. Each pane is its own session. "Launch Configurations" let users save named layouts (windows, tabs, panes) and reopen them per-project. Session restoration saves to SQLite on quit, including the last few command/output blocks.

**WezTerm** — Windows > Tabs > Panes in a binary tree (arbitrary splits). A central `mux` crate manages all state. Supports "workspaces" (like tmux sessions). Can run headless as `wezterm-mux-server` for remote access. Session restoration via plugin, saving every 15 minutes.

**Kitty** — OS Windows > Tabs > Kitty Windows. Configurable layouts (stack, tall, fat, grid, splits). Session files define arrangements declaratively. Powerful remote control IPC for scripting.

**Zellij** — Sessions > Tabs > Panes. KDL layout files for both manual workspace definition AND session resurrection (same format — elegant). Auto-saves every 1 second. WASM plugin system.

### What This Means for terminal-exploration

Phase 2 should support:
- **Tabs** for multiple AI sessions (Claude Code, Copilot, Gemini — each in its own tab)
- **Purpose-based tab naming** with color coding (the UX consensus from tmux, Warp, Kitty)
- **Split panes** within tabs (WezTerm's binary tree model is the gold standard in Rust)
- **Layout persistence** — save/restore tab arrangement on close/open
- **Named workspace profiles** — "my Claude Code project," "debugging session," etc.

Phase 1 (proxy) is single-session by design. Multi-window is a Phase 2 concern.

---

## 2. Session Forking (Branching a Terminal with Context)

### How Claude Code Does It Today
- `/fork` branches the conversation — the fork gets a new session ID, the original continues unchanged
- `--fork-session` flag for CLI-based forking
- What's preserved: conversation history (summarized if needed), user requests, key code snippets
- What's NOT preserved: the filesystem is shared (both sessions edit the same files)
- Older tool outputs are cleared first, then the conversation is summarized if needed
- Best practice: put persistent rules in `CLAUDE.md` rather than relying on conversation history

### Cross-Tool Session Handoff
- **`cli-continues`** (open source) grabs sessions from one AI tool and hands off to another, bringing conversation history, file changes, and working state. Supports 14 CLI tools.

### What This Means for terminal-exploration

Session forking at the terminal level is different from Claude Code's `/fork`:
- **Claude's fork** branches the AI conversation context
- **Terminal fork** would duplicate the terminal state — scrollback, environment, working directory

For Phase 2, we could support both:
1. **Quick-fork button/shortcut** that opens a new tab in the same directory with the same environment
2. **Integration with Claude's `/fork`** — detect when the user forks a Claude session and automatically open it in a new tab
3. **"Clone tab"** — duplicate the current tab's environment (like browser "duplicate tab")

The hard problem: scrollback context. A forked tab should probably NOT carry scrollback (it would be confusing), but it SHOULD carry the working directory, environment variables, and any terminal configuration.

---

## 3. Terminal Naming

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

### Recommendation for terminal-exploration

Auto-naming strategy:
```
[Tool Icon/Color] [Tool Name] — [Project Directory] [Status]
  🟠 Claude Code — terminal-exploration (thinking...)
  🟢 Copilot CLI — terminal-exploration (idle)
  🔵 Gemini CLI — api-project (writing code)
```
- Tool detected from the child process command
- Project directory from CWD
- Status parsed from VT output patterns (spinner detection, prompt detection)
- Always allow manual rename override

---

## 4. Accessibility — Font, Style, Color Customization

### What Modern Terminals Expose

| Feature | Ghostty | WezTerm | Kitty | Windows Terminal | Warp |
|---------|---------|---------|-------|-----------------|------|
| Font family | Config file | Lua script | Config file | JSON | GUI picker |
| Font size | Config file | Lua script | Config file | JSON | GUI slider |
| Font weight | Config file | Lua script | Config file | JSON (stroke weight) | — |
| Ligatures | `font-features` | Per-font harfbuzz | Config | — | Toggle |
| Fallback fonts | Multiple entries | `font_with_fallback()` | — | Cascading | — |
| Color scheme | Theme files | Lua / built-in | Theme support | Named scheme objects | YAML + AI-generated |
| High contrast | OS-aware | OS-aware | — | Built-in schemes | OS sync |
| Dynamic zoom | Ctrl+/-/0 | Ctrl+/-/0 | Ctrl+/-/0 | Ctrl+/-/0 | Ctrl+/-/0 |

### Accessibility Standards (WCAG 2.1 AA)
- **Contrast ratios:** 4.5:1 for normal text (<18pt), 3:1 for large text (18pt+ or 14pt bold)
- **Never rely on color alone** — use shape, text labels, or patterns as secondary indicators
- **Keyboard navigation:** Every interactive element operable via keyboard
- 95.9% of top million websites fail basic WCAG 2.2 — the bar is low, we should clear it easily

### Color Blindness (CVD) Support
- Bloomberg Terminal leads here: dedicated deuteranopia and protanomaly schemes, user-tested with CVD participants
- Key principle: **avoid red/green adjacency** (most common failure) and always pair color with secondary cues
- Dark backgrounds amplify CVD difficulty — darker colors on dark backgrounds are hardest to distinguish
- **vim-dichromatic** uses only blue and orange hues — distinguishable across all common CVD types

### Screen Reader Support
- **xterm.js** (our Phase 2 frontend) has a built-in `AccessibilityManager` that creates an off-screen DOM tree mirroring terminal content. Supports NVDA, JAWS, ChromeVox via `aria-posinset`/`aria-setsize` and an assertive live region.
- xterm.js screen reader mode queues keystrokes and matches them against output to avoid double-announcing typed characters

### Recommendation for terminal-exploration

**Phase 1 (proxy):** Delegates to host terminal. Correctly forward resize events when user zooms.

**Phase 2 (Tauri + xterm.js):**
- Enable xterm.js `screenReaderMode` — it's built-in, just needs activation
- Implement Ctrl+/-/0 zoom with ConPTY resize propagation
- Respect OS dark/light mode and Windows high contrast themes
- Ship 4+ color schemes: Default Dark, Default Light, High Contrast, CVD-Friendly (blue/orange palette)
- WCAG 2.1 AA compliance for all UI chrome (not just terminal content)
- Font picker: family, size, weight, ligature toggle (GUI, not config file)
- Theme format: TOML (consistent with project's existing config approach)

---

## 5. Progress Visualization

### How AI CLI Tools Show Progress Today

**Claude Code:** Customizable spinner verbs ("Thinking...", "Working...") on a 50ms animation loop. Shows effort level, token count, tool uses, and duration in results.

**Copilot CLI:** Loading spinner in diff mode. Streaming response size counter during tool calls. Auto-adapting status line for narrow terminals.

**Gemini CLI:** Pseudo-terminal snapshots streamed in real-time. JSONL event streaming for headless monitoring.

**Codex CLI:** Alternate screen buffer (no scrollback by design). Handles scrolling internally via mouse tracking.

### Progress Indicator Patterns

| Type | When to Use | Example |
|------|------------|---------|
| Bounded progress bar | Known-length operations (file download, batch processing) | `[████████░░] 80% 4/5 files (ETA 3s)` |
| Spinner + elapsed | Unknown-length operations (AI thinking, tool execution) | `⠹ Thinking... (12s)` |
| Streaming counter | Active streaming output (AI response generation) | `⠹ Writing... 2,847 tokens (8s)` |
| Status line | Persistent state info | `Claude Code · terminal-exploration · 45% context` |

### Timing Guidelines
- Spinner frame rate: **80-130ms** per frame (below 50ms = jittery, above 300ms = sluggish)
- Always show **elapsed time** — it anchors the user's sense of progress
- For streaming AI responses: show **token/byte counter** alongside spinner
- Use `indicatif` crate (Rust) for thread-safe progress bars with templates

### Recommendation for terminal-exploration

**Phase 1 (proxy):** The proxy passes through whatever progress indicators the AI tool renders. No additional progress UI in proxy mode.

**Phase 2 (Tauri):**
- **Status bar** at bottom: tool name, project directory, context usage %, elapsed time for current operation
- **Turn-level progress:** When AI is responding, show spinner + elapsed + token counter in the status bar
- **Tool call summary:** Collapse tool call details by default, show "Read 3 files, edited 1 file (4.2s)" as a summary line that expands on click
- **Stall detection:** If no new output for 10+ seconds during an active operation, show "Still working..." indicator to distinguish stall from hang

---

## 6. Milestone / User Prompt Jump-To

### Existing Navigation Patterns

**Warp Blocks** — Each command + output is a selectable "block." Cmd+Up/Down jumps between blocks. Blocks can be bookmarked. Cmd+F searches across blocks.

**iTerm2 Marks** — Shell integration auto-places a mark at each command prompt (blue triangle in margin). Cmd+Shift+Up/Down jumps between marks. Supports annotations (notes attached to text).

**tmux Copy Mode** — Enter copy mode, then use Page Up/Down, g/G (top/bottom), / (search). Vi-style navigation.

### The Compaction Problem

Claude Code's `/compact` summarizes conversation history into a condensed form. After compaction:
- Original messages no longer exist in the active context
- The conversation has a single summary block replacing many messages
- Raw history is still in `~/.claude/history.jsonl`

**Implication:** Jump-to navigation must work at two levels:
1. **Active conversation turns** — the messages currently in context
2. **Full session history** — the raw JSONL log, including pre-compaction messages

### Recommendation for terminal-exploration

**Phase 2 concept: Conversation Turn Navigation**

The proxy already processes all VT output through a VT100 emulator. We can detect **turn boundaries** by recognizing patterns in the output:

- **User prompt marker:** The input prompt line (detectable by prompt patterns or cursor position reset)
- **AI response start:** First output after prompt submission
- **AI response end:** Return to input prompt state
- **Tool call boundaries:** Detectable from output patterns (indented blocks, status lines)

Navigation shortcuts (Warp-inspired):
```
Ctrl+Up     — Jump to previous user prompt (milestone)
Ctrl+Down   — Jump to next user prompt
Ctrl+Shift+Up   — Jump to previous AI response start
Ctrl+Shift+Down — Jump to next AI response start
Ctrl+F      — Search across conversation history
Ctrl+G      — Jump to turn by number ("Turn 5", "Turn 12")
```

**Turn index sidebar** (Phase 2 Tauri):
```
┌─ Turn 1 ─────────────────────────────────┐
│ You: "Create a React component for..."    │
│ Claude: [expanded/collapsed]              │
├─ Turn 2 ─────────────────────────────────┤
│ You: "Add error handling to..."           │
│ Claude: [expanded/collapsed]              │
├─ ⚡ Compacted ────────────────────────────┤
│ Summary: "Built React component with..."  │
├─ Turn 8 ─────────────────────────────────┤
│ You: "Now add tests"                      │
│ Claude: [expanded/collapsed]              │
└───────────────────────────────────────────┘
```

The compaction boundary is explicitly shown. Pre-compaction turns can be loaded from the JSONL history file on demand but are visually distinct (grayed out, marked as "from history").

---

## 7. Keyboard Input — The Shift+Enter Problem

### The Core Issue

Since 1978, terminals have encoded Enter as ASCII 13 (carriage return). **Shift+Enter sends the exact same byte.** The terminal application literally cannot distinguish them. This is why Claude Code uses Alt+Enter (which sends ESC + CR — distinguishable) as its default newline shortcut.

### What Users Expect

Every modern app uses **Enter = submit, Shift+Enter = newline:**

| App | Enter | Shift+Enter |
|-----|-------|-------------|
| Slack | Send | New line |
| Discord | Send | New line |
| ChatGPT web | Send | New line |
| VS Code chat | Send | New line |
| WhatsApp/Telegram | Send | New line |

Claude Code's Alt+Enter violates this universal convention. It's the source of **at least 10 GitHub issues** including #1259 ("Support Shift+Enter for multiline input — industry standard," opened May 2025), regressions in #31734, #31904, and terminal-specific failures in #9321, #22719, #25057.

### How Claude Code Handles It Today
- **Enter:** Submit prompt
- **Alt+Enter / Option+Enter:** Insert newline (universal fallback)
- **Backslash + Enter:** Insert newline (workaround)
- **Shift+Enter:** Works ONLY if terminal supports Kitty protocol or has been configured via `/terminal-setup`
- `/terminal-setup` auto-configures Shift+Enter for VS Code, iTerm2, WezTerm

### The Kitty Keyboard Protocol — The Fix

Kitty protocol solves the 48-year limitation by encoding modifier keys explicitly:
- Enter → `CSI 13 u`
- Shift+Enter → `CSI 13;2 u` (now distinct!)
- Supports progressive enhancement levels

**Terminal support:**
| Terminal | Kitty Protocol |
|----------|---------------|
| Kitty | Full |
| WezTerm | Full |
| iTerm2 | Yes |
| Ghostty | Yes |
| **Windows Terminal** | **Added in Preview 1.25 (March 2026)** |
| Alacritty | Partial |
| macOS Terminal.app | No |

**Critical detail:** ConPTY does NOT pass Kitty protocol sequences through. It has its own "win32-input-mode" that encodes full Win32 key events as VT sequences. The proxy sits between the outer terminal and ConPTY, so it could **translate between protocols.**

### Other AI Tool Approaches
- **Gemini CLI:** Ctrl+J for newline (non-standard but distinct at byte level)
- **Copilot CLI:** No dedicated newline shortcut (paste multi-line only)
- **Warp:** Ctrl+R for newline (breaks reverse-search convention)

### Standard Shortcuts That Must Not Break

| Shortcut | Standard Action | Notes |
|----------|----------------|-------|
| Ctrl+C | Interrupt/cancel | Must pass through or handle for AI cancellation |
| Ctrl+D | EOF / exit | Claude Code uses this to exit |
| Ctrl+L | Clear screen | All three AI tools support this |
| Ctrl+R | Reverse history search | Warp repurposes this (controversial) — we should NOT |
| Ctrl+Z | Suspend process | Don't repurpose |
| Ctrl+A/E | Start/end of line | Must pass through for input editing |
| Ctrl+W | Delete word backward | Must pass through |
| Ctrl+U | Delete to start of line | Must pass through |
| Tab | Autocomplete | AI tools use this |
| Escape | Cancel/abort | Claude Code: single=cancel, double=rewind menu. Conflicts with vim. |

### Recommendation for terminal-exploration

**Phase 1 (proxy) — Key translation layer:**

The proxy is uniquely positioned to solve the Shift+Enter problem for ALL AI CLI tools:

1. **Detect Kitty protocol support** from the outer terminal (Windows Terminal 1.25+)
2. **Negotiate Kitty protocol** with the outer terminal on startup
3. **Receive Shift+Enter as `CSI 13;2 u`** from the outer terminal
4. **Translate to the AI tool's expected format:**
   - For Claude Code: translate to `\x1b\x0d` (ESC + CR = Alt+Enter equivalent)
   - For Gemini CLI: translate to Ctrl+J (`0x0a`)
   - For Copilot CLI: inject a literal newline character
5. **Fallback:** If outer terminal doesn't support Kitty protocol, Alt+Enter still works

This means Shift+Enter would "just work" in terminal-exploration regardless of which AI tool is running, without requiring `/terminal-setup` or any user configuration.

**Phase 2 (Tauri) — Full control:**

Since Tauri intercepts raw key events before they hit the terminal protocol layer, Shift+Enter detection is trivial — it's a GUI key event, not a terminal byte sequence. This solves the problem completely.

**Configurable behavior (Slack model):**
```toml
[keybindings]
# "standard" = Enter submits, Shift+Enter newline (default)
# "reversed" = Enter newline, Ctrl+Enter submits
enter_behavior = "standard"
```

---

## 8. Clickable URLs and File Paths

### Two Detection Mechanisms

**Regex-based (implicit):** Terminal scans screen content for URL patterns (`http://`, `https://`, file paths). Imprecise — can't reliably determine where URLs end with parentheses, trailing punctuation, or wrapped lines. All major terminals use this as a baseline.

**OSC 8 explicit hyperlinks (the "right" way):**
```
ESC]8;params;URI BEL  ...visible text...  ESC]8;;BEL
```
- Analogous to HTML `<a href>` — display text can differ from target URL
- Use `BEL` (`\x07`) terminator, not `ST` (`ESC\`) — more widely compatible (critical finding from Claude Code issue #13008)
- Params support `id=` for connecting disjoint cells sharing one link
- URI limit: 2083 bytes, ASCII 32-126 only
- Supported by ~20+ terminal emulators, tmux 3.4+, Zellij 0.21+

### How Terminals Handle Clicks

| Terminal | Activation | Hover Behavior | Confirmation Dialog |
|----------|-----------|----------------|-------------------|
| Windows Terminal | Ctrl+click | Tooltip with URL | No |
| Ghostty | Ctrl+click | Popover in lower-left corner | No |
| WezTerm | Click (configurable to Ctrl) | Underline | No |
| iTerm2 | Cmd+click | Underline | No |
| Kitty | Ctrl+click | Underline | No |
| VS Code terminal | Ctrl/Cmd+click | Underline + tooltip | No |

**No mainstream terminal implements a confirmation dialog before opening URLs.** The OSC 8 spec recommends it, but adoption is zero. This is a gap and a potential differentiator.

### File Path Handling — VS Code Is the Gold Standard

VS Code's integrated terminal has three link handler tiers:
1. **URIs/URLs** — `http://`, `vscode://`, `file://`
2. **File links** — verified to exist on disk, supports `file:1:2`, `file:line 1, column 2`, many formats
3. **Folder links** — opens new VS Code window

Shell integration lets VS Code know the CWD, so relative paths resolve correctly. Without it, falls back to workspace search. This is powered by xterm.js with custom link providers.

### How AI CLI Tools Handle This Today

**Claude Code:** Has added some OSC 8 support for file paths (issue #13008, 26 upvotes). Feature request for OSC 8 on Write/Edit/Read tool output (issue #27889). File paths with spaces are a pain point — OSC 8 solves elegantly by separating display text from URL-encoded `file://` URI.

**Gemini CLI / Copilot CLI:** No OSC 8 hyperlinks. Plain text file paths only.

**Other CLI tools with OSC 8:** `ls`, `eza`, `fd`, `bat`, `ripgrep`, GCC v10+, Cargo — all emit OSC 8 file hyperlinks.

### Security — This Is Critical for AI-Generated Output

AI-generated output is **untrusted content** that may contain crafted escape sequences. The proxy is uniquely positioned to address this.

**OSC 8 spoofing:** Display text can differ from actual URL. An AI model could be prompt-injected to output `[safe-looking-link](https://evil.com)` as an OSC 8 hyperlink. Users would see the safe text, click, and land on the malicious URL.

**URL scheme exploitation (real CVEs):**
- CVE-2023-46321: iTerm2 `x-man-page://` handler → argument injection → arbitrary code execution
- CVE-2023-46322: iTerm2 `ssh://` handler → writes to `.profile` → delayed code execution
- Hyper `ssh://` handler exploited via IFS variable substitution → code execution
- CVE-2022-46663: `less` failed to terminate OSC 8 sequences → RCE from git commit messages

**Escape sequence injection vectors:** Docker image metadata, Kubernetes events, git commit messages, AI model output can all contain embedded ANSI with malicious hyperlinks.

**12 CVEs found in terminal emulators in 2022-2023** (dgl.cx research). Developers are high-value targets.

### Recommendation for terminal-exploration

**Phase 1 (proxy) — Security-first passthrough:**

The proxy processes all VT output through the emulator. For OSC 8 hyperlinks:

1. **Pass through valid OSC 8** from child process to host terminal (don't strip them)
2. **URL scheme whitelist:** Only allow `http://`, `https://`, `file://` schemes through. Strip or neutralize `ssh://`, `x-man-page://`, `javascript:`, and other dangerous schemes
3. **Log suspicious links:** `warn!` when a non-whitelisted scheme is detected in child output
4. **Optional: synthesize OSC 8** for file paths that AI tools output as plain text (detect `src/foo.rs:42:10` patterns and wrap in `file://` hyperlinks)

**Phase 2 (Tauri + xterm.js) — Full control with confirmation:**

Since we own the rendering layer:

1. **Ctrl+click activation** (never bare click) — xterm.js supports this via link providers
2. **URL preview on hover** — show full actual URL in a tooltip, even if display text differs
3. **Confirmation dialog for URLs** — first terminal to do this:
   ```
   ┌─────────────────────────────────────────┐
   │  Open URL?                              │
   │                                         │
   │  Displayed: https://docs.example.com    │
   │  Actual:    https://docs.example.com    │
   │  ✅ Match                               │
   │                                         │
   │  [Open in Browser]  [Copy URL]  [Cancel]│
   └─────────────────────────────────────────┘
   ```
   If display text differs from actual URL, show a **mismatch warning:**
   ```
   ┌─────────────────────────────────────────┐
   │  ⚠️  URL Mismatch Detected              │
   │                                         │
   │  Displayed: https://bank.com/login      │
   │  Actual:    https://evil.phishing.com   │
   │  ❌ MISMATCH — displayed text does not  │
   │     match the target URL                │
   │                                         │
   │  [Copy URL]  [Cancel]                   │
   └─────────────────────────────────────────┘
   ```
4. **File path handling:**
   - Detect `file:line:column` patterns in output
   - Ctrl+click opens in the user's configured editor (`$EDITOR` or configurable)
   - If file doesn't exist on disk, show "File not found" instead of failing silently
   - Relative paths resolved against the child process's CWD (tracked via shell integration or ConPTY)
5. **Configurable behavior:**
   ```toml
   [links]
   # Require confirmation before opening URLs in browser
   confirm_urls = true
   # Require confirmation for file:// links
   confirm_files = false
   # Allowed URL schemes (others are blocked)
   allowed_schemes = ["http", "https", "file"]
   # Editor for opening files (defaults to $EDITOR)
   editor = "code"
   # Open files at line:column when detected
   editor_line_column = true
   ```

---

## 9. Output Layout — Inline vs Split vs Structured Stream

### The Question

AI CLI tools dump everything into a single scrolling stream: reasoning, code changes, tool usage, errors. VS Code marks these with colored dots, but it's still one undifferentiated scroll. Would splitting the terminal into parallel panes (chat, diffs, activity) help, or is it overwhelming?

### What the Research Says

**Raw inline (current Claude Code) fails for dense output:**
- All output types mixed in one undifferentiated scroll = "information firehose"
- Users report two opposing complaints simultaneously: "too much happening" AND "can't find what I need"
- 4,000+ scroll events/sec makes it literally unreadable during streaming

**Multi-pane is tempting but risky:**
- Each pane transition requires "mental recalibration and information recall" (UX cognitive load research)
- Zed moved from editor-native chat to an agent panel — users revolted, called it a "downgrade" (GitHub discussion #30596)
- Works ONLY when panes contain complementary info needed simultaneously (e.g., code + diff side-by-side)
- Fails when it just chops the same stream into pieces — creates more context-switching than it eliminates
- Cognitive research: humans can consciously process ~7 items at once (Sweller's cognitive load theory)

**The winning pattern: structured single stream with visual chunking:**

- **Warp blocks** — each command+output is a discrete visual unit with dividers. Users report "clean, block-based clarity" vs "crowded and linear." 90% faster scrolling than traditional terminals
- **Aider's architect/editor split** — separates reasoning from edits semantically within a single stream. Produced state-of-the-art benchmark results
- **Cursor 2.0's PR-review metaphor** — viewing AI changes as reviewable diffs, not a chat log. Well-received conceptually
- **Information radiators** — glanceable status (like Claude Code's colored dots) always visible, full detail on demand. Design principle: "selectivity and clarity — display only the most critical data"

### Industry Examples

| Tool | Approach | User Reception |
|------|----------|---------------|
| Claude Code | Raw inline stream | Flickering complaints, can't find things in scroll |
| Warp | Block-based (command+output units) | Strong positive — navigability, clarity, shareability |
| Cursor 2.0 | Agent sidebar with PR-review diffs | Mixed — concept liked, layout stability issues |
| Aider | Single stream with mode separation | Positive — reasoning vs edits clearly distinct |
| Zed | Moved from editor-native to agent panel | Negative backlash from power users |
| VS Code Copilot | Three modes (inline, chat panel, terminal) | Frustration about inconsistency between modes |
| Panes | Bundled terminal+chat+diff+activity | Positive for "all in one" but niche adoption |

### Key Design Principles

1. **Impose structure on the stream, don't split it.** Blocks and collapsible sections resolve the "too much" vs "can't find" tension without adding pane-switching costs.

2. **Progressive disclosure.** Show summary at glance level, expand for detail on demand. "Read 3 files, edited 1 file (+12 -3)" as a collapsed summary line, with full diff on click.

3. **Information radiators for status.** Always-visible status bar with tool, project, context %, elapsed time. Don't bury important state in the scroll.

4. **Preserve terminal-native interaction.** Zed's users revolted when they lost vim bindings in the new panel. Power users must feel like they're still in a terminal, not a chat app.

5. **Semantic separation within the stream.** Visually distinguish reasoning, file operations, code diffs, and conversational responses — even without separate panes.

### Recommendation

**Phase 1 (proxy):** Single stream. Differential rendering eliminates flicker — this alone is transformative. No layout changes needed.

**Phase 2 (Tauri):** Structured single stream with blocks and progressive disclosure:

```
┌─ Status Bar ─────────────────────────────────┐
│ 🟠 Claude Code · my-project · 45% ctx · 12s  │
├─ Turn 3 ──────────────────────────────────────┤
│ You: "Add error handling to the API routes"   │
├───────────────────────────────────────────────┤
│ 💭 Reasoning (collapsed — click to expand)    │
│ ▸ "I'll add try-catch blocks to..."           │
├───────────────────────────────────────────────┤
│ 📂 Read src/routes/api.ts                     │
│ 📝 Edit src/routes/api.ts (+12 -3)            │
│ 📝 Edit src/routes/auth.ts (+8 -2)            │
│    (collapsed diffs — click to expand)        │
├───────────────────────────────────────────────┤
│ ✅ "Added try-catch with proper error..."     │
├─ Turn 4 ──────────────────────────────────────┤
│ You: █                                        │
└───────────────────────────────────────────────┘
```

- **Blocks per conversation turn** — each prompt + response is a navigable unit
- **Collapsible sections** — reasoning, tool calls, diffs collapsed by default
- **Summary lines** — "Edit src/routes/api.ts (+12 -3)" instead of full diff inline
- **Status radiator** — always-visible bar, never buried in scroll
- **No forced multi-pane** — but allow optional side-by-side diff view for users who want it

---

## Summary: Feature Priority by Phase

### Phase 1 (CLI Proxy) — Available Now
| Feature | Priority | Notes |
|---------|----------|-------|
| Shift+Enter key translation | **High** | Unique value prop — solves the #1 UX complaint across all AI tools |
| OSC 8 passthrough + scheme whitelist | **High** | Security-first link handling; strip dangerous schemes from AI output |
| Resize forwarding for zoom | Must | Correctness requirement |
| Pass-through for all standard shortcuts | Must | Don't break Ctrl+C, Ctrl+L, etc. |

### Phase 2 (Tauri + xterm.js)
| Feature | Priority | Notes |
|---------|----------|-------|
| Tabs for multiple sessions | **High** | Each tab = one AI session |
| Tab naming (auto + manual) | **High** | Tool icon + project dir + status |
| Font/size/weight picker (GUI) | **High** | Accessibility baseline |
| 4+ color schemes (including high contrast, CVD) | **High** | WCAG 2.1 AA compliance |
| Ctrl+/-/0 zoom | **High** | Universal expectation |
| xterm.js screen reader mode | **High** | Built-in, just needs activation |
| Status bar (tool, project, context %, elapsed) | **High** | Always-visible progress |
| Conversation turn detection + navigation | **Medium** | Ctrl+Up/Down between prompts |
| Split panes within tabs | **Medium** | Binary tree model (WezTerm-style) |
| Clone/fork tab | **Medium** | Duplicate environment in new tab |
| Dark/light/OS-sync themes | **Medium** | Respect system preference |
| Collapsible tool call output | **Medium** | Show summary, expand on click |
| Configurable Enter behavior | **Medium** | Slack-style preference |
| URL confirmation dialog | **Medium** | First terminal to do this — security differentiator |
| Ctrl+click file opening (editor at line:col) | **Medium** | VS Code-style file path detection |
| OSC 8 mismatch warning | **Medium** | Flag when display text differs from actual URL |
| Turn index sidebar | **Low** | Visual conversation map |

### Phase 3 (Polish)
| Feature | Priority | Notes |
|---------|----------|-------|
| Session persistence (save/restore tabs) | **High** | Zellij-style auto-save |
| Conversation search (Ctrl+F across turns) | **High** | Search with message boundary awareness |
| Workspace profiles (named layouts) | **Medium** | Save/load project-specific setups |
| Compaction-aware history browsing | **Medium** | Load pre-compaction turns from JSONL |
| Persistent bookmarks | **Medium** | Survive session close |
| CVD-specific color schemes (tested) | **Medium** | Bloomberg-style user-tested palettes |
| Tab color coding by tool | **Low** | Visual differentiation at a glance |
| Annotation/notes on turns | **Low** | iTerm2-style scratchpad notes |
