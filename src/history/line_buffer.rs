use std::collections::VecDeque;
use tracing::{debug, trace};

/// Circular line buffer for scrollback history.
///
/// Stores up to `max_lines` lines of terminal output. When the limit
/// is reached, the oldest lines are dropped (FIFO). Lines are stored
/// as raw bytes including any ANSI escape sequences that passed filtering.
pub struct LineBuffer {
    lines: VecDeque<Vec<u8>>,
    max_lines: usize,
    /// Partial line buffer for data that doesn't end with a newline
    partial: Vec<u8>,

    // Metrics
    total_lines_added: u64,
    total_lines_dropped: u64,
}

impl LineBuffer {
    pub fn new(max_lines: usize) -> Self {
        debug!(max_lines, "initializing line buffer");
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1024)), // Don't pre-allocate everything
            max_lines,
            partial: Vec::new(),
            total_lines_added: 0,
            total_lines_dropped: 0,
        }
    }

    /// Push raw bytes into the buffer, splitting on newlines.
    pub fn push(&mut self, data: &[u8]) {
        self.partial.extend_from_slice(data);

        // Split on newlines
        while let Some(newline_pos) = self.partial.iter().position(|&b| b == b'\n') {
            let line = self.partial[..newline_pos].to_vec();
            self.partial = self.partial[newline_pos + 1..].to_vec();
            self.add_line(line);
        }
    }

    /// Add a complete line to the buffer
    fn add_line(&mut self, line: Vec<u8>) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.total_lines_dropped += 1;
        }
        self.lines.push_back(line);
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
    pub fn clear(&mut self) {
        debug!(
            lines_cleared = self.lines.len(),
            "clearing line buffer"
        );
        self.lines.clear();
        self.partial.clear();
    }

    /// Number of complete lines stored
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get all lines as byte slices (for replay)
    pub fn lines(&self) -> impl Iterator<Item = &[u8]> {
        self.lines.iter().map(|l| l.as_slice())
    }

    /// Get the last N lines
    pub fn tail(&self, n: usize) -> impl Iterator<Item = &[u8]> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip).map(|l| l.as_slice())
    }

    /// Returns accumulated metrics
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
        buf.push(b"hello world\n");
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.lines().next().unwrap(), b"hello world");
    }

    #[test]
    fn test_push_multiple_lines() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"line 1\nline 2\nline 3\n");
        assert_eq!(buf.len(), 3);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"line 1");
        assert_eq!(lines[1], b"line 2");
        assert_eq!(lines[2], b"line 3");
    }

    #[test]
    fn test_push_partial_line() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"partial");
        assert_eq!(buf.len(), 0); // Not yet a complete line

        buf.push(b" line\n");
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.lines().next().unwrap(), b"partial line");
    }

    #[test]
    fn test_push_across_multiple_calls() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"hello ");
        buf.push(b"world\nfoo ");
        buf.push(b"bar\n");
        assert_eq!(buf.len(), 2);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"hello world");
        assert_eq!(lines[1], b"foo bar");
    }

    #[test]
    fn test_circular_eviction() {
        let mut buf = LineBuffer::new(3);
        buf.push(b"line 1\nline 2\nline 3\nline 4\n");
        assert_eq!(buf.len(), 3);

        let lines: Vec<&[u8]> = buf.lines().collect();
        assert_eq!(lines[0], b"line 2");
        assert_eq!(lines[1], b"line 3");
        assert_eq!(lines[2], b"line 4");
    }

    #[test]
    fn test_clear() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"line 1\nline 2\n");
        assert_eq!(buf.len(), 2);

        buf.clear();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_tail() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"a\nb\nc\nd\ne\n");
        assert_eq!(buf.len(), 5);

        let tail: Vec<&[u8]> = buf.tail(3).collect();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0], b"c");
        assert_eq!(tail[1], b"d");
        assert_eq!(tail[2], b"e");
    }

    #[test]
    fn test_tail_more_than_available() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"a\nb\n");

        let tail: Vec<&[u8]> = buf.tail(10).collect();
        assert_eq!(tail.len(), 2);
    }

    #[test]
    fn test_metrics() {
        let mut buf = LineBuffer::new(3);
        buf.push(b"a\nb\nc\nd\ne\n");

        let metrics = buf.metrics();
        assert_eq!(metrics.total_lines_added, 5);
        assert_eq!(metrics.total_lines_dropped, 2);
        assert_eq!(metrics.current_size, 3);
        assert_eq!(metrics.max_lines, 3);
    }

    #[test]
    fn test_empty_lines() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"\n\n\n");
        assert_eq!(buf.len(), 3);

        for line in buf.lines() {
            assert!(line.is_empty());
        }
    }

    #[test]
    fn test_lines_with_ansi() {
        let mut buf = LineBuffer::new(100);
        buf.push(b"\x1b[31mred text\x1b[0m\n\x1b[1mbold\x1b[0m\n");
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.lines().next().unwrap(), b"\x1b[31mred text\x1b[0m");
    }
}
