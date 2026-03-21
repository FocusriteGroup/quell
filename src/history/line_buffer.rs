use std::collections::VecDeque;
use std::time::Instant;
use tracing::{debug, trace};

/// Type of event that produced a history entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistoryEventType {
    /// Normal passthrough output
    Output,
    /// Output from within a DEC mode 2026 sync block
    SyncBlock,
    /// Boundary marker: a full redraw was detected
    FullRedrawBoundary,
}

/// A single history entry with metadata.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 2 — fields used for structured output + replay
pub struct HistoryEntry {
    pub line: Vec<u8>,
    pub timestamp: Instant,
    pub event_type: HistoryEventType,
}

/// Circular line buffer for scrollback history.
///
/// Stores up to `max_lines` lines of terminal output. When the limit
/// is reached, the oldest lines are dropped (FIFO). Lines are stored
/// as raw bytes including any ANSI escape sequences that passed filtering.
pub struct LineBuffer {
    lines: VecDeque<HistoryEntry>,
    max_lines: usize,
    /// Partial line buffer for data that doesn't end with a newline
    partial: Vec<u8>,
    /// Event type for the current partial line
    partial_event_type: HistoryEventType,

    // Metrics
    total_lines_added: u64,
    total_lines_dropped: u64,
}

impl LineBuffer {
    pub fn new(max_lines: usize) -> Self {
        debug!(max_lines, "initializing line buffer");
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1024)),
            max_lines,
            partial: Vec::new(),
            partial_event_type: HistoryEventType::Output,
            total_lines_added: 0,
            total_lines_dropped: 0,
        }
    }

    /// Push raw bytes into the buffer, splitting on newlines.
    pub fn push(&mut self, data: &[u8], event_type: HistoryEventType) {
        self.partial.extend_from_slice(data);
        self.partial_event_type = event_type;

        // Split on newlines
        while let Some(newline_pos) = self.partial.iter().position(|&b| b == b'\n') {
            let line = self.partial[..newline_pos].to_vec();
            self.partial = self.partial[newline_pos + 1..].to_vec();
            self.add_line(HistoryEntry {
                line,
                timestamp: Instant::now(),
                event_type: self.partial_event_type,
            });
        }
    }

    /// Insert a boundary marker entry (e.g., full-redraw boundary).
    pub fn insert_boundary(&mut self, event_type: HistoryEventType) {
        self.add_line(HistoryEntry {
            line: Vec::new(),
            timestamp: Instant::now(),
            event_type,
        });
    }

    /// Add a complete entry to the buffer
    fn add_line(&mut self, entry: HistoryEntry) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.total_lines_dropped += 1;
        }
        self.lines.push_back(entry);
        self.total_lines_added += 1;

        if self.total_lines_added.is_multiple_of(10_000) {
            trace!(
                total_added = self.total_lines_added,
                total_dropped = self.total_lines_dropped,
                current_size = self.lines.len(),
                "line buffer milestone"
            );
        }
    }

    /// Clear all stored history
    #[allow(dead_code)] // Phase 2 — used for session management
    pub fn clear(&mut self) {
        debug!(
            lines_cleared = self.lines.len(),
            "clearing line buffer"
        );
        self.lines.clear();
        self.partial.clear();
    }

    /// Number of complete lines stored
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the buffer is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get all entries (for replay)
    #[allow(dead_code)] // Phase 2 — used for structured output replay
    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.lines.iter()
    }

    /// Get all lines as byte slices (convenience for simple access)
    #[allow(dead_code)]
    pub fn lines(&self) -> impl Iterator<Item = &[u8]> {
        self.lines.iter().map(|e| e.line.as_slice())
    }

    /// Get the last N entries
    #[allow(dead_code)]
    pub fn tail(&self, n: usize) -> impl Iterator<Item = &HistoryEntry> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip)
    }

    /// Returns accumulated metrics
    #[allow(dead_code)] // Phase 2 — used for status bar metrics
    pub fn metrics(&self) -> LineBufferMetrics {
        LineBufferMetrics {
            total_lines_added: self.total_lines_added,
            total_lines_dropped: self.total_lines_dropped,
            current_size: self.lines.len(),
            max_lines: self.max_lines,
        }
    }
}

/// Metrics from the line buffer
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 2
pub struct LineBufferMetrics {
    pub total_lines_added: u64,
    pub total_lines_dropped: u64,
    pub current_size: usize,
    pub max_lines: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let buf = LineBuffer::new(100);
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_push_single_line() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"hello world\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.lines().next().unwrap(), b"hello world");
    }

    #[test]
    fn test_push_multiple_lines() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"line 1\nline 2\nline 3\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 3);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"line 1");
        assert_eq!(lines[1], b"line 2");
        assert_eq!(lines[2], b"line 3");
    }

    #[test]
    fn test_push_partial_line() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"partial", HistoryEventType::Output);
        assert_eq!(buf.len(), 0); // Not yet a complete line

        buf.push(b" line\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.lines().next().unwrap(), b"partial line");
    }

    #[test]
    fn test_push_across_multiple_calls() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"hello ", HistoryEventType::Output);
        buf.push(b"world\nfoo ", HistoryEventType::Output);
        buf.push(b"bar\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 2);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"hello world");
        assert_eq!(lines[1], b"foo bar");
    }

    #[test]
    fn test_circular_eviction() {
        let mut buf = LineBuffer::new(3);
        buf.push(b"line 1\nline 2\nline 3\nline 4\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 3);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"line 2");
        assert_eq!(lines[1], b"line 3");
        assert_eq!(lines[2], b"line 4");
    }

    #[test]
    fn test_clear() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"line 1\nline 2\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 2);

        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_tail() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"a\nb\nc\nd\ne\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 5);

        let tail: Vec<&[u8]> = buf.tail(3).map(|e| e.line.as_slice()).collect();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0], b"c");
        assert_eq!(tail[1], b"d");
        assert_eq!(tail[2], b"e");
    }

    #[test]
    fn test_tail_more_than_available() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"a\nb\n", HistoryEventType::Output);

        let tail: Vec<_> = buf.tail(10).collect();
        assert_eq!(tail.len(), 2);
    }

    #[test]
    fn test_metrics() {
        let mut buf = LineBuffer::new(3);
        buf.push(b"a\nb\nc\nd\ne\n", HistoryEventType::Output);

        let metrics = buf.metrics();
        assert_eq!(metrics.total_lines_added, 5);
        assert_eq!(metrics.total_lines_dropped, 2);
        assert_eq!(metrics.current_size, 3);
        assert_eq!(metrics.max_lines, 3);
    }

    #[test]
    fn test_empty_lines() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"\n\n\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 3);

        for line in buf.lines() {
            assert!(line.is_empty());
        }
    }

    #[test]
    fn test_lines_with_ansi() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"\x1b[31mred text\x1b[0m\n\x1b[1mbold\x1b[0m\n", HistoryEventType::Output);
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.lines().next().unwrap(), b"\x1b[31mred text\x1b[0m");
    }

    #[test]
    fn test_event_type_preserved() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"normal line\n", HistoryEventType::Output);
        buf.push(b"sync line\n", HistoryEventType::SyncBlock);

        let entries: Vec<_> = buf.entries().collect();
        assert_eq!(entries[0].event_type, HistoryEventType::Output);
        assert_eq!(entries[1].event_type, HistoryEventType::SyncBlock);
    }

    #[test]
    fn test_insert_boundary() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"before\n", HistoryEventType::Output);
        buf.insert_boundary(HistoryEventType::FullRedrawBoundary);
        buf.push(b"after\n", HistoryEventType::SyncBlock);

        assert_eq!(buf.len(), 3);
        let entries: Vec<_> = buf.entries().collect();
        assert_eq!(entries[0].event_type, HistoryEventType::Output);
        assert_eq!(entries[1].event_type, HistoryEventType::FullRedrawBoundary);
        assert!(entries[1].line.is_empty());
        assert_eq!(entries[2].event_type, HistoryEventType::SyncBlock);
    }

    #[test]
    fn test_timestamps_monotonic() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"a\nb\nc\n", HistoryEventType::Output);

        let entries: Vec<_> = buf.entries().collect();
        for window in entries.windows(2) {
            assert!(window[1].timestamp >= window[0].timestamp);
        }
    }
}
