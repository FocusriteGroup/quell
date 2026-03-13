use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Benchmark: Sync block detection throughput
///
/// Measures how fast we can scan for BSU/ESU markers in a byte stream.
fn bench_sync_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync_detection");

    group.bench_function("scan_1kb_no_markers", |b| {
        let data = vec![b'x'; 1024];
        let finder = memchr::memmem::Finder::new(b"\x1b[?2026h");
        b.iter(|| {
            let _ = finder.find(black_box(&data));
        });
    });

    group.bench_function("scan_1kb_with_marker_at_end", |b| {
        let mut data = vec![b'x'; 1024 - 8];
        data.extend_from_slice(b"\x1b[?2026h");
        let finder = memchr::memmem::Finder::new(b"\x1b[?2026h");
        b.iter(|| {
            let _ = finder.find(black_box(&data));
        });
    });

    group.bench_function("scan_64kb_typical_claude_output", |b| {
        // Simulate a mix of text and ANSI codes
        let mut data = Vec::with_capacity(65536);
        for _ in 0..100 {
            data.extend_from_slice(b"\x1b[?2026h");
            data.extend_from_slice(b"\x1b[2J\x1b[H");
            for j in 0..24 {
                data.extend_from_slice(
                    format!("\x1b[{};1H\x1b[32mLine {}\x1b[0m some text content here\r\n", j + 1, j).as_bytes()
                );
            }
            data.extend_from_slice(b"\x1b[?2026l");
        }

        let finder_start = memchr::memmem::Finder::new(b"\x1b[?2026h");
        let finder_end = memchr::memmem::Finder::new(b"\x1b[?2026l");

        b.iter(|| {
            let mut pos = 0;
            let mut count = 0u32;
            while pos < data.len() {
                if let Some(start) = finder_start.find(black_box(&data[pos..])) {
                    pos += start + 8;
                    if let Some(end) = finder_end.find(&data[pos..]) {
                        pos += end + 8;
                        count += 1;
                    }
                } else {
                    break;
                }
            }
            black_box(count);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sync_detection);
criterion_main!(benches);
