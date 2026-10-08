use spacehunter_core::*;
use std::time::Instant;
fn main() {
    let t0 = Instant::now();
    let t = sample::generate(5, 2_000_000);
    println!(
        "generate+finish {} nodes: {:.0} ms",
        t.len(),
        t0.elapsed().as_secs_f64() * 1e3
    );
    let t1 = Instant::now();
    let c = layout_classic(
        &t,
        0,
        [0.0, 0.0, 1920.0, 1080.0],
        Some(1 << 40),
        &ClassicOptions {
            min_w: 32,
            min_h: 24,
            bias: 0,
        },
    );
    println!(
        "classic {} cells {:.1} ms",
        c.len(),
        t1.elapsed().as_secs_f64() * 1e3
    );
}
