# Terminal User Research & Personas

**Date:** 2026-03-13
**Purpose:** Understand who uses (and avoids) terminal-based AI tools, what they need, and how terminal-exploration serves each group.
**Scope:** All major AI CLI tools — Claude Code (primary), GitHub Copilot CLI, Google Gemini CLI. The proxy architecture is tool-agnostic; these personas apply across all three.

---

## Research Findings

### The Landscape

The terminal is experiencing a renaissance driven by two forces: GPU-accelerated emulators (Ghostty, WezTerm, Kitty) raising the performance bar, and AI coding assistants (Claude Code, Copilot CLI, Gemini CLI) making the terminal the primary interface for agentic development. But this convergence has exposed deep usability gaps that affect users across all skill levels.

### Pain Points by Category

#### 1. Scroll-Jumping & Flicker (The Universal Problem)
- Claude Code's top issues have ~1,860+ combined upvotes for scroll/flicker bugs, persisting 9+ months
- Google's Gemini CLI overhauled rendering in v0.15.0 specifically to fix this class of bug
- GitHub Copilot CLI has identical scroll-jumping and stuttering during streaming
- Root cause: AI tools stream rapid partial updates faster than terminal rendering can handle
- Anthropic recommends Ghostty with DEC mode 2026 — but that's Unix-only
- **This is the exact problem terminal-exploration solves, and it's not niche — it's the #1 complaint across all AI CLI tools**

#### 2. Output Readability Under Streaming Load
- Long AI responses scroll past faster than users can read
- No scroll lock — users lose their place when new output pushes content up
- Context switching between reading output and typing input is jarring
- Markdown rendering in terminals is inconsistent across emulators
- Code blocks blend into surrounding text without clear visual separation

#### 3. Beginner & Non-Technical Barriers
- CLIs are "unstructured text interfaces" — no visual affordances, no discoverability
- No undo, no breadcrumbs, no visual state indicators that GUI users expect
- Error messages are cryptic ("EPERM," "ENOENT") with no guidance
- Installation requires unfamiliar steps (package managers, PATH, config files)
- 48% of developers prefer to "stay hands-on" rather than delegate to AI — CLI amplifies hesitancy
- Claude Code launched a web version specifically to lower the CLI barrier
- Warp's block-based UI and AI integration are succeeding by making the terminal feel more like a GUI

#### 4. Accessibility Gaps
- CLIs lack semantic structure (no ARIA roles, no accessibility tree)
- Screen readers struggle with spinners, progress bars, ASCII art, streaming output
- Rapid terminal updates create noise floods for assistive technology
- Claude Code has an open feature request for `--screen-reader` mode
- Color-dependent UI elements are invisible to colorblind users without alternatives
- ACM research ("Accessibility of Command Line Interfaces") documents systematic exclusion

#### 5. Power User Expectations
- Sub-50ms startup (Ghostty, Alacritty set this bar)
- GPU-accelerated rendering for smooth scrolling at high throughput
- Built-in splits/tabs without needing tmux
- Scriptable/configurable (Lua in WezTerm, declarative in Ghostty)
- tmux + AI tools = known flickering issues that shouldn't exist in 2026
- Synchronized Output (DEC mode 2026) as table stakes

#### 6. The IDE vs Terminal Tension
- Cursor dominates AI coding because it "stays out of the way" inside the editor
- Terminal AI tools require context switching away from the editor
- Developers increasingly want "fire and forget" delegation — start a task, return to results
- Tool fragmentation: 54% of developers use 6+ tools, increasing cognitive load
- The role is shifting toward architecture, review, and guiding multiple AI agents

### Academic & Community Sources
- **"Terminal Lucidity" (MacInnis et al.)** — Mined 1,489 Stack Exchange questions; found configuration, rendering inconsistency, and escape sequence handling are top pain points
- **ACM "Accessibility of Command Line Interfaces"** — Documents how CLIs systematically exclude assistive technology users
- **clig.dev** — Community CLI design principles: discoverability, helpful errors, progressive disclosure

---

## User Personas

### Persona 1: "Alex" — The Terminal Power User

**Profile:** 10+ years experience. Lives in the terminal. Uses tmux, Neovim, custom dotfiles. Has opinions about font rendering and keybinding schemes.

**Terminal setup:** Ghostty or Kitty on Linux, WezTerm on macOS, Windows Terminal reluctantly on Windows. Multiple tmux sessions, custom keybindings, shell scripts for everything.

**Relationship with AI CLI tools:** Early adopter. Uses Claude Code daily for large refactors and code review. Has tried Copilot CLI for GitHub-integrated workflows and Gemini CLI for its Google ecosystem access. Runs multiple AI sessions in tmux panes and switches between tools based on task.

**Pain points:**
- Scroll-jumping during long AI responses breaks reading flow — affects Claude Code, Copilot CLI, and Gemini CLI equally
- tmux + any Ink-based CLI tool = flickering nightmare (known incompatibility)
- Wants to scroll back through output without losing the input prompt
- Zero tolerance for input lag or rendering glitches
- Frustrated that "the AI is fast but the terminal can't keep up"
- Has to learn different workarounds for each tool's rendering quirks

**What they need from terminal-exploration:**
- Rock-solid differential rendering that eliminates flicker
- Performance matching or beating their current emulator
- Scrollback history that survives Claude Code sessions
- No dumbing-down — full terminal capabilities, not a simplified wrapper
- Configurable keybindings, colors, and behavior without a GUI settings panel

**Key quote:** *"I don't need a prettier terminal. I need one that doesn't fight me when Claude is streaming 200KB of output."*

**Phase alignment:** Phase 1 (CLI proxy) delivers core value immediately.

---

### Persona 2: "Jordan" — The Pragmatic Mid-Level Dev

**Profile:** 3-5 years experience. Comfortable with git, basic shell commands, and their IDE's integrated terminal. Not a terminal power user but not afraid of it either.

**Terminal setup:** VS Code integrated terminal or Windows Terminal with default settings. Knows enough shell to navigate, run builds, and use git. Doesn't use tmux or terminal multiplexers.

**Relationship with AI CLI tools:** Started with Copilot CLI since it integrates with GitHub. Trying Claude Code after hearing it's more capable for agentic workflows. May use Gemini CLI occasionally. Doesn't have strong loyalty to one tool yet — uses whatever works best for the task.

**Pain points:**
- Loses their place in AI output when it scrolls past — affects all three tools
- Doesn't know which terminal to use (Windows Terminal? PowerShell? Git Bash? WSL?)
- Streaming output from any AI CLI tool makes the terminal "go crazy" with flickering
- Selecting text grabs line numbers and prompt characters — copying code blocks is painful
- Output feels like a wall of text — hard to parse where the AI's response starts and ends
- Switching between Claude Code, Copilot, and Gemini means re-learning quirks for each

**What they need from terminal-exploration:**
- Stable, readable output without scroll-jumping
- Clear visual separation between user input and AI output
- Easy text selection that respects code block boundaries
- Simple installation — single binary, download and run
- Sensible defaults that work without any configuration

**Key quote:** *"I just want to read what Claude is saying without the screen jumping around. Is that really too much to ask?"*

**Phase alignment:** Phase 1 solves the core pain. Phase 2 adds the visual polish they'd love.

---

### Persona 3: "Sam" — The Terminal-Hesitant Newcomer

**Profile:** Product manager, designer, data analyst, or junior developer who primarily uses GUIs. Has heard Claude Code is more powerful than the chat interface but the terminal feels intimidating. Currently uses Claude Desktop or the web app.

**Terminal setup:** Whatever came with their OS. Has opened PowerShell maybe twice. The blank prompt with a blinking cursor is actively intimidating.

**Relationship with AI CLI tools:** Has heard colleagues rave about Claude Code, Copilot CLI, and Gemini CLI. But every guide starts with "open your terminal and run..." which is already a barrier. Tried Claude Code once, hit an error, closed the window, went back to Claude Desktop. Might try Gemini CLI since they already use Google products.

**Pain points (not with terminals — with the *idea* of terminals):**
- No visual feedback about what's happening — GUIs have loading spinners, progress bars, breadcrumbs
- Fear of "breaking something" by typing the wrong command
- Doesn't know the difference between PowerShell, CMD, Git Bash, WSL — guides assume they do
- Installation instructions assume knowledge they don't have (PATH? package manager?)
- When something goes wrong, error messages are incomprehensible
- The terminal feels designed for people who already know how to use it
- Procrastinated switching for weeks/months because it seemed "too daunting and in depth"

**What they need from terminal-exploration:**
- An entry point that doesn't require terminal knowledge — double-clickable executable
- Visual affordances: clear prompts, status indicators, obvious "where to type"
- Helpful error messages explaining what went wrong and what to do next
- A feeling of safety — clear indication they can't accidentally break things
- Progressive disclosure — simple by default, power features discoverable over time
- Onboarding that doesn't assume any CLI knowledge

**Key quote:** *"I know Claude Code is better than the chat app but every time I open the terminal I feel like I'm going to accidentally delete my hard drive."*

**Phase alignment:** Phase 2 (Tauri standalone app) is where Sam finally gets on board. Phase 1 is invisible to them.

---

### Persona 4: "Riley" — The AI-Native Builder

**Profile:** 1-2 years coding experience, mostly learned through AI assistants. Comfortable with prompting but less comfortable with traditional development workflows. Building a startup, automating their job, or creating side projects entirely through AI.

**Terminal setup:** Whatever Claude Code told them to install. Follows setup guides step by step. Has a terminal open but thinks of it as "the place where Claude runs" rather than a general-purpose tool.

**Relationship with AI CLI tools:** Claude Code IS their development environment. They don't use a separate editor — they describe what they want, Claude builds it, they test it. 10+ sessions a day. Also uses Copilot CLI for quick git operations and Gemini CLI when they want a second opinion. Switches between tools fluidly based on what works.

**Pain points:**
- Long Claude sessions make the terminal slow and unresponsive
- Can't find code Claude generated 20 minutes ago — scrolled into oblivion
- When Claude is working on a long task, can't tell if it's still going or stuck
- Multiple tool calls make output noisy — they want results, not the process
- Wants to run Claude in the background and come back when it's done (fire-and-forget)
- Managing multiple projects means multiple terminal windows with no organization

**What they need from terminal-exploration:**
- Session history they can search — "find that React component Claude wrote earlier"
- Clear progress indication during long operations
- Ability to collapse/expand Claude's tool call details (show results, not the journey)
- Multiple sessions without multiple windows (tabs)
- Fast startup because they open/close sessions frequently
- Session persistence — pick up where they left off after closing

**Key quote:** *"Claude is my IDE. I need the terminal to be as good at showing me Claude's work as VS Code is at showing me files."*

**Phase alignment:** Phase 2 delivers tabs and session management. Phase 3 adds session persistence and search.

---

### Persona 5: "Morgan" — The Accessibility-Dependent Developer

**Profile:** Experienced developer who uses a screen reader (JAWS, NVDA) or other assistive technology. Fully capable engineer who is systematically excluded by terminal UIs that assume visual interaction.

**Terminal setup:** Windows Terminal with high contrast theme, screen reader running. Custom scripts to filter terminal noise. Avoids tools with heavy ASCII art or animation.

**Relationship with AI CLI tools:** Wants to use Claude Code, Copilot CLI, or Gemini CLI — AI coding assistants could be transformative for accessibility. But all three produce rapid streaming output that creates overwhelming noise for their screen reader. The tools that could help them most are currently the hardest to use.

**Pain points:**
- Streaming output triggers rapid-fire screen reader announcements — unusable during active generation
- ASCII art, box-drawing characters, and decorative elements are read character-by-character
- No way to tell the screen reader "wait until Claude is done, then read the response"
- Progress spinners create loops of "slash, dash, backslash, pipe, slash, dash..."
- Color-coded output (red errors, green success) is invisible without alternatives
- Most terminal tools treat accessibility as an afterthought — if they consider it at all

**What they need from terminal-exploration:**
- Screen reader mode that buffers output and announces complete responses
- Text-only mode stripping decorative elements
- Semantic structure — "Claude response begins" / "code block" / "response ends"
- Configurable verbosity for tool call output
- High contrast themes with non-color status indicators (text labels, sounds)
- ARIA-like structure if using a GUI frontend (Phase 2)

**Key quote:** *"I can write production code all day. But I can't use half the AI tools my team uses because they assume everyone is staring at a screen."*

**Phase alignment:** Phase 2 (Tauri) enables proper accessibility. Phase 3 polishes it.

---

## Feature Priority Matrix

### Must-Have for All Personas
1. **Eliminate scroll-jumping and flicker** — The universal pain point and the project's reason for existence
2. **Stable, readable streaming output** — Every persona needs to read AI responses without fighting the display
3. **Simple installation** — A single binary or installer that works without configuration

### Priority by Persona

| Feature | Alex (Power) | Jordan (Mid) | Sam (Newcomer) | Riley (AI-Native) | Morgan (A11y) |
|---------|:---:|:---:|:---:|:---:|:---:|
| Differential rendering | **Must** | **Must** | **Must** | **Must** | **Must** |
| Scrollback/history | **Must** | Nice | — | **Must** | Nice |
| Configurable keybindings | **Must** | — | — | — | Nice |
| Visual output separation | — | **Must** | **Must** | **Must** | — |
| Single-binary install | Nice | **Must** | **Must** | **Must** | **Must** |
| Screen reader mode | — | — | — | — | **Must** |
| Progress indicators | — | Nice | **Must** | **Must** | **Must** |
| Session search | Nice | — | — | **Must** | — |
| Collapsible tool output | — | Nice | Nice | **Must** | **Must** |
| Onboarding/help | — | — | **Must** | Nice | — |
| Tabs/multi-session | Nice | — | — | **Must** | — |
| Scroll lock | **Must** | Nice | — | Nice | Nice |

### Phase Alignment

| Phase | Primary Personas | Value Delivered |
|-------|-----------------|-----------------|
| **Phase 1: CLI Proxy** | Alex, Jordan | Eliminate flicker, stable scrollback — solves the #1 complaint |
| **Phase 2: Tauri + xterm.js** | Sam, Riley, Morgan | Visual UI, approachable entry point, accessibility foundation, tabs |
| **Phase 3: Polish** | All | Session persistence, search, full a11y, configurability, auto-update |

---

## Insight: The Adoption Funnel

There's a clear progression that mirrors how people adopt terminal-based AI tools:

```
Awareness → Interest → Hesitation → First Try → Friction → Abandon or Adapt
```

- **Sam** gets stuck at Hesitation — the terminal itself is the barrier
- **Jordan** gets stuck at Friction — they try it, hit scroll-jumping, wonder if it's worth it
- **Alex** pushes through Friction but is perpetually annoyed — they adapt with workarounds
- **Riley** has no choice but to Adapt — Claude Code IS their workflow, warts and all
- **Morgan** is blocked at First Try — the tool is functionally inaccessible

**Claude-terminal's job is to collapse this funnel — for all AI CLI tools, not just one.** Phase 1 removes the Friction for people already in the terminal (works with Claude Code, Copilot CLI, and Gemini CLI out of the box). Phase 2 removes the Hesitation for people who haven't gotten there yet.

### Why Multi-Tool Support Matters

The proxy architecture is inherently tool-agnostic — it processes standard VT100 output, not tool-specific protocols. This means:

- **Zero extra code** to support Copilot CLI and Gemini CLI alongside Claude Code
- **3x the addressable audience** — every user of any AI CLI tool on Windows benefits
- **Resilience to market shifts** — if developers switch between tools (and they do), the proxy stays relevant
- **Network effect** — a Copilot CLI user who discovers the proxy becomes aware of Claude Code's ecosystem

Claude Code remains the primary focus for testing, optimization, and community engagement. But we design and validate against all three.
