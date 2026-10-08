//! Faithful re-implementation of SpaceMonger 1.4's layout (reverse-engineered, see research/FINDINGS.md):
//! recursive *balanced binary partition* – items (sorted descending) are greedily split into two groups
//! of near-equal total size, the rectangle is cut across its (bias-weighted) longer side in that ratio,
//! and each half recurses. Folders reserve a 12 px title strip and a 3 px border.

use crate::layout::{Cell, CellKind};
use crate::tree::{NodeId, Tree, NONE};

#[derive(Clone, Copy, Debug)]
pub struct ClassicOptions {
    pub min_w: i32,
    pub min_h: i32,
    /// -20..=20; >0 favours cuts across the width (tall bars), <0 the opposite.
    pub bias: i32,
}

/// Density presets from the original table at 0x4249b8: Too Many … Too Few.
pub const DENSITY: [(i32, i32); 6] = [(96, 64), (64, 48), (48, 32), (32, 24), (24, 16), (16, 12)];

struct Ctx<'a> {
    tree: &'a Tree,
    o: ClassicOptions,
    free: u64,
    out: Vec<Cell>,
    base_depth: u16,
}

pub fn layout_classic(tree: &Tree, root: NodeId, rect: [f32; 4], free: Option<u64>, o: &ClassicOptions) -> Vec<Cell> {
    let mut cx = Ctx { tree, o: *o, free: free.unwrap_or(0), out: Vec::with_capacity(4096), base_depth: 0 };
    let (w, h) = (rect[2] as i32 - 1, rect[3] as i32 - 1);
    if w > 0 && h > 0 {
        cx.folder(rect[0] as i32, rect[1] as i32, w, h, root, 0, root == 0);
    }
    cx.out
}

impl Ctx<'_> {
    fn folder(&mut self, x: i32, y: i32, w: i32, h: i32, node: NodeId, depth: u16, with_free: bool) {
        let kids = self.tree.children(node);
        let mut idx: Vec<u32> = (0..kids.len() as u32).collect();
        if with_free && self.free > 0 {
            // the free-space entry sorts among the others
            let pos = idx.iter().position(|&i| self.tree.node(kids[i as usize]).size < self.free).unwrap_or(idx.len());
            idx.insert(pos, u32::MAX);
        }
        self.split(x, y, w, h, node, &idx, depth);
    }

    fn size_of(&self, node: NodeId, i: u32) -> u64 {
        if i == u32::MAX { self.free } else { self.tree.node(self.tree.children(node)[i as usize]).size }
    }

    fn split(&mut self, x: i32, y: i32, w: i32, h: i32, node: NodeId, idx: &[u32], depth: u16) {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let (mut sa, mut sb) = (0u64, 0u64);
        for &i in idx {
            let s = self.size_of(node, i);
            if s == 0 {
                continue;
            }
            if sa > sb {
                b.push(i);
                sb += s;
            } else {
                a.push(i);
                sa += s;
            }
        }
        let total = sa + sb;
        if total == 0 {
            return;
        }
        let bias = self.o.bias.clamp(-20, 20);
        let fw = 8 + bias.max(0) as i64;
        let fh = 8 + (-bias).max(0) as i64;
        let (ra, rb);
        if (w as i64) * fw / 8 > (h as i64) * fh / 8 {
            let wa = (w as u64 * sa / total) as i32;
            ra = (x, y, wa, h);
            rb = (x + wa, y, w - wa, h);
        } else {
            let ha = (h as u64 * sa / total) as i32;
            ra = (x, y, w, ha);
            rb = (x, y + ha, w, h - ha);
        }
        for (r, g, gs) in [(ra, &a, sa), (rb, &b, sb)] {
            if g.is_empty() {
                continue;
            }
            let ok = r.2 > self.o.min_w && r.3 > self.o.min_h;
            if g.len() > 1 && ok {
                self.split(r.0, r.1, r.2, r.3, node, g, depth);
            } else if ok {
                self.item(node, g[0], r, depth);
            } else {
                self.out.push(Cell { node, x: r.0 as f32, y: r.1 as f32, w: r.2 as f32, h: r.3 as f32, header: 0.0, depth, kind: CellKind::Lump, size: gs, count: g.len() as u32 });
            }
        }
    }

    fn item(&mut self, parent: NodeId, i: u32, r: (i32, i32, i32, i32), depth: u16) {
        let (x, y, w, h) = r;
        if i == u32::MAX {
            self.out.push(Cell { node: NONE, x: x as f32, y: y as f32, w: w as f32, h: h as f32, header: 0.0, depth, kind: CellKind::Free, size: self.free, count: 0 });
            return;
        }
        let id = self.tree.children(parent)[i as usize];
        let n = self.tree.node(id);
        if n.is_dir {
            let header = if h > 14 { 12.0 } else { 0.0 };
            self.out.push(Cell { node: id, x: x as f32, y: y as f32, w: w as f32, h: h as f32, header, depth, kind: CellKind::Dir, size: n.size, count: 0 });
            self.folder(x + 3, y + 12, w - 6, h - 15, id, depth + 1, false);
        } else {
            self.out.push(Cell { node: id, x: x as f32, y: y as f32, w: w as f32, h: h as f32, header: 0.0, depth, kind: CellKind::File, size: n.size, count: 0 });
        }
    }
}

impl Ctx<'_> {
    #[allow(dead_code)]
    fn _unused(&self) -> u16 {
        self.base_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::TreeBuilder;

    #[test]
    fn partitions_without_overlap() {
        let t = crate::sample::generate(3, 3000);
        let o = ClassicOptions { min_w: 16, min_h: 12, bias: 0 };
        let cells = layout_classic(&t, 0, [0.0, 0.0, 900.0, 600.0], Some(t.total_size() / 3), &o);
        assert!(cells.len() > 50);
        assert!(cells.iter().any(|c| c.kind == CellKind::Free));
        let leaves: Vec<_> = cells.iter().filter(|c| c.kind != CellKind::Dir).collect();
        for (i, a) in leaves.iter().enumerate() {
            assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= 900.0 && a.y + a.h <= 600.0);
            for b in &leaves[i + 1..] {
                let ox = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                let oy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                assert!(ox <= 0.001 || oy <= 0.001, "overlap {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn single_item_fills() {
        let mut b = TreeBuilder::new("r");
        b.add_file(0, "a", 10, 0);
        let t = b.finish();
        let c = layout_classic(&t, 0, [0.0, 0.0, 200.0, 100.0], None, &ClassicOptions { min_w: 16, min_h: 12, bias: 0 });
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].w, c[0].h), (199.0, 99.0));
    }
}
