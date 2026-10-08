//! 2D treemap painting: one cached gouraud-shaded mesh + a capped list of text labels.

use crate::colors::{to32, ColorCtx};
use eframe::egui::{self, epaint::Vertex, Color32, Mesh, Pos2, Rect};
use spacehunter_core::{fmt, Cell, CellKind, Tree};
use std::sync::Arc;

pub struct Label {
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
    m.vertices.push(Vertex { pos: r.left_top(), uv, color: tl });
    m.vertices.push(Vertex { pos: r.right_top(), uv, color: tr });
    m.vertices.push(Vertex { pos: r.right_bottom(), uv, color: br });
    m.vertices.push(Vertex { pos: r.left_bottom(), uv, color: bl });
    m.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
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

pub fn build(cells: &[Cell], tree: &Tree, cc: &ColorCtx, origin: Pos2, bevel: bool, labels_on: bool) -> Scene {
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(cells.len() * 4);
    mesh.indices.reserve(cells.len() * 6);
    let mut labels = Vec::new();
    let text_hi = if cc.dark { Color32::from_rgb(235, 238, 245) } else { Color32::from_rgb(20, 24, 32) };
    for c in cells {
        let r = Rect::from_min_size(Pos2::new(origin.x + c.x, origin.y + c.y), egui::vec2(c.w, c.h));
        match c.kind {
            CellKind::Dir => {
                let f = cc.frame(c.depth);
                quad(&mut mesh, r, to32(f), to32(f), to32(f), to32(f));
                if c.header > 0.0 {
                    let h = cc.header(c.depth);
                    let hr = Rect::from_min_size(r.min + egui::vec2(1.0, 1.0), egui::vec2((c.w - 2.0).max(0.0), c.header - 1.0));
                    quad(&mut mesh, hr, shade(h, 0.04), shade(h, 0.04), shade(h, -0.02), shade(h, -0.02));
                    if labels_on {
                        let t = format!("{}  ·  {}", tree.name(c.node), fmt::size(c.size));
                        if let Some(t) = fit(&t, c.w - 4.0) {
                            labels.push(Label { pos: hr.min + egui::vec2(3.0, 0.5), text: t, color: text_hi, size: 11.0 });
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
                    let mid = [(light[0] + base[0]) / 2.0, (light[1] + base[1]) / 2.0, (light[2] + base[2]) / 2.0];
                    let mid2 = [(dark[0] + base[0]) / 2.0, (dark[1] + base[1]) / 2.0, (dark[2] + base[2]) / 2.0];
                    quad(&mut mesh, rr, to32(light), to32(mid), to32(dark), to32(mid2));
                } else {
                    let col = to32(base);
                    quad(&mut mesh, rr, col, col, col, col);
                }
                if labels_on && c.w >= 26.0 && c.h >= 12.0 && labels.len() < 2500 {
                    let name = match c.kind {
                        CellKind::File => tree.name(c.node).to_owned(),
                        CellKind::Lump => format!("{} small items", c.count),
                        CellKind::Free => "Free space".to_owned(),
                        CellKind::Dir => unreachable!(),
                    };
                    let lum = base[0] * 0.3 + base[1] * 0.59 + base[2] * 0.11;
                    let col = if lum > 0.55 { Color32::from_rgb(15, 18, 26) } else { Color32::from_rgb(245, 247, 252) };
                    if let Some(t) = fit(&name, c.w) {
                        labels.push(Label { pos: rr.min + egui::vec2(3.0, 1.0), text: t, color: col, size: 10.5 });
                        if c.h >= 25.0 {
                            if let Some(t) = fit(&fmt::size(c.size), c.w) {
                                labels.push(Label { pos: rr.min + egui::vec2(3.0, 13.0), text: t, color: col.gamma_multiply(0.75), size: 9.5 });
                            }
                        }
                    }
                }
            }
        }
    }
    Scene { mesh: Arc::new(mesh), labels }
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
        painter.text(l.pos + offset, egui::Align2::LEFT_TOP, &l.text, font(l.size), l.color);
    }
}
