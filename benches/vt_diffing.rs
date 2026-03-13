use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Benchmark: VT100 emulator feed + differential rendering
///
/// Measures how fast we can process typical Claude Code output
/// and compute screen diffs.
fn bench_diff_rendering(c: &mut Criterion) {
    // TODO: Use captured Claude Code output samples

    let mut group = c.benchmark_group("vt_diffing");

    group.bench_function("feed_small_update", |b| {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"initial content on screen\r\n");
        let prev = parser.screen().clone();

        b.iter(|| {
            parser.process(black_box(b"\x1b[2;1Hupdated line content"));
            let _diff = parser.screen().contents_diff(&prev);
        });
    });

    group.bench_function("feed_full_redraw_24x80", |b| {
        let mut parser = vt100::Parser::new(24, 80, 0);
        // Simulate a full 24x80 screen write
        let mut full_screen = Vec::with_capacity(24 * 82);
        for i in 0..24 {
            full_screen.extend_from_slice(format!("Line {:3} {}\r\n", i, "x".repeat(74)).as_bytes());
        }

        parser.process(&full_screen);
        let prev = parser.screen().clone();

        b.iter(|| {
            parser.process(black_box(b"\x1b[H")); // cursor home
            parser.process(black_box(&full_screen));
            let _diff = parser.screen().contents_diff(&prev);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_diff_rendering);
criterion_main!(benches);
