# Progress Visualization

**Source:** UX feature research, 2026-03-13

---

## How AI CLI Tools Show Progress Today

**Claude Code:** Customizable spinner verbs ("Thinking...", "Working...") on a 50ms animation loop. Shows effort level, token count, tool uses, and duration in results.

**Copilot CLI:** Loading spinner in diff mode. Streaming response size counter during tool calls. Auto-adapting status line for narrow terminals.

**Gemini CLI:** Pseudo-terminal snapshots streamed in real-time. JSONL event streaming for headless monitoring.

**Codex CLI:** Alternate screen buffer (no scrollback by design). Handles scrolling internally via mouse tracking.

## Progress Indicator Patterns

| Type | When to Use | Example |
|------|------------|---------|
| Bounded progress bar | Known-length operations (file download, batch processing) | `[========..] 80% 4/5 files (ETA 3s)` |
| Spinner + elapsed | Unknown-length operations (AI thinking, tool execution) | `Thinking... (12s)` |
| Streaming counter | Active streaming output (AI response generation) | `Writing... 2,847 tokens (8s)` |
| Status line | Persistent state info | `Claude Code - quell - 45% context` |

## Timing Guidelines

- Spinner frame rate: **80-130ms** per frame (below 50ms = jittery, above 300ms = sluggish)
- Always show **elapsed time** — it anchors the user's sense of progress
- For streaming AI responses: show **token/byte counter** alongside spinner
- Use `indicatif` crate (Rust) for thread-safe progress bars with templates

## Recommendation

**Phase 1 (proxy):** The proxy passes through whatever progress indicators the AI tool renders. No additional progress UI in proxy mode.

**Phase 2 (Tauri):**
- **Status bar** at bottom: tool name, project directory, context usage %, elapsed time for current operation
- **Turn-level progress:** When AI is responding, show spinner + elapsed + token counter in the status bar
- **Tool call summary:** Collapse tool call details by default, show "Read 3 files, edited 1 file (4.2s)" as a summary line that expands on click
- **Stall detection:** If no new output for 10+ seconds during an active operation, show "Still working..." indicator to distinguish stall from hang
