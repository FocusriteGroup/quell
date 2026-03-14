use std::fmt;

/// Which AI CLI tool the proxy is wrapping.
/// Determines key translation behavior (e.g., Shift+Enter mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Claude,
    Gemini,
    Copilot,
    Unknown,
}

impl ToolKind {
    /// Detect the tool from the child command string.
    /// Matches against the executable name, ignoring path and extension.
    pub fn detect(command: &str) -> Self {
        // Extract the executable name: first token, strip path and extension
        let exe = command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .strip_suffix(".exe")
            .unwrap_or(
                command
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(""),
            )
            .to_lowercase();

        match exe.as_str() {
            "claude" => Self::Claude,
            "gemini" => Self::Gemini,
            "copilot" => Self::Copilot,
            _ => Self::Unknown,
        }
    }

    /// The byte sequence to send to the child when Shift+Enter is pressed.
    pub fn shift_enter_bytes(&self) -> &'static [u8] {
        match self {
            // ESC + CR — equivalent to Alt+Enter, which Claude Code recognizes as newline
            Self::Claude => b"\x1b\x0d",
            // Ctrl+J — Gemini CLI's newline shortcut
            Self::Gemini => b"\x0a",
            // Literal newline
            Self::Copilot => b"\x0a",
            // Default to ESC + CR (safe — most tools treat this as newline or ignore it)
            Self::Unknown => b"\x1b\x0d",
        }
    }
}

impl fmt::Display for ToolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Gemini => write!(f, "gemini"),
            Self::Copilot => write!(f, "copilot"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Parse a tool kind from a CLI argument string.
pub fn parse_tool_kind(s: &str) -> Result<ToolKind, String> {
    match s.to_lowercase().as_str() {
        "claude" => Ok(ToolKind::Claude),
        "gemini" => Ok(ToolKind::Gemini),
        "copilot" => Ok(ToolKind::Copilot),
        _ => Err(format!("unknown tool '{s}' (expected: claude, gemini, copilot)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude() {
        assert_eq!(ToolKind::detect("claude"), ToolKind::Claude);
        assert_eq!(ToolKind::detect("claude --help"), ToolKind::Claude);
        assert_eq!(ToolKind::detect("claude.exe"), ToolKind::Claude);
        assert_eq!(ToolKind::detect(r"C:\Users\me\.npm\claude.exe"), ToolKind::Claude);
        assert_eq!(ToolKind::detect("/usr/local/bin/claude"), ToolKind::Claude);
    }

    #[test]
    fn test_detect_gemini() {
        assert_eq!(ToolKind::detect("gemini"), ToolKind::Gemini);
        assert_eq!(ToolKind::detect("gemini chat"), ToolKind::Gemini);
    }

    #[test]
    fn test_detect_copilot() {
        assert_eq!(ToolKind::detect("copilot"), ToolKind::Copilot);
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(ToolKind::detect("python"), ToolKind::Unknown);
        assert_eq!(ToolKind::detect("cmd.exe"), ToolKind::Unknown);
        assert_eq!(ToolKind::detect(""), ToolKind::Unknown);
    }

    #[test]
    fn test_shift_enter_bytes() {
        assert_eq!(ToolKind::Claude.shift_enter_bytes(), b"\x1b\x0d");
        assert_eq!(ToolKind::Gemini.shift_enter_bytes(), b"\x0a");
        assert_eq!(ToolKind::Copilot.shift_enter_bytes(), b"\x0a");
        assert_eq!(ToolKind::Unknown.shift_enter_bytes(), b"\x1b\x0d");
    }

    #[test]
    fn test_parse_tool_kind() {
        assert_eq!(parse_tool_kind("claude"), Ok(ToolKind::Claude));
        assert_eq!(parse_tool_kind("Claude"), Ok(ToolKind::Claude));
        assert_eq!(parse_tool_kind("GEMINI"), Ok(ToolKind::Gemini));
        assert_eq!(parse_tool_kind("copilot"), Ok(ToolKind::Copilot));
        assert!(parse_tool_kind("vim").is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(ToolKind::Claude.to_string(), "claude");
        assert_eq!(ToolKind::Gemini.to_string(), "gemini");
        assert_eq!(ToolKind::Copilot.to_string(), "copilot");
        assert_eq!(ToolKind::Unknown.to_string(), "unknown");
    }
}
