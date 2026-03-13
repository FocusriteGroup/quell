// Escape sequence filter for safe history replay
//
// When replaying history, we need to strip sequences that could
// cause side effects (terminal queries, mode changes, etc.) while
// preserving visual sequences (colors, cursor moves, text).
//
// Uses termwiz's parser to classify every escape sequence, then
// applies a whitelist: only sequences known to be safe for replay
// are kept. Everything else is stripped.

use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, Esc, EscCode, CSI};
use termwiz::escape::osc::OperatingSystemCommand as OSC;
use tracing::{debug, trace};

/// Filters terminal escape sequences for safe replay.
///
/// Uses termwiz to parse and classify sequences, keeping only
/// those on the whitelist (visual formatting, cursor movement, text).
pub struct EscapeFilter;

impl EscapeFilter {
    /// Filter bytes for safe replay.
    ///
    /// Parses the input with termwiz to classify every escape sequence,
    /// then keeps only whitelisted (safe) sequences. Returns filtered bytes
    /// with dangerous sequences removed.
    pub fn filter_for_replay(data: &[u8]) -> Vec<u8> {
        let mut parser = Parser::new();
        let mut output = Vec::with_capacity(data.len());
        let mut pos = 0;

        while pos < data.len() {
            match parser.parse_first_as_vec(&data[pos..]) {
                Some((actions, consumed)) if consumed > 0 => {
                    let original = &data[pos..pos + consumed];
                    if actions.iter().all(|a| is_safe_for_replay(a)) {
                        output.extend_from_slice(original);
                    } else {
                        trace!(
                            actions = ?actions,
                            bytes = consumed,
                            "stripped unsafe sequence during replay filter"
                        );
                    }
                    pos += consumed;
                }
                _ => {
                    // Parser couldn't make progress — copy byte and advance
                    output.push(data[pos]);
                    pos += 1;
                }
            }
        }

        debug!(
            input_bytes = data.len(),
            output_bytes = output.len(),
            stripped_bytes = data.len() - output.len(),
            "replay filter complete"
        );

        output
    }
}

/// Returns true if an action is safe for replay (whitelist approach).
fn is_safe_for_replay(action: &Action) -> bool {
    match action {
        // Plain text — always safe
        Action::Print(_) | Action::PrintString(_) => true,

        // Control codes — safe subset
        Action::Control(code) => {
            use termwiz::escape::ControlCode;
            matches!(
                code,
                ControlCode::LineFeed
                    | ControlCode::CarriageReturn
                    | ControlCode::HorizontalTab
                    | ControlCode::Backspace
                    | ControlCode::Bell
                    | ControlCode::FormFeed
                    | ControlCode::VerticalTab
                    | ControlCode::Null
            )
        }

        // CSI sequences — whitelisted subcategories
        Action::CSI(csi) => is_safe_csi(csi),

        // OSC sequences — only safe subset
        Action::OperatingSystemCommand(osc) => is_safe_osc(osc),

        // ESC sequences — safe subset
        Action::Esc(esc) => is_safe_esc(esc),

        // DCS, Sixel, KittyImage, XtGetTcap — not safe for replay
        Action::DeviceControl(_)
        | Action::Sixel(_)
        | Action::KittyImage(_)
        | Action::XtGetTcap(_) => false,
    }
}

/// Returns true if a CSI sequence is safe for replay.
fn is_safe_csi(csi: &CSI) -> bool {
    match csi {
        // SGR (colors, bold, italic, etc.) — always safe
        CSI::Sgr(_) => true,

        // Cursor movement/positioning — safe
        CSI::Cursor(cursor) => {
            use termwiz::escape::csi::Cursor;
            matches!(
                cursor,
                Cursor::Up(_)
                    | Cursor::Down(_)
                    | Cursor::Left(_)
                    | Cursor::Right(_)
                    | Cursor::NextLine(_)
                    | Cursor::PrecedingLine(_)
                    | Cursor::Position { .. }
                    | Cursor::LinePositionAbsolute(_)
                    | Cursor::CharacterPositionAbsolute(_)
                    | Cursor::CharacterAndLinePosition { .. }
                    | Cursor::ForwardTabulation(_)
                    | Cursor::BackwardTabulation(_)
                    | Cursor::SaveCursor
                    | Cursor::RestoreCursor
                    | Cursor::CursorStyle(_)
                    | Cursor::SetTopAndBottomMargins { .. }
            )
            // Stripped: RequestActivePositionReport, ActivePositionReport
        }

        // Edit operations (clear screen, delete lines, etc.) — safe
        CSI::Edit(_) => true,

        // Mode changes — NOT safe (could change terminal state)
        CSI::Mode(_) => false,

        // Device queries — NOT safe (trigger terminal responses)
        CSI::Device(_) => false,

        // Mouse reports — NOT safe (could confuse terminal)
        CSI::Mouse(_) => false,

        // Window operations — NOT safe (move/resize/iconify windows)
        CSI::Window(_) => false,

        // Keyboard protocol — NOT safe
        CSI::Keyboard(_) => false,

        // Character path — safe (text layout)
        CSI::SelectCharacterPath(_, _) => true,

        // Unknown CSI — strip for safety
        CSI::Unspecified(_) => false,
    }
}

/// Returns true if an OSC sequence is safe for replay.
fn is_safe_osc(osc: &OSC) -> bool {
    match osc {
        // Window/icon title — safe for replay
        OSC::SetIconNameAndWindowTitle(_)
        | OSC::SetWindowTitle(_)
        | OSC::SetIconName(_)
        | OSC::SetWindowTitleSun(_)
        | OSC::SetIconNameSun(_) => true,

        // Hyperlinks — safe
        OSC::SetHyperlink(_) => true,

        // Clipboard operations — NOT safe
        OSC::ClearSelection(_) | OSC::QuerySelection(_) | OSC::SetSelection(_, _) => false,

        // Color changes — safe (visual only)
        OSC::ChangeColorNumber(_) | OSC::ChangeDynamicColors(_, _) => true,

        // Color resets — safe
        OSC::ResetDynamicColor(_) | OSC::ResetColors(_) => true,

        // CWD notification — safe
        OSC::CurrentWorkingDirectory(_) => true,

        // System notifications — safe (informational)
        OSC::SystemNotification(_) => true,

        // iTerm2 proprietary — strip for safety
        OSC::ITermProprietary(_) => false,

        // FinalTerm semantic prompts — safe (shell integration)
        OSC::FinalTermSemanticPrompt(_) => true,

        // Unknown/vendor extensions — strip for safety
        OSC::RxvtExtension(_) | OSC::Unspecified(_) => false,
    }
}

/// Returns true if an ESC sequence is safe for replay.
fn is_safe_esc(esc: &Esc) -> bool {
    match esc {
        Esc::Code(code) => matches!(
            code,
            EscCode::ReverseIndex
                | EscCode::NextLine
                | EscCode::DecSaveCursorPosition
                | EscCode::DecRestoreCursorPosition
                | EscCode::StringTerminator
        ),
        // Unknown ESC sequences — strip
        Esc::Unspecified { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_passes_through() {
        let input = b"hello world";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_sgr_colors_preserved() {
        // Bold red text
        let input = b"\x1b[1;31mhello\x1b[0m";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_cursor_moves_preserved() {
        // Move to position 5,10 then write
        let input = b"\x1b[5;10Hhello";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_cursor_up_down_left_right_preserved() {
        let input = b"\x1b[3A\x1b[2B\x1b[1C\x1b[4D";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_edit_operations_preserved() {
        // Clear screen + erase line
        let input = b"\x1b[2J\x1b[K";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_da_queries_stripped() {
        // DA1 query
        let input = b"before\x1b[cafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_da_secondary_stripped() {
        let input = b"before\x1b[>cafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_dsr_stripped() {
        // Device status report
        let input = b"before\x1b[5nafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_cursor_position_request_stripped() {
        // CPR request (ESC[6n)
        let input = b"before\x1b[6nafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_mode_changes_stripped() {
        // Set bracketed paste mode
        let input = b"before\x1b[?2004hafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_mode_reset_stripped() {
        // Reset alternate screen
        let input = b"before\x1b[?1049lafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_mouse_mode_stripped() {
        // Enable SGR mouse
        let input = b"before\x1b[?1006hafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_kitty_keyboard_stripped() {
        // Kitty keyboard query — CSI ? u
        let input = b"before\x1b[?uafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_osc52_clipboard_stripped() {
        // Clipboard set
        let input = b"before\x1b]52;c;SGVsbG8=\x07after";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_osc2_title_preserved() {
        let input = b"\x1b]2;My Title\x07";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_osc8_hyperlink_preserved() {
        let input = b"\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_dcs_stripped() {
        // DCS sequence
        let input = b"before\x1bPq#0;2;0;0;0#1;2;100;100;0\x1b\\after";
        let output = EscapeFilter::filter_for_replay(input);
        // DCS content stripped, text preserved
        assert!(output.starts_with(b"before"));
        assert!(output.ends_with(b"after"));
    }

    #[test]
    fn test_control_codes_safe_subset() {
        // LF, CR, TAB, BS — safe
        let input = b"line1\nline2\r\n\ttab\x08bs";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_c1_control_codes_handled() {
        // C1 bytes are stripped by Layer 1 (OutputFilter) before reaching
        // replay. If they somehow arrive here, termwiz may not parse them
        // as escape initiators in UTF-8 mode, so they pass through.
        // This test verifies Layer 2 doesn't crash on C1 input.
        let input = b"before\x90after";
        let output = EscapeFilter::filter_for_replay(input);
        // Output should at least contain the text portions
        assert!(output.starts_with(b"before"));
        assert!(output.ends_with(b"after"));
    }

    #[test]
    fn test_save_restore_cursor_preserved() {
        let input = b"\x1b[s\x1b[5;10Hhello\x1b[u";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_scroll_preserved() {
        // Scroll up/down
        let input = b"\x1b[3S\x1b[2T";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_mixed_safe_and_unsafe() {
        // SGR + DA query + text + mode set
        let input = b"\x1b[1mhello\x1b[c\x1b[?2004h world\x1b[0m";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"\x1b[1mhello world\x1b[0m");
    }

    #[test]
    fn test_window_operations_stripped() {
        // Window resize request
        let input = b"before\x1b[8;40;120tafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }

    #[test]
    fn test_reverse_index_preserved() {
        let input = b"line1\x1bMline0";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_empty_input() {
        let output = EscapeFilter::filter_for_replay(b"");
        assert!(output.is_empty());
    }

    #[test]
    fn test_dec_save_restore_cursor() {
        // ESC 7 (save) and ESC 8 (restore) — DEC style
        let input = b"\x1b7\x1b[10;20H\x1b8";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_256_color_preserved() {
        // 256-color foreground
        let input = b"\x1b[38;5;196mred\x1b[0m";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_24bit_color_preserved() {
        // 24-bit RGB foreground
        let input = b"\x1b[38;2;255;128;0morange\x1b[0m";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_soft_reset_stripped() {
        // DECSTR soft reset — unsafe
        let input = b"before\x1b[!pafter";
        let output = EscapeFilter::filter_for_replay(input);
        assert_eq!(output, b"beforeafter");
    }
}
