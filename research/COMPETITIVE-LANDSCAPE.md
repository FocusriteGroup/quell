# Competitive Landscape

**Date:** 2026-03-13
**Status:** The proxy-only value proposition has a narrowing window. Phase 2 features are the long-term moat.

---

## Direct Competitors

### Anthropic's Own Rendering Fixes
- Rewrote Claude Code's renderer — ~85% flicker reduction
- Contributing DEC mode 2026 patches upstream (VS Code, tmux)
- Recommends Ghostty for zero-flicker experience (Unix only)
- **Still incomplete on Windows.** VS Code terminal patches in progress. Windows Terminal works but scroll-jumping persists in many setups.

### Claude Chill (github.com/davidbeesley/claude-chill)
- Rust PTY proxy — same architecture as this project
- Intercepts sync blocks, VT100 emulation, differential rendering
- **Linux/macOS only — no Windows support**
- Validates the approach but shows someone else is in this space

### Gemini CLI Built-In PTY
- v0.15.0 overhauled rendering with pseudo-terminal snapshots and diff streaming
- Built directly into the tool (no separate proxy needed)
- Still has regressions in tmux/multiplexer environments

### Copilot CLI (GA March 2026)
- Has its own terminal rendering approach
- Now supports multiple AI models including Claude Opus 4.6
- Flickering still reported in VS Code terminal

## Indirect Competitors

### Terminal Emulators
- **Ghostty** — Zero flicker via native DEC 2026 support. Unix only.
- **WezTerm** — Full DEC 2026, ships custom OpenConsole on Windows
- **Windows Terminal** — DEC 2026 since v1.24, Kitty protocol in Preview 1.25

### AI Coding Environments
- **Cursor** — IDE-integrated, avoids terminal rendering entirely
- **OpenCode** — MIT-licensed Claude Code alternative, 640+ contributors, any model provider
- **Panes** — Bundles terminal + chat + diff in unified interface

## The Narrowing Window

The proxy's core value (eliminate flicker) is being addressed at the source:
- Anthropic fixing Claude Code's renderer
- Google fixing Gemini CLI's renderer
- Terminal emulators adding DEC 2026 support

**What the AI tools WON'T build** (our Phase 2 moat):
- Multi-tool terminal (Claude + Copilot + Gemini in tabs)
- Structured block output with progressive disclosure
- Scroll lock / freeze during streaming
- Cross-session search and persistent history
- Accessibility-first design (screen reader mode, CVD schemes)
- URL security filtering at the proxy layer
- Shift+Enter key translation for all tools

## Strategic Implication

Phase 1 proxy is still valuable NOW (Windows has zero solutions, tools' fixes are incomplete). But don't over-invest in Phase 1 polish. Get it working, prove it, move to Phase 2 where the differentiation lives.
