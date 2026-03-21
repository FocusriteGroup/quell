// Replay tests — feed synthetic VT fixtures through the proxy pipeline
// and assert invariants on the output.
//
// These tests exercise the same processing chain as the live proxy:
//   OutputFilter → SyncBlockDetector → strip_clear_screen
// but without ConPTY, threads, or real I/O.

use quell::history::OutputFilter;
use quell::proxy::strip_clear_screen;
use quell::vt::{SyncBlockDetector, SyncEvent};

// ── Replay infrastructure ───────────────────────────────────────────

struct ReplayResult {
    output: Vec<u8>,
    input_bytes: usize,
    #[allow(dead_code)] // Read in feature-gated fixture replay test
    sync_block_count: usize,
    full_redraw_count: usize,
    osc52_stripped: u64,
    links_stripped: u64,
}

/// Replay chunks through the proxy pipeline, collecting output and metrics.
fn replay_pipeline(chunks: &[&[u8]]) -> ReplayResult {
    let mut output_filter = OutputFilter::new();
    let mut detector = SyncBlockDetector::new();
    let mut output = Vec::new();
    let mut input_bytes = 0usize;
    let mut sync_block_count = 0usize;
    let mut full_redraw_count = 0usize;

    for chunk in chunks {
        input_bytes += chunk.len();
        let filtered = output_filter.filter(chunk);
        let filtered_owned = filtered.to_vec();
        let events = detector.process(&filtered_owned);

        let mut has_full_redraw = false;
        for event in &events {
            if let SyncEvent::SyncBlock {
                is_full_redraw: true,
                ..
            } = event
            {
                has_full_redraw = true;
            }
        }

        if has_full_redraw {
            for event in &events {
                match event {
                    SyncEvent::PassThrough(bytes) => {
                        output.extend_from_slice(bytes);
                    }
                    SyncEvent::SyncBlock { data, is_full_redraw } => {
                        if *is_full_redraw {
                            sync_block_count += 1;
                            full_redraw_count += 1;
                            let stripped = strip_clear_screen(data);
                            // Re-wrap in BSU/ESU
                            output.extend_from_slice(b"\x1b[?2026h");
                            output.extend_from_slice(&stripped);
                            output.extend_from_slice(b"\x1b[?2026l");
                        } else if memchr::memmem::find(data, b"\x1b[2J").is_some() {
                            sync_block_count += 1;
                            let stripped = strip_clear_screen(data);
                            output.extend_from_slice(b"\x1b[?2026h");
                            output.extend_from_slice(&stripped);
                            output.extend_from_slice(b"\x1b[?2026l");
                        } else {
                            sync_block_count += 1;
                            output.extend_from_slice(b"\x1b[?2026h");
                            output.extend_from_slice(data);
                            output.extend_from_slice(b"\x1b[?2026l");
                        }
                    }
                }
            }
        } else {
            for event in &events {
                match event {
                    SyncEvent::PassThrough(bytes) => {
                        output.extend_from_slice(bytes);
                    }
                    SyncEvent::SyncBlock { data, .. } => {
                        sync_block_count += 1;
                        output.extend_from_slice(&filtered_owned);
                        // Avoid double-counting: the filtered_owned already
                        // contains the sync block data with BSU/ESU
                        let _ = data;
                        break;
                    }
                }
            }
        }
    }

    let metrics = output_filter.metrics();

    ReplayResult {
        output,
        input_bytes,
        sync_block_count,
        full_redraw_count,
        osc52_stripped: metrics.osc52_stripped,
        links_stripped: metrics.links_stripped,
    }
}

// ── Synthetic fixtures ──────────────────────────────────────────────

fn fixture_with_osc52() -> Vec<&'static [u8]> {
    vec![
        b"Normal text before\r\n",
        b"\x1b]52;c;SGVsbG8gV29ybGQ=\x07",
        b"Normal text after\r\n",
        b"\x1b]52;p;c2VjcmV0\x1b\\",
        b"Final line\r\n",
    ]
}

fn fixture_with_full_redraws() -> Vec<&'static [u8]> {
    vec![
        b"Preamble text\r\n",
        // Full-redraw sync block: BSU + clear screen + cursor home + content + ESU
        b"\x1b[?2026h\x1b[2J\x1b[HLine 1 of redraw\r\nLine 2 of redraw\r\n\x1b[?2026l",
        b"Interstitial text\r\n",
        // Another full-redraw
        b"\x1b[?2026h\x1b[2J\x1b[1;1HUpdated line 1\r\nUpdated line 2\r\n\x1b[?2026l",
    ]
}

fn fixture_with_unicode() -> Vec<&'static [u8]> {
    vec![
        "┌──────────┐\r\n".as_bytes(),
        "│ Hello 🌍 │\r\n".as_bytes(),
        "└──────────┘\r\n".as_bytes(),
        "中文测试 日本語テスト\r\n".as_bytes(),
        "❯ prompt\r\n".as_bytes(),
    ]
}

fn fixture_with_osc8_links() -> Vec<&'static [u8]> {
    vec![
        // Allowed: https link
        b"\x1b]8;;https://example.com\x07click here\x1b]8;;\x07\r\n",
        // Blocked: ssh link
        b"\x1b]8;;ssh://evil.com\x07evil link\x1b]8;;\x07\r\n",
        // Allowed: file link
        b"\x1b]8;;file:///tmp/test.rs\x07test.rs\x1b]8;;\x07\r\n",
    ]
}

fn fixture_mixed_streaming() -> Vec<&'static [u8]> {
    vec![
        b"\x1b[?2004h",                                  // Bracketed paste mode
        b"\x1b]2;Claude Code\x07",                       // Window title
        b"Streaming text chunk 1...\r\n",
        b"\x1b[36mcolored output\x1b[0m\r\n",
        b"\x1b]52;c;Y2xpcGJvYXJk\x07",                  // OSC 52 clipboard
        b"\x1b[?2026h\x1b[2J\x1b[HFull redraw content\r\n\x1b[?2026l",
        b"More streaming...\r\n",
        b"\x1b[?2026hNormal sync block\r\n\x1b[?2026l",
        b"Final text\r\n",
    ]
}

// ── Assertion tests ─────────────────────────────────────────────────

#[test]
fn test_replay_no_clear_screen_leaks() {
    let result = replay_pipeline(&fixture_with_full_redraws());

    // No bare CSI 2J should appear in final output
    assert!(
        memchr::memmem::find(&result.output, b"\x1b[2J").is_none(),
        "clear-screen sequence leaked through to output"
    );
}

#[test]
fn test_replay_osc52_stripped() {
    let result = replay_pipeline(&fixture_with_osc52());

    // No OSC 52 in output
    assert!(
        memchr::memmem::find(&result.output, b"\x1b]52;").is_none(),
        "OSC 52 leaked through to output"
    );

    // Verify stripping happened
    assert!(
        result.osc52_stripped > 0,
        "expected OSC 52 sequences to be counted as stripped"
    );

    // Normal text preserved
    assert!(
        memchr::memmem::find(&result.output, b"Normal text before").is_some(),
        "normal text before OSC 52 was lost"
    );
    assert!(
        memchr::memmem::find(&result.output, b"Normal text after").is_some(),
        "normal text after OSC 52 was lost"
    );
}

#[test]
fn test_replay_osc8_whitelist() {
    let result = replay_pipeline(&fixture_with_osc8_links());

    // HTTPS link preserved
    assert!(
        memchr::memmem::find(&result.output, b"https://example.com").is_some(),
        "https link was incorrectly stripped"
    );

    // SSH link stripped (only the OSC 8 wrapper, visible text preserved)
    assert!(
        memchr::memmem::find(&result.output, b"ssh://evil.com").is_none(),
        "ssh link was not stripped"
    );

    assert!(
        result.links_stripped > 0,
        "expected blocked links to be counted"
    );

    // Visible text from blocked link still present
    assert!(
        memchr::memmem::find(&result.output, b"evil link").is_some(),
        "visible text from blocked link was lost"
    );
}

#[test]
fn test_replay_unicode_preserved() {
    let result = replay_pipeline(&fixture_with_unicode());

    // Box drawing
    assert!(
        memchr::memmem::find(&result.output, "┌──────────┐".as_bytes()).is_some(),
        "box-drawing top border lost"
    );
    assert!(
        memchr::memmem::find(&result.output, "└──────────┘".as_bytes()).is_some(),
        "box-drawing bottom border lost"
    );

    // Emoji
    assert!(
        memchr::memmem::find(&result.output, "🌍".as_bytes()).is_some(),
        "emoji was lost"
    );

    // CJK
    assert!(
        memchr::memmem::find(&result.output, "中文测试".as_bytes()).is_some(),
        "CJK text was lost"
    );

    // Prompt character
    assert!(
        memchr::memmem::find(&result.output, "❯".as_bytes()).is_some(),
        "prompt character was lost"
    );
}

#[test]
fn test_replay_sync_block_count() {
    let result = replay_pipeline(&fixture_with_full_redraws());

    assert_eq!(
        result.full_redraw_count, 2,
        "expected 2 full-redraw sync blocks"
    );
}

#[test]
fn test_replay_output_bounds() {
    let result = replay_pipeline(&fixture_mixed_streaming());

    // Output should be within reasonable bounds of input
    // Not too small (data lost) or too large (data duplicated)
    let ratio = result.output.len() as f64 / result.input_bytes as f64;
    assert!(
        ratio >= 0.3,
        "output too small relative to input ({} vs {} bytes, ratio {:.2})",
        result.output.len(),
        result.input_bytes,
        ratio
    );
    assert!(
        ratio <= 3.0,
        "output too large relative to input ({} vs {} bytes, ratio {:.2})",
        result.output.len(),
        result.input_bytes,
        ratio
    );
}

#[test]
fn test_replay_mixed_streaming_integrity() {
    let result = replay_pipeline(&fixture_mixed_streaming());

    // OSC 52 stripped
    assert!(
        memchr::memmem::find(&result.output, b"\x1b]52;").is_none(),
        "OSC 52 leaked in mixed stream"
    );

    // Clear screen stripped from full-redraw block
    assert!(
        memchr::memmem::find(&result.output, b"\x1b[2J").is_none(),
        "clear-screen leaked in mixed stream"
    );

    // Normal content preserved
    assert!(
        memchr::memmem::find(&result.output, b"Streaming text chunk 1").is_some(),
        "streaming text lost"
    );
    assert!(
        memchr::memmem::find(&result.output, b"Final text").is_some(),
        "final text lost"
    );
}

// ── Regression: sync-aware ESC[2J filtering ─────────────────────────

#[test]
fn test_replay_full_redraw_detected_after_64kb() {
    // Regression: ESC[2J inside sync blocks must not be stripped
    // even after 64KB of output (the old threshold-based behavior).
    // The output filter now tracks BSU/ESU state and only strips
    // ESC[2J outside sync blocks.
    let preamble = vec![b'x'; 70_000]; // > 64KB
    let full_redraw =
        b"\x1b[?2026h\x1b[2J\x1b[HRedraw content\r\n\x1b[?2026l";

    let result = replay_pipeline(&[&preamble, full_redraw]);
    assert_eq!(
        result.full_redraw_count, 1,
        "full-redraw sync block after 64KB must still be detected"
    );
}

#[test]
fn test_replay_clear_screen_stripped_outside_sync_after_startup() {
    // ESC[2J outside sync blocks is stripped after the startup grace
    // period (first 2 allowed). The new approach is sync-aware with
    // a small startup allowance, not byte-threshold based.
    let result = replay_pipeline(&[
        b"\x1b[2J",  // startup grace 1 — allowed
        b"\x1b[2J",  // startup grace 2 — allowed
        b"\x1b[2J",  // 3rd outside sync block — stripped
    ]);
    // The 3rd ESC[2J should be stripped; first 2 pass through but
    // are pass-through data (not in sync blocks), so they appear in output
    assert_eq!(
        memchr::memmem::find_iter(&result.output, b"\x1b[2J").count(),
        2,
        "only startup grace ESC[2J should pass through"
    );
}

// ── Fixture file replay (requires `recording` feature for read_vtcap) ───

#[cfg(feature = "recording")]
#[test]
fn test_replay_fixture_files() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let vtcap_files: Vec<_> = std::fs::read_dir(&fixture_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "vtcap")
        })
        .collect();

    if vtcap_files.is_empty() {
        eprintln!("no .vtcap fixture files found — skipping file replay tests");
        return;
    }

    for entry in vtcap_files {
        let path = entry.path();
        eprintln!("replaying fixture: {}", path.display());

        let (header, chunks) =
            quell::proxy::recorder::read_vtcap(&path).expect("failed to read vtcap fixture");

        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.data.as_slice()).collect();
        let result = replay_pipeline(&chunk_refs);

        // Basic invariants for any recording
        assert!(
            memchr::memmem::find(&result.output, b"\x1b]52;").is_none(),
            "OSC 52 leaked in fixture {}",
            path.display()
        );

        assert!(
            !result.output.is_empty() || chunks.is_empty(),
            "non-empty fixture {} produced empty output",
            path.display()
        );

        eprintln!(
            "  {} chunks, {} input bytes, {} output bytes, {} sync blocks",
            chunks.len(),
            result.input_bytes,
            result.output.len(),
            result.sync_block_count
        );

        let _ = header; // used implicitly via read_vtcap validation
    }
}
