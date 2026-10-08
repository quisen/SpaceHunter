//! Squarified treemap layout with nested folder frames.
//!
//! Produces a flat list of [`Cell`]s in paint order (parents before children), so
//! rendering is a linear pass and hit-testing is "last cell containing the point".

use crate::tree::{NodeId, Tree, NONE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Dir,
    File,
    /// Many tiny entries merged into one block (`node` is the owning directory).
    Lump,
    /// Free space of the volume (`node` = NONE).
    Free,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub node: NodeId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Height of the title strip of a folder frame (0 if none).
    pub header: f32,
    pub depth: u16,
    pub kind: CellKind,
    pub size: u64,
    /// For `Lump`: number of merged entries.
    pub count: u32,
}

impl Cell {
    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    /// Maximum visible depth relative to the current root.
    pub max_depth: u16,
    /// Entries whose block would be smaller than this many pixels² are merged ("density").
    pub min_area: f32,
    /// Folder frames are not subdivided below this edge length.
    pub min_dir_edge: f32,
    /// -1 = favour tall (vertical) blocks, 0 = squarish, +1 = favour wide (horizontal) blocks.
    pub bias: f32,
    /// Title strip height for folder frames (0 = none).
    pub header: f32,
    /// Frame padding around the children of a folder.
    pub pad: f32,
    /// Minimum frame width/height to show a title strip.
    pub header_min_w: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            max_depth: u16::MAX,
            min_area: 36.0,
            min_dir_edge: 10.0,
            bias: 0.0,
            header: 14.0,
            pad: 2.0,
            header_min_w: 48.0,
        }
    }
}

struct Job {
    node: NodeId,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    depth: u16,
}

/// Lay out `root`'s subtree inside the rectangle (x, y, w, h). `free` adds a free-space block next to the root.
pub fn layout(
    tree: &Tree,
    root: NodeId,
    rect: [f32; 4],
    free: Option<u64>,
    o: &LayoutOptions,
) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::with_capacity(4096);
    let [x, y, w, h] = rect;
    let rn = tree.node(root);
    if w < 1.0 || h < 1.0 {
        return out;
    }
    let mut stack = vec![Job {
        node: root,
        x,
        y,
        w,
        h,
        depth: 0,
    }];
    // With free space, the root frame is split between the data and the free block.
    if let Some(free) = free.filter(|&f| f > 0 && rn.size > 0) {
        let total = rn.size + free;
        let (dw, dh) = if w >= h {
            (w * rn.size as f32 / total as f32, h)
        } else {
            (w, h * rn.size as f32 / total as f32)
        };
        stack[0].w = dw;
        stack[0].h = dh;
        let (fx, fy, fw, fh) = if w >= h {
            (x + dw, y, w - dw, h)
        } else {
            (x, y + dh, w, h - dh)
        };
        out.push(Cell {
            node: NONE,
            x: fx,
            y: fy,
            w: fw,
            h: fh,
            header: 0.0,
            depth: 0,
            kind: CellKind::Free,
            size: free,
            count: 0,
        });
    }
    let mut rows: Vec<(usize, usize)> = Vec::new();
    while let Some(j) = stack.pop() {
        let n = tree.node(j.node);
        let kids = tree.children(j.node);
        if !n.is_dir {
            out.push(Cell {
                node: j.node,
                x: j.x,
                y: j.y,
                w: j.w,
                h: j.h,
                header: 0.0,
                depth: j.depth,
                kind: CellKind::File,
                size: n.size,
                count: 0,
            });
            continue;
        }
        let can_split = j.depth < o.max_depth
            && j.w >= o.min_dir_edge
            && j.h >= o.min_dir_edge
            && !kids.is_empty()
            && n.size > 0;
        let header =
            if can_split && o.header > 0.0 && j.w >= o.header_min_w && j.h >= o.header * 2.5 {
                o.header
            } else {
                0.0
            };
        out.push(Cell {
            node: j.node,
            x: j.x,
            y: j.y,
            w: j.w,
            h: j.h,
            header,
            depth: j.depth,
            kind: CellKind::Dir,
            size: n.size,
            count: 0,
        });
        if !can_split {
            continue;
        }
        let pad = if j.w > o.pad * 6.0 && j.h > o.pad * 6.0 {
            o.pad
        } else {
            0.0
        };
        let (ix, iy) = (j.x + pad, j.y + pad + header);
        let (iw, ih) = (j.w - pad * 2.0, j.h - pad * 2.0 - header);
        if iw < 2.0 || ih < 2.0 {
            continue;
        }
        let scale = (iw * ih) as f64 / n.size as f64; // px² per byte
                                                      // How many leading children are big enough; the rest becomes one lump.
        let mut keep = kids.len();
        for (i, &c) in kids.iter().enumerate() {
            if (tree.node(c).size as f64 * scale) < o.min_area as f64 {
                keep = i;
                break;
            }
        }
        let lump_size: u64 = kids[keep..].iter().map(|&c| tree.node(c).size).sum();
        let lump_count = (kids.len() - keep) as u32;
        let mut sizes: Vec<f64> = kids[..keep]
            .iter()
            .map(|&c| tree.node(c).size as f64)
            .collect();
        if lump_count > 0 && lump_size > 0 {
            sizes.push(lump_size as f64);
        }
        if sizes.is_empty() {
            continue;
        }
        // Warp x by k so that "squarish" in warped space is wide/tall in real space.
        let k = 2f32.powf(-o.bias.clamp(-1.0, 1.0));
        let (ww, wh) = (iw * k, ih);
        let areas: Vec<f64> = {
            let t: f64 = sizes.iter().sum();
            sizes.iter().map(|s| s / t * (ww * wh) as f64).collect()
        };
        rows.clear();
        let placed = squarify(&areas, ww as f64, wh as f64, &mut rows);
        let base = out.len();
        let _ = base;
        for (idx, r) in placed.iter().enumerate() {
            // r: x,y,w,h in warped space
            let (rx, ry, rw, rh) = (
                ix + r[0] as f32 / k,
                iy + r[1] as f32,
                r[2] as f32 / k,
                r[3] as f32,
            );
            if idx < keep {
                stack.push(Job {
                    node: kids[idx],
                    x: rx,
                    y: ry,
                    w: rw,
                    h: rh,
                    depth: j.depth + 1,
                });
            } else {
                out.push(Cell {
                    node: j.node,
                    x: rx,
                    y: ry,
                    w: rw,
                    h: rh,
                    header: 0.0,
                    depth: j.depth + 1,
                    kind: CellKind::Lump,
                    size: lump_size,
                    count: lump_count,
                });
            }
        }
    }
    // The stack is LIFO, which already yields parents before their children for every branch.
    out
}

/// Classic squarified algorithm (Bruls et al.). `areas` must be sorted descending and sum to w*h.
fn squarify(areas: &[f64], w: f64, h: f64, _scratch: &mut Vec<(usize, usize)>) -> Vec<[f64; 4]> {
    let mut res = vec![[0.0; 4]; areas.len()];
    let (mut x, mut y, mut rw, mut rh) = (0.0, 0.0, w, h);
    let mut i = 0;
    while i < areas.len() {
        let short = rw.min(rh);
        // grow the row while the worst aspect ratio improves
        let mut j = i;
        let mut sum = 0.0;
        let mut best = f64::INFINITY;
        while j < areas.len() {
            let s2 = sum + areas[j];
            let (mn, mx) = (
                areas[i..=j]
                    .last()
                    .copied()
                    .unwrap_or(areas[j])
                    .min(areas[j]),
                areas[i],
            );
            let _ = mn;
            // worst ratio of the row: max(short²·max/s², s²/(short²·min))
            let min_a = areas[j];
            let worst = ((short * short * mx) / (s2 * s2)).max((s2 * s2) / (short * short * min_a));
            if worst > best {
                break;
            }
            best = worst;
            sum = s2;
            j += 1;
        }
        // row = areas[i..j]
        let thick = if short > 0.0 { sum / short } else { 0.0 };
        let mut off = 0.0;
        if rw >= rh {
            // vertical strip on the left, items stacked top→bottom
            for a in i..j {
                let len = if thick > 0.0 { areas[a] / thick } else { 0.0 };
                res[a] = [x, y + off, thick, len];
                off += len;
            }
            x += thick;
            rw -= thick;
        } else {
            for a in i..j {
                let len = if thick > 0.0 { areas[a] / thick } else { 0.0 };
                res[a] = [x + off, y, len, thick];
                off += len;
            }
            y += thick;
            rh -= thick;
        }
        i = j;
    }
    res
}

/// Deepest cell under the point (cells are in paint order).
pub fn hit_test(cells: &[Cell], px: f32, py: f32) -> Option<usize> {
    cells.iter().rposition(|c| c.contains(px, py))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::TreeBuilder;

    fn sample() -> Tree {
        let mut b = TreeBuilder::new("root");
        for d in 0..5 {
            let id = b.add_dir(0, &format!("d{d}"));
            for f in 0..20 {
                b.add_file(
                    id,
                    &format!("f{f}"),
                    1000 * (f as u64 + 1) * (d as u64 + 1),
                    0,
                );
            }
        }
        b.finish()
    }

    #[test]
    fn overview_preserves_sizes_and_drills_one_level_at_a_time() {
        let t = sample();
        let options = LayoutOptions {
            max_depth: 1,
            pad: 0.0,
            header: 0.0,
            min_area: 0.0,
            ..Default::default()
        };
        let cells = layout(&t, 0, [0.0, 0.0, 800.0, 600.0], None, &options);
        let children: Vec<_> = cells.iter().filter(|c| c.depth == 1).collect();
        assert_eq!(children.len(), 5);
        assert!(cells.iter().all(|c| c.depth <= 1));
        assert_eq!(children.iter().map(|c| c.size).sum::<u64>(), t.total_size());
        for c in children {
            assert!((c.w * c.h / 480000.0 - c.size as f32 / t.total_size() as f32).abs() < 0.001);
            let inner = layout(&t, c.node, [0.0, 0.0, 800.0, 600.0], None, &options);
            assert_eq!(
                inner.iter().filter(|c| c.kind == CellKind::File).count(),
                20
            );
        }
    }

    #[test]
    fn areas_are_proportional_and_inside() {
        let t = sample();
        let o = LayoutOptions {
            min_area: 0.0,
            ..Default::default()
        };
        let cells = layout(&t, 0, [0.0, 0.0, 800.0, 600.0], None, &o);
        let files: Vec<_> = cells.iter().filter(|c| c.kind == CellKind::File).collect();
        assert_eq!(files.len(), 100);
        for c in &cells {
            assert!(
                c.x >= -0.01 && c.y >= -0.01 && c.x + c.w <= 800.01 && c.y + c.h <= 600.01,
                "{c:?}"
            );
        }
        // overlap check between sibling files
        for (i, a) in files.iter().enumerate() {
            for b in &files[i + 1..] {
                let ox = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                let oy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                assert!(ox <= 0.01 || oy <= 0.01, "overlap {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn lumps_small_entries() {
        let t = sample();
        let o = LayoutOptions {
            min_area: 400.0,
            ..Default::default()
        };
        let cells = layout(&t, 0, [0.0, 0.0, 400.0, 300.0], None, &o);
        assert!(cells.iter().any(|c| c.kind == CellKind::Lump));
    }

    #[test]
    fn free_space_block() {
        let t = sample();
        let cells = layout(
            &t,
            0,
            [0.0, 0.0, 800.0, 600.0],
            Some(t.total_size()),
            &LayoutOptions::default(),
        );
        let f = cells.iter().find(|c| c.kind == CellKind::Free).unwrap();
        assert!((f.w * f.h - 800.0 * 600.0 / 2.0).abs() < 1.0);
    }
}
