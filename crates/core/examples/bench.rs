//! `cargo run --release -p spacehunter-core --example bench -- <path>`
use spacehunter_core::{layout_classic, scan, ClassicOptions, Progress};
use std::{path::Path, sync::Arc, time::Instant};

fn main() {
    let p = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let t0 = Instant::now();
    let tree = scan::scan(
        Path::new(&p),
        scan::ScanOptions::default(),
        Arc::new(Progress::default()),
    );
    let dt = t0.elapsed();
    println!(
        "scan   : {} nodes, {} in {:.0} ms ({:.0} k entries/s)",
        tree.len(),
        spacehunter_core::fmt::size(tree.total_size()),
        dt.as_secs_f64() * 1e3,
        tree.len() as f64 / dt.as_secs_f64() / 1e3
    );
    let o = ClassicOptions {
        min_w: 32,
        min_h: 24,
        bias: 0,
    };
    let t1 = Instant::now();
    let n = (0..20)
        .map(|_| layout_classic(&tree, 0, [0.0, 0.0, 1920.0, 1080.0], None, &o).len())
        .last()
        .unwrap();
    println!(
        "classic: {n} cells, {:.2} ms/layout",
        t1.elapsed().as_secs_f64() * 1e3 / 20.0
    );
    let lo = spacehunter_core::LayoutOptions::default();
    let t2 = Instant::now();
    let n = (0..20)
        .map(|_| spacehunter_core::layout(&tree, 0, [0.0, 0.0, 1920.0, 1080.0], None, &lo).len())
        .last()
        .unwrap();
    println!(
        "squarif: {n} cells, {:.2} ms/layout",
        t2.elapsed().as_secs_f64() * 1e3 / 20.0
    );
}
