# Keyboard Input — Shift+Enter, Kitty Protocol, Key Translation

**Source:** UX feature research, 2026-03-13

---

## The Shift+Enter Problem

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

## The Kitty Keyboard Protocol — The Fix

Kitty protocol solves the 48-year limitation by encoding modifier keys explicitly:
- Enter -> `CSI 13 u`
- Shift+Enter -> `CSI 13;2 u` (now distinct!)
- Supports progressive enhancement levels

### Terminal support:
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

## Other AI Tool Approaches
- **Gemini CLI:** Ctrl+J for newline (non-standard but distinct at byte level)
- **Copilot CLI:** No dedicated newline shortcut (paste multi-line only)
- **Warp:** Ctrl+R for newline (breaks reverse-search convention)

## Standard Shortcuts That Must Not Break

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

## Recommendation

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

This means Shift+Enter would "just work" in quell regardless of which AI tool is running, without requiring `/terminal-setup` or any user configuration.

**Phase 2 (Tauri) — Full control:**

Since Tauri intercepts raw key events before they hit the terminal protocol layer, Shift+Enter detection is trivial — it's a GUI key event, not a terminal byte sequence. This solves the problem completely.

**Configurable behavior (Slack model):**
```toml
[keybindings]
# "standard" = Enter submits, Shift+Enter newline (default)
# "reversed" = Enter newline, Ctrl+Enter submits
enter_behavior = "standard"
```
