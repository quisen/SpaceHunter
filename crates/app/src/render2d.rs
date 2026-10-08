//! 2D treemap painting: one cached gouraud-shaded mesh + a capped list of text labels.

use crate::colors::{to32, ColorCtx};
use crate::i18n::Language;
use eframe::egui::{self, epaint::Vertex, Color32, Mesh, Pos2, Rect};
use spacehunter_core::{fmt, Cell, CellKind, Tree};
use std::sync::Arc;

pub struct Label {
    pub clip: Rect,
    pub pos: Pos2,
    pub text: String,
    pub color: Color32,
    pub size: f32,
}

pub struct Scene {
    pub mesh: Arc<Mesh>,
    pub labels: Vec<Label>,
}

fn shade(c: [f32; 3], d: f32) -> Color32 {
    to32([c[0] + d, c[1] + d, c[2] + d])
}

fn quad(m: &mut Mesh, r: Rect, tl: Color32, tr: Color32, br: Color32, bl: Color32) {
    let i = m.vertices.len() as u32;
    let uv = egui::epaint::WHITE_UV;
    m.vertices.push(Vertex {
        pos: r.left_top(),
        uv,
        color: tl,
    });
    m.vertices.push(Vertex {
        pos: r.right_top(),
        uv,
        color: tr,
    });
    m.vertices.push(Vertex {
        pos: r.right_bottom(),
        uv,
        color: br,
    });
    m.vertices.push(Vertex {
        pos: r.left_bottom(),
        uv,
        color: bl,
    });
    m.indices
        .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
}

const CHAR_W: f32 = 5.6;

fn fit(text: &str, width: f32) -> Option<String> {
    let max = ((width - 4.0) / CHAR_W) as usize;
    if max < 3 {
        return None;
    }
    let n = text.chars().count();
    if n <= max {
        Some(text.to_owned())
    } else {
        let mut s: String = text.chars().take(max - 1).collect();
        s.push('…');
        Some(s)
    }
}

pub fn build(
    cells: &[Cell],
    tree: &Tree,
    cc: &ColorCtx,
    origin: Pos2,
    bevel: bool,
    labels_on: bool,
    lang: Language,
) -> Scene {
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(cells.len() * 4);
    mesh.indices.reserve(cells.len() * 6);
    let mut labels = Vec::new();
    let text_hi = if cc.dark {
        Color32::from_rgb(235, 238, 245)
    } else {
        Color32::from_rgb(20, 24, 32)
    };
    for c in cells {
        let r = Rect::from_min_size(
            Pos2::new(origin.x + c.x, origin.y + c.y),
            egui::vec2(c.w, c.h),
        );
        match c.kind {
            CellKind::Dir => {
                let f = cc.frame(c.depth);
                quad(&mut mesh, r, to32(f), to32(f), to32(f), to32(f));
                if c.header > 0.0 {
                    let h = cc.header(c.depth);
                    let hr = Rect::from_min_size(
                        r.min + egui::vec2(1.0, 1.0),
                        egui::vec2((c.w - 2.0).max(0.0), c.header - 1.0),
                    );
                    quad(
                        &mut mesh,
                        hr,
                        shade(h, 0.04),
                        shade(h, 0.04),
                        shade(h, -0.02),
                        shade(h, -0.02),
                    );
                    if labels_on && c.depth <= 1 && c.w >= 140.0 && c.h >= 70.0 && labels.len() < 24
                    {
                        let t = format!("{}  ·  {}", tree.name(c.node), fmt::size(c.size));
                        if let Some(t) = fit(&t, c.w - 4.0) {
                            labels.push(Label {
                                clip: r,
                                pos: hr.min + egui::vec2(3.0, 0.5),
                                text: t,
                                color: text_hi,
                                size: 11.0,
                            });
                        }
                    }
                }
            }
            _ => {
                let [base, light, dark] = cc.shades(tree, c);
                let g = if c.w > 2.5 && c.h > 2.5 { 0.5 } else { 0.0 };
                let rr = r.shrink(g);
                if rr.width() <= 0.0 || rr.height() <= 0.0 {
                    continue;
                }
                if bevel && c.w > 3.0 && c.h > 3.0 {
                    let mid = [
                        (light[0] + base[0]) / 2.0,
                        (light[1] + base[1]) / 2.0,
                        (light[2] + base[2]) / 2.0,
                    ];
                    let mid2 = [
                        (dark[0] + base[0]) / 2.0,
                        (dark[1] + base[1]) / 2.0,
                        (dark[2] + base[2]) / 2.0,
                    ];
                    quad(
                        &mut mesh,
                        rr,
                        to32(light),
                        to32(mid),
                        to32(dark),
                        to32(mid2),
                    );
                } else {
                    let col = to32(base);
                    quad(&mut mesh, rr, col, col, col, col);
                }
                if labels_on
                    && c.w >= 120.0
                    && c.h >= 64.0
                    && c.w * c.h >= 12000.0
                    && labels.len() < 40
                {
                    let name = match c.kind {
                        CellKind::File => tree.name(c.node).to_owned(),
                        CellKind::Lump => format!(
                            "{} {}",
                            c.count,
                            if lang == Language::PtBr {
                                "itens pequenos"
                            } else {
                                "small items"
                            }
                        ),
                        CellKind::Free => lang.text("Free space").to_owned(),
                        CellKind::Dir => unreachable!(),
                    };
                    let lum = base[0] * 0.3 + base[1] * 0.59 + base[2] * 0.11;
                    let col = if lum > 0.55 {
                        Color32::from_rgb(15, 18, 26)
                    } else {
                        Color32::from_rgb(245, 247, 252)
                    };
                    if let Some(t) = fit(&name, c.w) {
                        labels.push(Label {
                            clip: r,
                            pos: rr.min + egui::vec2(3.0, 1.0),
                            text: t,
                            color: col,
                            size: 10.5,
                        });
                        if c.h >= 25.0 {
                            if let Some(t) = fit(&fmt::size(c.size), c.w) {
                                labels.push(Label {
                                    clip: r,
                                    pos: rr.min + egui::vec2(3.0, 13.0),
                                    text: t,
                                    color: col.gamma_multiply(0.75),
                                    size: 9.5,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Scene {
        mesh: Arc::new(mesh),
        labels,
    }
}

/// A single hierarchy level, with a fixed text budget and no recursive title strips.
/// Small items remain proportional and discoverable through hover and the side panel.
pub fn build_overview(
    cells: &[Cell],
    tree: &Tree,
    dark: bool,
    labels_on: bool,
    lang: Language,
) -> Scene {
    let mut mesh = Mesh::default();
    let mut labels = Vec::new();
    let total = cells.iter().map(|c| c.size as f64).sum::<f64>().max(1.0);
    let palette = [
        [0.34, 0.27, 0.48],
        [0.21, 0.34, 0.42],
        [0.22, 0.38, 0.34],
        [0.43, 0.32, 0.24],
        [0.30, 0.34, 0.48],
        [0.42, 0.27, 0.35],
    ];
    let mut sorted: Vec<_> = cells.iter().collect();
    sorted.sort_by_key(|c| std::cmp::Reverse(c.size));
    let text = if dark {
        Color32::from_rgb(244, 241, 250)
    } else {
        Color32::from_rgb(30, 28, 40)
    };
    for (rank, c) in sorted.iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(c.x, c.y), egui::vec2(c.w, c.h)).shrink(3.0);
        if !rect.is_positive() {
            continue;
        }
        let hash = if c.node == spacehunter_core::NONE {
            0
        } else {
            tree.name(c.node)
                .bytes()
                .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize))
        };
        let base = if c.kind == CellKind::Lump {
            [0.23, 0.25, 0.29]
        } else {
            palette[hash % palette.len()]
        };
        let base = if dark {
            base
        } else {
            base.map(|v| 0.72 + v * 0.42)
        };
        let col = to32(base);
        quad(&mut mesh, rect, col, col, col, col);
        if !labels_on || rank >= 16 || rect.width() < 100.0 || rect.height() < 64.0 {
            continue;
        }
        let roomy = rect.width() >= 175.0 && rect.height() >= 125.0;
        let inset = if roomy { 18.0 } else { 10.0 };
        let clip = rect.shrink(inset);
        let name = match c.kind {
            CellKind::Lump => lang.text("Other items"),
            CellKind::Free => lang.text("Free space"),
            _ => tree.name(c.node),
        };
        // Conservative estimate plus an explicit clip avoids text crossing tile boundaries.
        let width = clip.width() * if roomy { 0.58 } else { 0.8 };
        if let Some(name) = fit(name, width) {
            labels.push(Label {
                clip,
                pos: clip.min,
                text: name,
                color: text,
                size: if roomy { 18.0 } else { 13.0 },
            });
            labels.push(Label {
                clip,
                pos: clip.min + egui::vec2(0.0, if roomy { 31.0 } else { 23.0 }),
                text: fmt::size(c.size),
                color: text,
                size: if roomy { 25.0 } else { 13.0 },
            });
            if roomy {
                let kind = match c.kind {
                    CellKind::Dir => lang.text("Folder"),
                    CellKind::File => lang.text("File"),
                    CellKind::Lump => lang.text("Other items"),
                    CellKind::Free => lang.text("Free space"),
                };
                labels.push(Label {
                    clip,
                    pos: Pos2::new(clip.min.x, clip.max.y - 15.0),
                    text: format!("{:.1}% · {}", c.size as f64 / total * 100.0, kind),
                    color: text.gamma_multiply(0.72),
                    size: 12.0,
                });
            }
        }
    }
    Scene {
        mesh: Arc::new(mesh),
        labels,
    }
}

pub fn paint(painter: &egui::Painter, scene: &Scene, offset: egui::Vec2) {
    // The mesh was built for a fixed origin; translating a clone is cheaper than rebuilding but
    // we avoid even that when the offset is zero.
    if offset == egui::Vec2::ZERO {
        painter.add(egui::Shape::mesh(scene.mesh.clone()));
    } else {
        let mut m = (*scene.mesh).clone();
        m.translate(offset);
        painter.add(egui::Shape::mesh(Arc::new(m)));
    }
    let font = |s: f32| egui::FontId::proportional(s);
    for l in &scene.labels {
        painter
            .with_clip_rect(l.clip.translate(offset).intersect(painter.clip_rect()))
            .text(
                l.pos + offset,
                egui::Align2::LEFT_TOP,
                &l.text,
                font(l.size),
                l.color,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacehunter_core::{layout, LayoutOptions};

    #[test]
    fn overview_has_bounded_readable_labels_on_dense_demo() {
        let tree = spacehunter_core::sample::generate(42, 60000);
        for (w, h) in [(1100.0, 700.0), (320.0, 400.0)] {
            let mut cells = layout(
                &tree,
                0,
                [0.0, 0.0, w, h],
                None,
                &LayoutOptions {
                    max_depth: 1,
                    header: 0.0,
                    pad: 0.0,
                    min_area: (w * h * 0.008).max(1200.0),
                    ..Default::default()
                },
            );
            cells.retain(|c| c.kind != CellKind::Dir || c.node != 0);
            let scene = build_overview(&cells, &tree, true, true, Language::PtBr);
            assert!(!scene.labels.is_empty());
            assert!(scene.labels.len() <= 48);
            for label in &scene.labels {
                assert!(label.size >= 12.0);
                assert!(label.clip.contains(label.pos));
                assert!(label.pos.y + label.size <= label.clip.max.y + 0.01);
            }
            assert!(build_overview(&cells, &tree, true, false, Language::EnUs)
                .labels
                .is_empty());
        }
    }
}
