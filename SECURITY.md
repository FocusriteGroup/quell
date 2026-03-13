# Security & Trust Model

## What This Tool Does

terminal-exploration is a terminal proxy that sits between your terminal emulator and an AI CLI tool (Claude Code, Copilot CLI, Gemini CLI). It intercepts VT output, tracks screen state, and sends only differential updates to eliminate scroll-jumping and flicker.

## What The Proxy Can See

The proxy processes **all terminal output** from the child process. This includes:
- AI-generated code and responses
- File contents displayed by the AI tool
- Error messages (which may contain paths, stack traces)
- Environment variable dumps (if displayed by the tool)
- API keys or secrets if they appear in terminal output

The proxy also processes **all keyboard input** from the user, forwarding it to the child process.

## What The Proxy Does NOT Do

- **No network access.** The proxy binary makes zero network connections. It communicates only with the child process (via ConPTY pipes) and the user's terminal (via stdin/stdout).
- **No content logging.** Output content is never written to log files by default. Logging captures only metadata: frame counts, byte throughput, render timing, errors.
- **No telemetry.** No usage data is collected or transmitted.
- **No data persistence.** Terminal content exists only in memory (the history buffer) and is discarded when the proxy exits. Phase 2's session persistence is opt-in and stores data only on the local filesystem.

## Security Measures

### URL Scheme Whitelisting
AI-generated output is untrusted content. The proxy filters OSC 8 hyperlinks, allowing only `http://`, `https://`, and `file://` schemes. Other schemes (which have been used in real CVEs — `ssh://`, `x-man-page://`) are stripped.

### No Modification of Content
The proxy transforms the *rendering* of output (VT differential updates) but never modifies the *content*. What the AI tool writes is what the user sees — just rendered more efficiently.

### Raw Mode Restoration
The proxy saves and restores terminal mode on exit, including on panic. A crash cannot leave the terminal in an unusable state.

## Threat Model

### What could go wrong if the proxy had a vulnerability:
- An attacker with code execution on the user's machine could modify the proxy binary to intercept secrets from terminal output
- A malicious dependency in the build chain could inject data exfiltration

### Mitigations:
- The project is open source — all code is auditable
- Minimal dependency surface (only well-known Rust crates)
- Phase 2 distributed binaries will be code-signed
- Users can build from source to eliminate supply chain risk

## Verification

Users can verify the proxy makes no network connections:
- Windows: `netstat -b` while the proxy is running — the binary should have zero network connections
- Process Monitor (Sysinternals): filter by process name, verify no network activity

## Licensing

MIT license — free for any use, commercial or personal. See LICENSE file.
