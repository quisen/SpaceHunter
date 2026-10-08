use eframe::egui::Color32;
use spacehunter_core::{fmt, Cell, CellKind, NodeId, Tree};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    /// Hue from the file extension (like WinDirStat).
    Type,
    /// Every block gets its own hue; neighbours differ.
    Rainbow,
    /// Hue by folder depth.
    Depth,
    /// Heat map: small = cold, big = hot.
    Size,
    /// Windows-like greys / single accent.
    Mono,
}

impl Scheme {
    pub const ALL: [Scheme; 5] = [
        Scheme::Type,
        Scheme::Rainbow,
        Scheme::Depth,
        Scheme::Size,
        Scheme::Mono,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Scheme::Type => "By file type",
            Scheme::Rainbow => "Rainbow",
            Scheme::Depth => "By depth",
            Scheme::Size => "Size heat-map",
            Scheme::Mono => "Monochrome",
        }
    }
}

pub fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

fn hash(s: &str) -> u32 {
    let mut h = 2166136261u32;
    for b in s.bytes() {
        h = (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(16777619);
    }
    h ^ (h >> 15)
}

pub fn to32(c: [f32; 3]) -> Color32 {
    Color32::from_rgb(
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
    )
}

pub struct ColorCtx {
    pub depth_base: u16,
    pub scheme: Scheme,
    pub dark: bool,
    pub max_size: u64,
}

/// SpaceMonger's original depth palette: (base, light, dark) per `depth & 7`.
const CLASSIC: [[[u8; 3]; 3]; 8] = [
    [[255, 127, 127], [255, 191, 191], [191, 127, 127]],
    [[255, 191, 127], [255, 223, 191], [191, 159, 95]],
    [[255, 255, 0], [255, 255, 191], [191, 191, 63]],
    [[127, 255, 127], [191, 255, 191], [127, 191, 127]],
    [[127, 255, 255], [223, 255, 255], [127, 191, 191]],
    [[191, 191, 255], [223, 223, 255], [159, 159, 255]],
    [[191, 191, 191], [223, 223, 223], [159, 159, 159]],
    [[255, 127, 255], [255, 191, 255], [191, 127, 191]],
];

impl ColorCtx {
    fn classic(&self, depth: u16) -> [[f32; 3]; 3] {
        let e = &CLASSIC[((self.depth_base + depth) & 7) as usize];
        let f = |c: [u8; 3]| {
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
            ]
        };
        let k = if self.dark { 0.82 } else { 1.0 };
        let dim = |c: [f32; 3]| [c[0] * k, c[1] * k, c[2] * k];
        [dim(f(e[0])), dim(f(e[1])), dim(f(e[2]))]
    }

    /// (base, light, dark) of a block for the bevel.
    pub fn shades(&self, tree: &Tree, c: &Cell) -> [[f32; 3]; 3] {
        if self.scheme == Scheme::Rainbow && matches!(c.kind, CellKind::File | CellKind::Dir) {
            return self.classic(c.depth);
        }
        let b = self.leaf(tree, c);
        let sh = |d: f32| [b[0] + d, b[1] + d, b[2] + d];
        [b, sh(0.18), sh(-0.16)]
    }

    /// Base colour (linear 0..1 sRGB values) of a leaf block.
    pub fn leaf(&self, tree: &Tree, c: &Cell) -> [f32; 3] {
        let l = if self.dark { 0.52 } else { 0.62 };
        match c.kind {
            CellKind::Free => {
                return if self.dark {
                    [0.10, 0.22, 0.14]
                } else {
                    [0.78, 0.92, 0.80]
                }
            }
            CellKind::Lump => {
                return if self.dark {
                    [0.30, 0.31, 0.34]
                } else {
                    [0.72, 0.73, 0.76]
                }
            }
            _ => {}
        }
        let name = tree.name(c.node);
        let is_dir = c.kind == CellKind::Dir;
        match self.scheme {
            Scheme::Type => {
                if is_dir {
                    return hsl(215.0, 0.25, l - 0.1);
                }
                let e = fmt::ext(name);
                let hh = hash(e);
                let hue = if e.is_empty() {
                    40.0
                } else {
                    (hh % 360) as f32
                };
                let s = if e.is_empty() { 0.1 } else { 0.62 };
                hsl(hue, s, l + ((hash(name) & 7) as f32 - 3.5) * 0.012)
            }
            Scheme::Rainbow => self.classic(c.depth)[0],
            Scheme::Depth => hsl(
                c.depth as f32 * 47.0 + 200.0,
                0.55,
                l + if is_dir { -0.12 } else { 0.0 },
            ),
            Scheme::Size => {
                let t = if self.max_size > 1 {
                    ((c.size.max(1) as f32).ln() / (self.max_size as f32).ln()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                hsl(240.0 - t * 240.0, 0.75, l - 0.06)
            }
            Scheme::Mono => {
                let v = 0.45 + ((hash(name) & 15) as f32) * 0.006;
                if self.dark {
                    [v * 0.7, v * 0.9, v * 1.15]
                } else {
                    [v + 0.25, v + 0.3, v + 0.38]
                }
            }
        }
    }

    /// Frame colour of a folder (nesting is visible through slightly shifting lightness).
    pub fn frame(&self, depth: u16) -> [f32; 3] {
        let k = (depth % 6) as f32 * 0.018;
        if self.dark {
            hsl(222.0, 0.16, 0.10 + k)
        } else {
            hsl(222.0, 0.12, 0.90 - k)
        }
    }
    pub fn header(&self, depth: u16) -> [f32; 3] {
        if self.scheme == Scheme::Rainbow {
            let c = self.classic(depth)[0];
            return [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6];
        }
        let k = (depth % 6) as f32 * 0.018;
        if self.dark {
            hsl(222.0, 0.22, 0.20 + k)
        } else {
            hsl(222.0, 0.18, 0.80 - k)
        }
    }
}

/// Max single file size in a subtree – used by the heat-map.
pub fn max_file_size(tree: &Tree, root: NodeId) -> u64 {
    let mut m = 0;
    let mut st = vec![root];
    while let Some(n) = st.pop() {
        let node = tree.node(n);
        if node.is_dir {
            st.extend_from_slice(tree.children(n));
        } else {
            m = m.max(node.size);
        }
    }
    m
}
