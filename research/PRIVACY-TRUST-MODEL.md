# Privacy & Trust Model — Deep Analysis

**Date:** 2026-03-13
**Status:** Critical path item. Must be addressed before any public release.

---

## The Trust Position

The proxy sits in a MITM-equivalent position between the user's terminal and their AI tool. It sees:
- All keyboard input (including passwords, API keys typed)
- All terminal output (code, secrets, file contents, error messages)
- All escape sequences (hyperlinks, titles, clipboard operations)

This is the same trust position as the terminal emulator itself. Users already trust Windows Terminal / VS Code with this data. The bar for a third-party proxy is higher.

---

## Threat Categories

### 1. The Proxy Itself as Attack Surface

**Escape sequence forwarding attacks (HIGH RISK)**

The child process (Claude Code, Copilot, Gemini) emits VT sequences. A malicious child — or a legitimate tool operating on a compromised repository — could emit crafted sequences designed to attack the outer terminal *through* the proxy.

Known attack classes:
| Attack | Mechanism | Proxy Risk |
|--------|-----------|------------|
| Title injection (OSC 2) | Set title to shell command, request title report → echoed as terminal input | **High** — proxy relays OSC sequences |
| DECRQSS echoback | Query response echoes attacker data including newlines → command injection | **High** if proxy responds to queries |
| Clipboard exfiltration (OSC 52) | Child reads clipboard via escape sequence | **Medium** — should be blocked |
| Hyperlink spoofing (OSC 8) | Display text differs from actual URL → phishing | **Medium** — addressed by scheme whitelist |
| C1 control ambiguity | 0x80-0x9F bytes interpreted as control codes in non-UTF-8 mode | **Medium** |

**Mitigations required:**
- Never respond to terminal query sequences (DECRQSS, title report, etc.) on behalf of the proxy
- Strip dangerous OSC sequences: OSC 52 (clipboard), OSC 50 (font query)
- Reject C1 control bytes (0x80-0x9F) — only accept 7-bit escape sequences
- The URL scheme whitelist already covers OSC 8

### 2. VT Emulator Memory Safety

The `vt100` crate processes adversarial input by design. It's written in Rust (memory-safe for safe code) but:
- Depends on `vte` which may contain `unsafe` blocks
- IEEE S&P 2026 research found 12 memory safety bugs across 63 Rust crates by fuzzing `unsafe` boundaries
- Integer overflow in screen dimensions during resize is a known class of bug
- Extreme sequence parameters (e.g., `ESC[999999;999999H`) could cause panics

**Mitigations required:**
- Fuzz the VT processing pipeline with `cargo-fuzz`
- Catch panics at the proxy boundary (crash emulator state, not the whole proxy)
- Bounds-check sequence parameters before passing to `vt100`

### 3. Supply Chain

Five malicious Rust crates were discovered in March 2026 exfiltrating `.env` files. Our dependency list is small and well-known (`vt100`, `memchr`, `vte`, `termwiz`, `windows`) but not immune.

**Mitigations required:**
- `cargo-audit` in CI against RustSec Advisory Database
- `cargo-deny` for license checking and crate banning
- Consider `cargo-vet` for formal review tracking
- Pin dependency versions in `Cargo.lock` (already committed)

### 4. Binary Authenticity

Without code signing, users can't verify they're running the real binary. Windows SmartScreen will actively warn against unsigned executables.

**Mitigations required:**
- Phase 1: distribute via scoop/winget (package manager verification) + provide SHA256 checksums
- Phase 2: code signing for Tauri .exe (Certum OV certificate ~$200/year, or SignPath.io free for open source)

### 5. Config File Trust

CVE-2025-59536 showed that Claude Code's project-level config files could be weaponized. If the proxy reads `.quell.toml` from the working directory, a malicious repository could configure the proxy to behave unexpectedly.

**Current risk:** The proxy reads config from `--config` flag, `./` (local), and `%APPDATA%` (global). The local config path means a cloned repo could include a proxy config.

**Mitigations required:**
- Warn if loading project-local config that differs from user-global config
- Never execute commands or shell expansions from config values
- Consider removing project-local config search path entirely (only `--config` flag and `%APPDATA%`)

### 6. Session Persistence Data at Rest (Phase 2/3)

Session persistence stores terminal content to disk, which will contain code, secrets, PII.

**Mitigations required:**
- Default to OFF — opt-in only
- Store in user-profile directory only (never in project directory where git could commit it)
- Encrypt at rest using DPAPI (Windows data protection API)
- Automatic expiration (configurable, default 30 days)
- "Clear all history" command that reliably deletes everything
- Never transmit session data over any network

### 7. Logging Content Leaks

Structured logging at `trace!` level may include raw VT bytes (CLAUDE.md specifies this). If a user enables trace logging and the log file is in a shared location, terminal content leaks to disk.

**Mitigations required:**
- Default log level `info` — no content at this level
- `debug` logs include metadata only (frame counts, byte sizes, timing)
- `trace` logs warn at startup: "Trace logging may include terminal content"
- Log files respect user-profile scoping (never in project directory)

---

## Trust Communication Strategy

### What Users Need to See

Based on the Warp controversy (login requirement → "spyware" accusations → 1,346-issue GitHub thread) and broader developer tool trust patterns:

1. **Prominent "no network" statement** — developers are paranoid about tools that phone home, rightfully so
2. **Explicit list of what IS and ISN'T logged** — be specific, not vague
3. **Open source as baseline** — necessary but not sufficient for trust
4. **Verifiable claims** — tell users HOW to verify (netstat, Process Monitor)
5. **Responsible disclosure process** — GitHub Private Vulnerability Reporting

### Artifacts to Produce

| Document | Purpose | When |
|----------|---------|------|
| `SECURITY.md` | Threat model, reporting process, what data is handled | Before Phase 1 release |
| `PRIVACY.md` | What's stored, where, how to delete, encryption details | Before Phase 2 release (session persistence) |
| GitHub Security Policy | Enable Private Vulnerability Reporting | Before Phase 1 release |
| SBOM | Software Bill of Materials from `Cargo.lock` | Automate in CI |
| Dependency audit results | `cargo-audit` / `cargo-deny` output | Automate in CI |

---

## Escape Sequence Security — Deep Dive

This deserves special attention because the proxy's core job is forwarding VT sequences, and this is the #1 attack surface for terminals.

### Sequences to BLOCK (never forward from child to outer terminal)

| Sequence | Name | Risk | Action |
|----------|------|------|--------|
| `ESC]52;...ST` | OSC 52 Clipboard | Child reads/writes clipboard | **Strip entirely** |
| `ESC]50;...ST` | OSC 50 Font Query | Font name echoed unescaped → Zsh expansion | **Strip entirely** |
| `ESC[c` | DA Primary | Device attributes report echoed as input | **Don't respond** |
| `ESC[>c` | DA Secondary | Same | **Don't respond** |
| `ESC[=c` | DA Tertiary | Same | **Don't respond** |
| `ESCPDECRQSS...ST` | DECRQSS | Setting report echoed with attacker data | **Don't forward query response** |
| `ESC]l...ST` | OSC Title Report | Title echoed as input → command injection | **Don't respond** |

### Sequences to FILTER (forward with sanitization)

| Sequence | Name | Risk | Action |
|----------|------|------|--------|
| `ESC]8;...ST` | OSC 8 Hyperlinks | Scheme spoofing, phishing | **Whitelist schemes** (http, https, file) |
| `ESC]2;...ST` | OSC 2 Set Title | Title can contain misleading text | **Forward but strip control chars from title text** |
| 0x80-0x9F | C1 Control Codes | Ambiguous interpretation in non-UTF-8 | **Strip — only accept 7-bit escapes** |

### Sequences to ALLOW (safe to forward)

| Category | Examples | Notes |
|----------|----------|-------|
| Cursor movement | CUP, CUU, CUD, CUF, CUB | Safe |
| Text styling | SGR (colors, bold, etc.) | Safe |
| Screen management | ED, EL, DECSTBM | Safe |
| DEC mode 2026 | BSU/ESU sync markers | Core to our function |
| Character sets | SCS, G0-G3 | Safe |
| Scrolling | SU, SD, DECSTBM | Safe |

### Implementation Approach

This maps directly to the existing escape filter architecture (Milestone 1.4):
- **Layer 1 (byte-level):** Strip known-dangerous byte patterns (OSC 52, OSC 50, C1 codes)
- **Layer 2 (parse-level via termwiz):** Classify parsed sequences against the allow/filter/block tables above

The escape filter is currently stubbed as pass-through. The security work is completing the filter with the correct classification rules.

---

## Comparison with Claude-Chill

| Aspect | claude-chill | quell |
|--------|-------------|---------------------|
| SECURITY.md | None | Yes (written) |
| Threat model | None | Documented |
| Content logging | None | None by default, trace-level with warning |
| Network access | None | None |
| Escape sequence filtering | Not documented | Explicit allow/filter/block lists |
| Code signing | None | Planned (Phase 2) |
| Supply chain auditing | Not documented | cargo-audit + cargo-deny in CI |
| Session persistence | No | Opt-in, encrypted at rest (Phase 2/3) |
| Config trust | Not discussed | Project-local config considered |

We're ahead of the comparable tool on every security dimension. This is a genuine differentiator.
