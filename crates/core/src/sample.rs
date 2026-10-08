//! Deterministic synthetic tree – used by the web demo and benchmarks.

use crate::tree::{Tree, TreeBuilder};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn generate(seed: u64, target_nodes: usize) -> Tree {
    let mut r = Rng(seed.max(1) | 1);
    let mut b = TreeBuilder::new("Demo (C:)");
    let exts = [
        "dll", "exe", "mp4", "jpg", "png", "txt", "zip", "pdf", "mp3", "log", "cs", "rs", "js",
        "json", "iso", "bin",
    ];
    let tops = [
        "Windows",
        "Program Files",
        "Users",
        "Games",
        "Projects",
        "Videos",
        "Downloads",
        "Temp",
        "Backups",
    ];
    let mut dirs = vec![0u32];
    for t in tops {
        dirs.push(b.add_dir(0, t));
    }
    while b.len() < target_nodes {
        let parent = dirs[(r.next() as usize) % dirs.len()];
        if r.f() < 0.05 {
            let id = b.add_dir(parent, &format!("folder_{}", r.next() % 100000));
            dirs.push(id);
        } else {
            let e = exts[(r.next() as usize) % exts.len()];
            // log-normal-ish sizes: many small, a few huge
            let size =
                (2f64.powf(8.0 + r.f() * 20.0 + if r.f() < 0.02 { 8.0 } else { 0.0 })) as u64;
            b.add_file(
                parent,
                &format!("file_{}.{e}", r.next() % 1000000),
                size,
                1_600_000_000 + (r.next() % 100_000_000) as u32,
            );
        }
    }
    b.finish()
}
