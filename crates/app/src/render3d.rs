//! 3D "city" view: every file is an extruded box, folders are stacked terraces.
//! Instanced rendering with a real depth buffer (one draw call for the whole map).

use crate::colors::ColorCtx;
use eframe::egui;
use egui_glow::glow::{self, HasContext};
use spacehunter_core::{Cell, CellKind, Tree};
use std::sync::{Arc, Mutex};

const FLOATS: usize = 9; // x z w d | base height | r g b

#[derive(Clone, Copy)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: [f32; 3],
}

impl Camera {
    pub fn for_map(w: f32, h: f32) -> Self {
        Camera { yaw: 0.6, pitch: 0.75, dist: w.max(h) * 1.25, target: [w / 2.0, 0.0, h / 2.0] }
    }
    pub fn eye(&self) -> [f32; 3] {
        let (cp, sp, cy, sy) = (self.pitch.cos(), self.pitch.sin(), self.yaw.cos(), self.yaw.sin());
        [self.target[0] + self.dist * cp * sy, self.target[1] + self.dist * sp, self.target[2] + self.dist * cp * cy]
    }
    pub fn view_proj(&self, aspect: f32) -> [f32; 16] {
        let eye = self.eye();
        let v = look_at(eye, self.target, [0.0, 1.0, 0.0]);
        let p = perspective(0.8, aspect, self.dist * 0.01, self.dist * 8.0);
        mul(&p, &v)
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Column-major 4x4.
fn look_at(eye: [f32; 3], at: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = norm(sub(at, eye));
    let s = norm(cross(f, up));
    let u = cross(s, f);
    [
        s[0], u[0], -f[0], 0.0, s[1], u[1], -f[1], 0.0, s[2], u[2], -f[2], 0.0, -dot(s, eye), -dot(u, eye), dot(f, eye), 1.0,
    ]
}
fn perspective(fovy: f32, aspect: f32, n: f32, f: f32) -> [f32; 16] {
    let t = 1.0 / (fovy / 2.0).tan();
    [t / aspect, 0.0, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, 0.0, (f + n) / (n - f), -1.0, 0.0, 0.0, 2.0 * f * n / (n - f), 0.0]
}
fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for rr in 0..4 {
            r[c * 4 + rr] = (0..4).map(|k| a[k * 4 + rr] * b[c * 4 + k]).sum();
        }
    }
    r
}

/// CPU-side instance list (also used for picking).
pub struct Boxes {
    pub data: Vec<f32>,
    /// instance index -> index into the cell list
    pub cell_of: Vec<u32>,
    pub extent: [f32; 2],
}

pub struct HeightStyle {
    pub by_size: bool,
    pub scale: f32,
}

pub fn build_boxes(cells: &[Cell], tree: &Tree, cc: &ColorCtx, extent: [f32; 2], hs: &HeightStyle) -> Boxes {
    let slab = 3.0f32;
    let hmax = extent[0].min(extent[1]) * 0.25 * hs.scale;
    let max = cells.iter().filter(|c| c.kind == CellKind::File).map(|c| c.size).max().unwrap_or(1).max(1) as f32;
    let mut data = Vec::with_capacity(cells.len() * FLOATS);
    let mut cell_of = Vec::with_capacity(cells.len());
    for (i, c) in cells.iter().enumerate() {
        let base = c.depth as f32 * slab;
        let (h, col) = match c.kind {
            CellKind::Dir => (slab, cc.frame(c.depth)),
            CellKind::Free => (slab * 0.5, cc.leaf(tree, c)),
            CellKind::Lump => (slab * 1.5, cc.leaf(tree, c)),
            _ => {
                let rel = (c.size as f32 / max).sqrt();
                let h = if hs.by_size { slab + hmax * rel } else { slab * 4.0 };
                (h, cc.leaf(tree, c))
            }
        };
        let g = if c.kind == CellKind::Dir { 0.0 } else { 0.4 };
        if c.w <= g * 2.0 || c.h <= g * 2.0 {
            continue;
        }
        data.extend_from_slice(&[c.x + g, c.y + g, c.w - g * 2.0, c.h - g * 2.0, base, h, col[0], col[1], col[2]]);
        cell_of.push(i as u32);
    }
    Boxes { data, cell_of, extent }
}

/// Slab-method ray/box test; returns instance index of nearest hit.
pub fn pick(b: &Boxes, origin: [f32; 3], dir: [f32; 3]) -> Option<usize> {
    let mut best = (f32::INFINITY, None);
    for (i, ch) in b.data.chunks_exact(FLOATS).enumerate() {
        let lo = [ch[0], ch[4], ch[1]];
        let hi = [ch[0] + ch[2], ch[4] + ch[5], ch[1] + ch[3]];
        let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
        for a in 0..3 {
            if dir[a].abs() < 1e-9 {
                if origin[a] < lo[a] || origin[a] > hi[a] {
                    t1 = -1.0;
                    break;
                }
            } else {
                let (mut a0, mut a1) = ((lo[a] - origin[a]) / dir[a], (hi[a] - origin[a]) / dir[a]);
                if a0 > a1 {
                    std::mem::swap(&mut a0, &mut a1);
                }
                t0 = t0.max(a0);
                t1 = t1.min(a1);
                if t0 > t1 {
                    break;
                }
            }
        }
        if t0 <= t1 && t0 < best.0 {
            best = (t0, Some(i));
        }
    }
    best.1
}

/// Ray through a point of the 3D viewport (pixel offsets from the viewport's top-left, in points).
pub fn ray(cam: &Camera, size: egui::Vec2, p: egui::Vec2) -> ([f32; 3], [f32; 3]) {
    let eye = cam.eye();
    let f = norm(sub(cam.target, eye));
    let s = norm(cross(f, [0.0, 1.0, 0.0]));
    let u = cross(s, f);
    let t = (0.8f32 / 2.0).tan();
    let nx = (p.x / size.x * 2.0 - 1.0) * t * (size.x / size.y);
    let ny = (1.0 - p.y / size.y * 2.0) * t;
    let d = norm([f[0] + s[0] * nx + u[0] * ny, f[1] + s[1] * nx + u[1] * ny, f[2] + s[2] * nx + u[2] * ny]);
    (eye, d)
}

const VS: &str = r#"
layout(location=0) in vec4 i_rect;   // x z w d
layout(location=1) in vec2 i_bh;     // base, height
layout(location=2) in vec3 i_col;
uniform mat4 u_mvp;
uniform int u_hover;
uniform int u_sel;
out vec3 v_col;
out vec2 v_edge;   // distance to the face edges, in map pixels
out vec2 v_size;
out float v_top;
out float v_fog;
void main() {
    int id = gl_VertexID / 6;
    int vi = gl_VertexID - id * 6;
    int axis = id / 2;
    float sgn = (id - axis * 2 == 1) ? 1.0 : -1.0;
    int qi = (vi == 0) ? 0 : (vi == 1 || vi == 4) ? 1 : (vi == 2 || vi == 3) ? 2 : 3;
    float a = float(qi & 1);
    float b = float(qi >> 1);
    vec3 p; vec3 n = vec3(0.0);
    if (axis == 0)      { p = vec3(sgn > 0.0 ? 1.0 : 0.0, a, b); n.x = sgn; }
    else if (axis == 1) { p = vec3(b, sgn > 0.0 ? 1.0 : 0.0, a); n.y = sgn; }
    else                { p = vec3(a, b, sgn > 0.0 ? 1.0 : 0.0); n.z = sgn; }
    vec3 world = vec3(i_rect.x + p.x * i_rect.z, i_bh.x + p.y * i_bh.y, i_rect.y + p.z * i_rect.w);
    gl_Position = u_mvp * vec4(world, 1.0);
    vec3 L = normalize(vec3(-0.45, 0.85, -0.35));
    float diff = max(dot(n, L), 0.0);
    float light = 0.42 + 0.62 * diff + (n.x != 0.0 ? 0.0 : 0.0);
    vec3 c = i_col * light;
    if (gl_InstanceID == u_hover) c = mix(c, vec3(1.0), 0.45);
    if (gl_InstanceID == u_sel)   c = mix(c, vec3(1.0, 0.85, 0.2), 0.55);
    v_col = c;
    v_top = n.y > 0.5 ? 1.0 : 0.0;
    v_edge = vec2(p.x * i_rect.z, p.z * i_rect.w);
    v_size = i_rect.zw;
    v_fog = clamp(gl_Position.w * 0.0, 0.0, 1.0);
}
"#;

const FS: &str = r#"
in vec3 v_col;
in vec2 v_edge;
in vec2 v_size;
in float v_top;
in float v_fog;
out vec4 o_col;
void main() {
    vec3 c = v_col;
    if (v_top > 0.5) {
        vec2 d = min(v_edge, v_size - v_edge);
        float e = min(d.x, d.y);
        c *= mix(0.72, 1.0, smoothstep(0.0, 1.2, e));
    }
    o_col = vec4(c, 1.0);
}
"#;

struct Gpu {
    prog: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    count: i32,
    uploaded_rev: u64,
}

/// Shared between UI thread (which sets camera/data) and the GL callback.
pub struct Shared {
    gpu: Option<Gpu>,
    pub data: Vec<f32>,
    pub rev: u64,
    pub hover: i32,
    pub sel: i32,
    pub mvp: [f32; 16],
    pub clear: [f32; 3],
}

impl Default for Shared {
    fn default() -> Self {
        Shared { gpu: None, data: Vec::new(), rev: 0, hover: -1, sel: -1, mvp: [0.0; 16], clear: [0.0; 3] }
    }
}

pub type SharedRef = Arc<Mutex<Shared>>;

fn compile(gl: &glow::Context) -> Result<glow::Program, String> {
    let es = gl.version().is_embedded;
    let header = if es { "#version 300 es\nprecision highp float;\nprecision highp int;\n" } else { "#version 330 core\n" };
    unsafe {
        let prog = gl.create_program()?;
        let mut shaders = Vec::new();
        for (ty, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
            let sh = gl.create_shader(ty)?;
            gl.shader_source(sh, &format!("{header}{src}"));
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(gl.get_shader_info_log(sh));
            }
            gl.attach_shader(prog, sh);
            shaders.push(sh);
        }
        gl.link_program(prog);
        if !gl.get_program_link_status(prog) {
            return Err(gl.get_program_info_log(prog));
        }
        for s in shaders {
            gl.detach_shader(prog, s);
            gl.delete_shader(s);
        }
        Ok(prog)
    }
}

fn init(gl: &glow::Context) -> Option<Gpu> {
    unsafe {
        let prog = match compile(gl) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("3D shader error: {e}");
                return None;
            }
        };
        let vao = gl.create_vertex_array().ok()?;
        let vbo = gl.create_buffer().ok()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let stride = (FLOATS * 4) as i32;
        for (loc, n, off) in [(0u32, 4, 0), (1, 2, 16), (2, 3, 24)] {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_f32(loc, n, glow::FLOAT, false, stride, off);
            gl.vertex_attrib_divisor(loc, 1);
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        Some(Gpu { prog, vao, vbo, count: 0, uploaded_rev: u64::MAX })
    }
}

/// Add the draw callback for the 3D map to the painter.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, shared: SharedRef) {
    let cb = egui_glow::CallbackFn::new(move |_info, p| {
        let gl = p.gl();
        let mut s = shared.lock().unwrap();
        if s.gpu.is_none() {
            s.gpu = init(gl);
        }
        let s = &mut *s;
        let Some(g) = s.gpu.as_mut() else { return };
        unsafe {
            if g.uploaded_rev != s.rev {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(g.vbo));
                let bytes: &[u8] = std::slice::from_raw_parts(s.data.as_ptr() as *const u8, s.data.len() * 4);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
                g.count = (s.data.len() / FLOATS) as i32;
                g.uploaded_rev = s.rev;
            }
            gl.clear_color(s.clear[0], s.clear[1], s.clear[2], 1.0);
            gl.clear_depth_f32(1.0);
            gl.depth_mask(true);
            gl.clear(glow::DEPTH_BUFFER_BIT | glow::COLOR_BUFFER_BIT);
            if g.count == 0 {
                return;
            }
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            gl.use_program(Some(g.prog));
            gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(g.prog, "u_mvp").as_ref(), false, &s.mvp);
            gl.uniform_1_i32(gl.get_uniform_location(g.prog, "u_hover").as_ref(), s.hover);
            gl.uniform_1_i32(gl.get_uniform_location(g.prog, "u_sel").as_ref(), s.sel);
            gl.bind_vertex_array(Some(g.vao));
            gl.draw_arrays_instanced(glow::TRIANGLES, 0, 36, g.count);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
        }
    });
    painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
}
