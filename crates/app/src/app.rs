use crate::colors::{max_file_size, ColorCtx, Scheme};
use crate::platform::{self, Job, Loaded};
use crate::{render2d, render3d};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use spacehunter_core::{fmt, hit_test, layout, layout_classic, Cell, CellKind, ClassicOptions, LayoutOptions, NodeId, Tree, DENSITY, NONE};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use web_time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    D2,
    D3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    /// SpaceMonger's balanced binary partition.
    Classic,
    /// Squarified treemap (better aspect ratios).
    Squarified,
}

pub struct Settings {
    pub algo: Algo,
    pub density: usize,
    pub bias: f32,
    pub scheme: Scheme,
    pub show_free: bool,
    pub animated: bool,
    pub tooltips: bool,
    pub labels: bool,
    pub bevel: bool,
    pub headers: bool,
    pub size_on_disk: bool,
    pub delete_enabled: bool,
    pub height_by_size: bool,
    pub height_scale: f32,
    pub dark: bool,
}

pub const DENSITY_LABELS: [&str; 6] = ["Too many files", "Very many files", "Lots of files", "Normal", "Very few files", "Too few files"];
const DENSITY_AREA: [f32; 6] = [3.0, 10.0, 22.0, 40.0, 110.0, 320.0];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            algo: Algo::Classic,
            density: 3,
            bias: 0.0,
            scheme: Scheme::Type,
            show_free: false,
            animated: true,
            tooltips: true,
            labels: true,
            bevel: true,
            headers: true,
            size_on_disk: true,
            delete_enabled: false,
            height_by_size: true,
            height_scale: 1.0,
            dark: true,
        }
    }
}

#[derive(Default)]
pub struct Args {
    pub path: Option<std::path::PathBuf>,
    pub demo: bool,
    pub mode3d: bool,
    pub screenshot: Option<std::path::PathBuf>,
    pub zoom_first: bool,
}

#[derive(PartialEq, Clone, Copy)]
struct LayoutKey {
    root: NodeId,
    w: u32,
    h: u32,
    rev: u64,
    tree: u64,
}

struct Anim {
    root: NodeId,
    from: [f32; 4],
    to: [f32; 4],
    start: Instant,
    /// Show `then_root` once finished (zoom out keeps the old root shrinking, then switches).
    zoom_out: bool,
}

pub struct SpaceHunter {
    args: Args,
    tree: Option<Arc<Tree>>,
    tree_id: u64,
    disk: Option<(u64, u64)>,
    root_path: Option<std::path::PathBuf>,
    view_root: NodeId,
    selected: Option<NodeId>,
    job: Option<Job>,
    job_started: Instant,
    mode: Mode,
    s: Settings,
    rev: u64,
    cells: Vec<Cell>,
    cells_key: Option<LayoutKey>,
    layout_ms: f32,
    scene: Option<render2d::Scene>,
    scene_key: u64,
    boxes: Option<render3d::Boxes>,
    boxes_key: u64,
    shared3d: render3d::SharedRef,
    cam: render3d::Camera,
    cam_for: Option<(u32, u32)>,
    hover: Option<usize>,
    anim: Option<Anim>,
    scroll_acc: f32,
    show_setup: bool,
    show_about: bool,
    show_side: bool,
    confirm_delete: Option<NodeId>,
    ctx_target: Option<NodeId>,
    toast: Option<(String, Instant)>,
    top_files: Vec<NodeId>,
    max_cache: std::cell::Cell<(u64, NodeId, u64)>,
    dirs_cache: std::cell::Cell<(u64, NodeId, u64)>,
    top_key: (u64, NodeId),
    frames_since_load: u32,
    shot_requested: bool,
    #[cfg(target_arch = "wasm32")]
    drop_slot: std::rc::Rc<std::cell::RefCell<Option<Job>>>,
}

impl SpaceHunter {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        apply_theme(&cc.egui_ctx);
        let mut app = SpaceHunter {
            mode: if args.mode3d { Mode::D3 } else { Mode::D2 },
            tree: None,
            tree_id: 0,
            disk: None,
            root_path: None,
            view_root: 0,
            selected: None,
            job: None,
            job_started: Instant::now(),
            s: Settings::default(),
            rev: 1,
            cells: Vec::new(),
            cells_key: None,
            layout_ms: 0.0,
            scene: None,
            scene_key: 0,
            boxes: None,
            boxes_key: 0,
            shared3d: Default::default(),
            cam: render3d::Camera::for_map(1000.0, 700.0),
            cam_for: None,
            hover: None,
            anim: None,
            scroll_acc: 0.0,
            show_setup: false,
            show_about: false,
            show_side: true,
            confirm_delete: None,
            ctx_target: None,
            toast: None,
            top_files: Vec::new(),
            max_cache: std::cell::Cell::new((u64::MAX, NONE, 0)),
            dirs_cache: std::cell::Cell::new((u64::MAX, NONE, 0)),
            top_key: (u64::MAX, NONE),
            frames_since_load: 0,
            shot_requested: false,
            #[cfg(target_arch = "wasm32")]
            drop_slot: Default::default(),
            args,
        };
        #[cfg(target_arch = "wasm32")]
        crate::web::install_drop(&cc.egui_ctx, app.drop_slot.clone());
        if app.args.demo {
            app.job = Some(platform::demo(&cc.egui_ctx));
        } else if let Some(p) = app.args.path.clone() {
            app.start_path(&cc.egui_ctx, p);
        }
        app
    }

    // ───────────────────────────── loading ─────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    fn start_path(&mut self, ctx: &egui::Context, p: std::path::PathBuf) {
        self.cancel_job();
        self.job_started = Instant::now();
        self.job = Some(platform::start_scan(ctx, p, platform::default_opts(self.s.size_on_disk)));
    }
    #[cfg(target_arch = "wasm32")]
    fn start_path(&mut self, ctx: &egui::Context, _p: std::path::PathBuf) {
        self.job = Some(platform::demo(ctx));
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = platform::pick_folder() {
            self.start_path(ctx, p);
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.cancel_job();
            self.job_started = Instant::now();
            self.job = platform::start_open(ctx);
        }
    }

    fn cancel_job(&mut self) {
        if let Some(j) = self.job.take() {
            j.progress.cancel.store(true, Relaxed);
        }
    }

    fn rescan(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.root_path.clone() {
            self.start_path(ctx, p);
        } else {
            self.open_dialog(ctx);
        }
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        {
            let dropped = self.drop_slot.borrow_mut().take();
            if let Some(j) = dropped {
                self.cancel_job();
                self.job_started = Instant::now();
                self.job = Some(j);
            }
        }
        let Some(job) = &self.job else { return };
        // Read `done` BEFORE the inbox: a worker fills the inbox and only then sets `done`.
        let finished = job.progress.done.load(Relaxed);
        let loaded = job.inbox.lock().unwrap().take();
        if let Some(Loaded { tree, path, disk }) = loaded {
            let had = self.tree.is_some();
            self.tree_id += 1;
            let (n, sz) = (tree.len(), tree.total_size());
            self.toast = Some((if sz == 0 { format!("Scan finished but found no readable files ({n} items) – check permissions") } else { format!("Loaded {} items, {}", group(n as u64), fmt::size(sz)) }, Instant::now()));
            self.tree = Some(Arc::new(tree));
            self.disk = disk;
            if path.is_some() || !had || self.root_path.is_some() {
                self.root_path = path;
            }
            self.view_root = 0;
            self.selected = None;
            self.hover = None;
            self.cells_key = None;
            self.boxes = None;
            self.top_key = (u64::MAX, NONE);
            self.cam_for = None;
            self.frames_since_load = 0;
            self.job = None;
            ctx.request_repaint();
        } else if finished {
            if let Some(e) = job.progress.error.lock().unwrap().clone() {
                if e != "cancelled" {
                    self.toast = Some((format!("Could not load: {e}"), Instant::now()));
                }
            }
            self.job = None;
        }
    }

    // ───────────────────────────── navigation ─────────────────────────────

    fn tree(&self) -> Option<Arc<Tree>> {
        self.tree.clone()
    }

    fn zoom_to(&mut self, node: NodeId, canvas: Rect) {
        let Some(t) = self.tree() else { return };
        if node == self.view_root || !t.node(node).is_dir {
            return;
        }
        if self.s.animated && self.mode == Mode::D2 {
            // grow from the folder's current rectangle
            if let Some(c) = self.cells.iter().find(|c| c.node == node && c.kind == CellKind::Dir) {
                self.anim = Some(Anim { root: node, from: [c.x, c.y, c.w, c.h], to: [0.0, 0.0, canvas.width(), canvas.height()], start: Instant::now(), zoom_out: false });
            }
        }
        self.view_root = node;
        self.selected = None;
        self.cam_for = None;
    }

    fn zoom_out(&mut self, canvas: Rect) {
        let Some(t) = self.tree() else { return };
        let p = t.node(self.view_root).parent;
        if p == NONE {
            return;
        }
        let old = self.view_root;
        self.view_root = p;
        self.selected = Some(old);
        self.cam_for = None;
        if self.s.animated && self.mode == Mode::D2 {
            // relayout the parent now to find where `old` sits, then shrink into it
            let cells = self.compute_layout(&t, p, canvas);
            if let Some(c) = cells.iter().find(|c| c.node == old && c.kind == CellKind::Dir) {
                self.anim = Some(Anim { root: old, from: [0.0, 0.0, canvas.width(), canvas.height()], to: [c.x, c.y, c.w, c.h], start: Instant::now(), zoom_out: true });
            }
        }
    }

    fn layout_opts(&self) -> LayoutOptions {
        LayoutOptions {
            min_area: DENSITY_AREA[self.s.density.min(5)],
            bias: self.s.bias,
            header: if self.s.headers { 14.0 } else { 0.0 },
            ..Default::default()
        }
    }

    fn compute_layout(&self, t: &Tree, root: NodeId, canvas: Rect) -> Vec<Cell> {
        let free = if self.s.show_free && root == 0 { self.disk.map(|d| d.1) } else { None };
        self.layout_in(t, root, [0.0, 0.0, canvas.width(), canvas.height()], free)
    }

    fn layout_in(&self, t: &Tree, root: NodeId, rect: [f32; 4], free: Option<u64>) -> Vec<Cell> {
        match self.s.algo {
            Algo::Squarified => layout(t, root, rect, free, &self.layout_opts()),
            Algo::Classic => {
                let (min_w, min_h) = DENSITY[self.s.density.min(5)];
                // bias slider: -1..1 -> original -20..20
                layout_classic(t, root, rect, free, &ClassicOptions { min_w, min_h, bias: (self.s.bias * 20.0).round() as i32 })
            }
        }
    }

    fn color_ctx(&self, t: &Tree) -> ColorCtx {
        // O(tree) scan: cache per (tree, view root) instead of running it every frame.
        let (tid, root, v) = self.max_cache.get();
        let max_size = if tid == self.tree_id && root == self.view_root {
            v
        } else {
            let v = max_file_size(t, self.view_root);
            self.max_cache.set((self.tree_id, self.view_root, v));
            v
        };
        ColorCtx { depth_base: t.depth(self.view_root) as u16, scheme: self.s.scheme, dark: self.s.dark, max_size }
    }

    fn ensure_layout(&mut self, t: &Tree, canvas: Rect) {
        let key = LayoutKey { root: self.view_root, w: canvas.width() as u32, h: canvas.height() as u32, rev: self.rev, tree: self.tree_id };
        if self.cells_key != Some(key) {
            let t0 = Instant::now();
            self.cells = self.compute_layout(t, self.view_root, canvas);
            self.layout_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.cells_key = Some(key);
            self.scene = None;
            self.boxes = None;
            self.hover = None;
        }
    }

    fn node_of(&self, cell: usize) -> Option<NodeId> {
        let c = self.cells.get(cell)?;
        (c.node != NONE).then_some(c.node)
    }

    fn dir_of(&self, t: &Tree, cell: usize) -> Option<NodeId> {
        let c = self.cells.get(cell)?;
        if c.node == NONE {
            return None;
        }
        if t.node(c.node).is_dir {
            Some(c.node)
        } else {
            Some(t.node(c.node).parent)
        }
    }

    fn full_path(&self, t: &Tree, id: NodeId) -> std::path::PathBuf {
        // the root node stores the absolute scan path as its name
        let sep = std::path::MAIN_SEPARATOR;
        std::path::PathBuf::from(t.path(id, sep))
    }

    fn top_files(&mut self, t: &Tree) {
        let key = (self.tree_id, self.view_root);
        if self.top_key == key {
            return;
        }
        self.top_key = key;
        const N: usize = 30;
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, NodeId)>> = Default::default();
        let mut st = vec![self.view_root];
        while let Some(n) = st.pop() {
            for &c in t.children(n) {
                let node = t.node(c);
                if node.is_dir {
                    st.push(c);
                } else if heap.len() < N || node.size > heap.peek().map_or(0, |r| r.0 .0) {
                    heap.push(std::cmp::Reverse((node.size, c)));
                    if heap.len() > N {
                        heap.pop();
                    }
                }
            }
        }
        let mut v: Vec<_> = heap.into_iter().map(|r| r.0).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        self.top_files = v.into_iter().map(|x| x.1).collect();
    }

    // ───────────────────────────── UI pieces ─────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let ctx = ui.ctx().clone();
        let has = self.tree.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("SpaceHunter").strong().size(15.0).color(ACCENT));
            ui.add_space(6.0);
            #[cfg(not(target_arch = "wasm32"))]
            {
                if ui.add(primary_button("Open folder")).on_hover_text("Choose a folder or drive (Ctrl+O)").clicked() {
                    self.open_dialog(&ctx);
                }
                ui.menu_button("Drives", |ui| {
                    for (label, p) in platform::roots() {
                        if ui.button(label).clicked() {
                            self.start_path(&ctx, p);
                            ui.close();
                        }
                    }
                });
            }
            #[cfg(target_arch = "wasm32")]
            {
                if ui.add(primary_button("Open folder")).on_hover_text("Choose a folder to analyse (Ctrl+O). Nothing is uploaded – everything stays in your browser.").clicked() {
                    self.open_dialog(&ctx);
                }
                if ui.button("Demo").clicked() {
                    self.job = Some(platform::demo(&ctx));
                }
            }
            if ui.add_enabled(has, tool_button("Rescan")).on_hover_text("Scan again (F5)").clicked() {
                self.rescan(&ctx);
            }
            ui.separator();
            if ui.add_enabled(has, tool_button("Full view")).on_hover_text("Zoom full (Home)").clicked() {
                self.zoom_full(canvas);
            }
            if ui.add_enabled(has && self.view_root != 0, tool_button("Up")).on_hover_text("Zoom out (Backspace)").clicked() {
                self.zoom_out(canvas);
            }
            let sel_dir = self.selected.filter(|&n| self.tree.as_ref().is_some_and(|t| t.node(n).is_dir && n != self.view_root));
            if ui.add_enabled(sel_dir.is_some(), tool_button("Enter")).on_hover_text("Zoom into the selected folder (Enter / double-click)").clicked() {
                self.zoom_to(sel_dir.unwrap(), canvas);
            }
            ui.separator();
            ui.add_enabled_ui(self.disk.is_some(), |ui| {
                if pill(ui, "Free space", self.s.show_free).on_hover_text("Show the free space of the drive as one more block").clicked() {
                    self.s.show_free = !self.s.show_free;
                    self.rev += 1;
                }
            });
            ui.separator();
            if pill(ui, "2D", self.mode == Mode::D2).clicked() {
                self.mode = Mode::D2;
            }
            if pill(ui, "3D", self.mode == Mode::D3).clicked() {
                self.mode = Mode::D3;
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                let t = self.selected.filter(|&n| n != 0 && self.root_path.is_some());
                if ui.add_enabled(t.is_some(), tool_button("Open in file manager")).on_hover_text("Open with the default application / show in file manager").clicked() {
                    self.run_selected();
                }
                let can_del = t.is_some() && self.s.delete_enabled;
                let r = ui.add_enabled(can_del, tool_button("Delete"));
                let r = if self.s.delete_enabled { r } else { r.on_disabled_hover_text("Disabled – enable it in Setup") };
                if r.clicked() {
                    self.confirm_delete = t;
                }
            }
            ui.separator();
            if ui.add(tool_button("Setup")).clicked() {
                self.show_setup = !self.show_setup;
            }
            if ui.add(tool_button("About")).clicked() {
                self.show_about = !self.show_about;
            }
            if pill(ui, "Panel", self.show_side).clicked() {
                self.show_side = !self.show_side;
            }
        });
    }

    fn zoom_full(&mut self, canvas: Rect) {
        while self.view_root != 0 {
            let p = self.tree.as_ref().map_or(0, |t| t.node(self.view_root).parent);
            if p == NONE {
                break;
            }
            if p == 0 {
                self.zoom_out(canvas);
                return;
            }
            self.view_root = p;
        }
        self.cam_for = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_selected(&mut self) {
        if let (Some(t), Some(n)) = (self.tree(), self.selected) {
            let p = self.full_path(&t, n);
            if t.node(n).is_dir {
                platform::reveal(&p, true);
            } else {
                platform::run(&p);
            }
        }
    }

    fn breadcrumb(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let Some(t) = self.tree() else { return };
        let mut chain = Vec::new();
        let mut n = self.view_root;
        while n != NONE {
            chain.push(n);
            n = t.node(n).parent;
        }
        chain.reverse();
        let mut target = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, &n) in chain.iter().enumerate() {
                if i > 0 {
                    ui.label(egui::RichText::new("›").weak());
                }
                let name = if i == 0 { t.name(n).to_owned() } else { t.name(n).to_owned() };
                let last = i + 1 == chain.len();
                if ui.add(egui::Button::new(egui::RichText::new(name).strong().underline()).frame(false).selected(last)).clicked() && !last {
                    target = Some(n);
                }
            }
            ui.label(egui::RichText::new(format!("   {}", fmt::size(t.node(self.view_root).size))).weak());
        });
        if let Some(n) = target {
            self.zoom_to_any(n, canvas);
        }
    }

    fn zoom_to_any(&mut self, n: NodeId, canvas: Rect) {
        // zoom out through ancestors (no animation across several levels)
        let was = self.s.animated;
        self.s.animated = false;
        self.view_root = n;
        self.selected = None;
        self.cam_for = None;
        self.s.animated = was;
        let _ = canvas;
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let Some(t) = self.tree() else {
            ui.label("Nothing loaded.");
            return;
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(n) = self.selected.or(Some(self.view_root)) {
                let node = t.node(n);
                ui.heading(if n == 0 { t.name(0).to_owned() } else { t.name(n).to_owned() });
                ui.label(egui::RichText::new(t.path(n, std::path::MAIN_SEPARATOR)).small().weak());
                ui.add_space(4.0);
                egui::Grid::new("details").num_columns(2).show(ui, |ui| {
                    ui.label("Size");
                    ui.label(format!("{} ({} bytes)", fmt::size(node.size), group(node.size)));
                    ui.end_row();
                    let rootsz = t.node(self.view_root).size.max(1);
                    ui.label("Of view");
                    ui.label(format!("{:.2} %", node.size as f64 / rootsz as f64 * 100.0));
                    ui.end_row();
                    if node.is_dir {
                        ui.label("Files");
                        ui.label(group(node.files as u64));
                        ui.end_row();
                        ui.label("Folders");
                        let (tid, cn, v) = self.dirs_cache.get();
                        let dirs = if tid == self.tree_id && cn == n { v } else { let v = t.dir_count(n); self.dirs_cache.set((self.tree_id, n, v)); v };
                        ui.label(group(dirs));
                        ui.end_row();
                    }
                    let d = fmt::date(node.mtime);
                    if !d.is_empty() {
                        ui.label("Modified");
                        ui.label(d + " UTC");
                        ui.end_row();
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("CONTENTS OF THIS VIEW").size(10.5).color(MUTED));
            let vr = t.node(self.view_root).size.max(1);
            let mut go = None;
            for &c in t.children(self.view_root).iter().take(40) {
                let node = t.node(c);
                let frac = node.size as f32 / vr as f32;
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
                let p = ui.painter_at(r);
                let bar = Rect::from_min_size(r.min, egui::vec2(r.width() * frac.max(0.004), r.height()));
                p.rect_filled(bar, 5.0, if node.is_dir { Color32::from_rgba_unmultiplied(98, 142, 255, 60) } else { Color32::from_rgba_unmultiplied(240, 170, 90, 55) });
                if resp.hovered() || self.selected == Some(c) {
                    p.rect_stroke(r, 5.0, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
                }
                p.text(r.left_center() + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, if node.is_dir { format!("{}/", t.name(c)) } else { t.name(c).to_owned() }, FontId::proportional(12.0), ui.visuals().text_color());
                p.text(r.right_center() - egui::vec2(8.0, 0.0), Align2::RIGHT_CENTER, fmt::size(node.size), FontId::proportional(11.0), ui.visuals().weak_text_color());
                if resp.clicked() {
                    self.selected = Some(c);
                }
                if resp.double_clicked() && node.is_dir {
                    go = Some(c);
                }
            }
            if let Some(g) = go {
                self.zoom_to(g, canvas);
            }
            ui.separator();
            ui.label(egui::RichText::new("LARGEST FILES IN VIEW").size(10.5).color(MUTED));
            self.top_files(&t);
            for &f in &self.top_files.clone() {
                let node = t.node(f);
                let r = ui.selectable_label(self.selected == Some(f), format!("{}  —  {}", fmt::size(node.size), t.name(f)));
                let r = r.on_hover_text(t.path(f, std::path::MAIN_SEPARATOR));
                if r.clicked() {
                    self.selected = Some(f);
                }
                if r.double_clicked() {
                    let p = node.parent;
                    self.zoom_to(p, canvas);
                    self.selected = Some(f);
                }
            }
        });
    }

    fn setup_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_setup;
        let mut changed = false;
        egui::Window::new("SpaceHunter Setup").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.strong("File layout");
            ui.horizontal(|ui| {
                ui.label("Algorithm:");
                changed |= ui.selectable_value(&mut self.s.algo, Algo::Classic, "Classic (SpaceMonger)").changed();
                changed |= ui.selectable_value(&mut self.s.algo, Algo::Squarified, "Squarified").changed();
            });
            ui.horizontal(|ui| {
                ui.label("Density:");
                let mut d = self.s.density as f32;
                if ui.add(egui::Slider::new(&mut d, 0.0..=5.0).step_by(1.0).show_value(false)).changed() {
                    self.s.density = d as usize;
                    changed = true;
                }
                ui.label(DENSITY_LABELS[self.s.density.min(5)]);
            });
            ui.horizontal(|ui| {
                ui.label("Bias:");
                ui.label("Vert");
                changed |= ui.add(egui::Slider::new(&mut self.s.bias, -1.0..=1.0).show_value(false)).changed();
                ui.label("Horz");
            });
            changed |= ui.checkbox(&mut self.s.headers, "Folder title bars").changed();
            ui.separator();
            ui.strong("Display colors");
            egui::ComboBox::from_id_salt("scheme").selected_text(self.s.scheme.label()).show_ui(ui, |ui| {
                for s in Scheme::ALL {
                    if ui.selectable_value(&mut self.s.scheme, s, s.label()).changed() {
                        changed = true;
                    }
                }
            });
            changed |= ui.checkbox(&mut self.s.bevel, "3D shaded blocks").changed();
            changed |= ui.checkbox(&mut self.s.labels, "Show names inside blocks").changed();
            if ui.checkbox(&mut self.s.dark, "Dark theme").changed() {
                ctx.set_theme(if self.s.dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                changed = true;
            }
            ui.separator();
            ui.strong("3D view");
            changed |= ui.checkbox(&mut self.s.height_by_size, "Height proportional to file size").changed();
            changed |= ui.add(egui::Slider::new(&mut self.s.height_scale, 0.2..=3.0).text("Height scale")).changed();
            ui.separator();
            ui.strong("Miscellaneous");
            ui.checkbox(&mut self.s.animated, "Animated zoom in / zoom out");
            ui.checkbox(&mut self.s.tooltips, "Show tooltips");
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.checkbox(&mut self.s.size_on_disk, "Measure size on disk (clusters/blocks) – applies on next scan");
                ui.checkbox(&mut self.s.delete_enabled, "Enable the Delete command (permanent!)");
            }
        });
        if changed {
            self.rev += 1;
        }
        self.show_setup = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About SpaceHunter").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.heading("SpaceHunter");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label("A fast, modern take on the classic SpaceMonger disk-space visualiser.");
            ui.add_space(6.0);
            ui.label("• Parallel Rust scanner, squarified treemap, 2D and 3D views");
            ui.label("• Double-click a folder to zoom in · Backspace to zoom out");
            ui.label("• 3D: drag to orbit, right-drag to pan, wheel to zoom");
            ui.add_space(6.0);
            ui.hyperlink_to("github.com/quisen/SpaceHunter", "https://github.com/quisen/SpaceHunter");
        });
        self.show_about = open;
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(n), Some(t)) = (self.confirm_delete, self.tree()) {
            let path = self.full_path(&t, n);
            let mut close = false;
            egui::Window::new("Delete permanently?").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(path.display().to_string());
                ui.label(format!("{} will be removed and cannot be recovered.", fmt::size(t.node(n).size)));
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new("Delete").color(Color32::from_rgb(240, 90, 90))).clicked() {
                        match platform::delete(&path, t.node(n).is_dir) {
                            Ok(()) => {
                                self.toast = Some((format!("Deleted {}", path.display()), Instant::now()));
                                self.rescan(ctx);
                            }
                            Err(e) => self.toast = Some((format!("Windows failed to delete file: {e}"), Instant::now())),
                        }
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if close {
                self.confirm_delete = None;
            }
        }
        let _ = ctx;
    }

    fn tooltip(&self, ui: &mut egui::Ui, t: &Tree, cell: &Cell) {
        let vr = t.node(self.view_root).size.max(1);
        match cell.kind {
            CellKind::Free => {
                ui.strong("Free space");
                ui.label(fmt::size(cell.size));
            }
            CellKind::Lump => {
                ui.strong(format!("{} small items", cell.count));
                ui.label(format!("{} in total", fmt::size(cell.size)));
                ui.label(egui::RichText::new("Increase density in Setup, or zoom in to see them.").weak());
            }
            _ => {
                let n = t.node(cell.node);
                ui.strong(t.name(cell.node));
                ui.label(egui::RichText::new(t.path(cell.node, std::path::MAIN_SEPARATOR)).small().weak());
                ui.label(format!("{}  ({:.2}% of view)", fmt::size(n.size), n.size as f64 / vr as f64 * 100.0));
                if n.is_dir {
                    ui.label(format!("{} files", group(n.files as u64)));
                }
                let d = fmt::date(n.mtime);
                if !d.is_empty() {
                    ui.label(format!("Modified {d} UTC"));
                }
            }
        }
    }

    // ───────────────────────────── canvas ─────────────────────────────

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let bg = if self.s.dark { Color32::from_rgb(14, 16, 22) } else { Color32::from_rgb(232, 235, 241) };
        painter.rect_filled(rect, 0.0, bg);
        let canvas = Rect::from_min_size(Pos2::ZERO, rect.size());

        let Some(t) = self.tree() else {
            self.empty_state(ui, rect);
            return;
        };
        if rect.width() < 20.0 || rect.height() < 20.0 {
            return;
        }
        self.ensure_layout(&t, canvas);
        let cc = self.color_ctx(&t);

        // ── input shared by both modes
        let pointer = resp.hover_pos().map(|p| p - rect.min);
        let mut zoom_target: Option<NodeId> = None;
        let mut click_cell: Option<usize> = None;
        let mut dbl_cell: Option<usize> = None;

        match self.mode {
            Mode::D2 => {
                self.hover = if self.anim.is_some() { None } else { pointer.and_then(|p| hit_test(&self.cells, p.x, p.y)) };
                if resp.clicked() {
                    click_cell = self.hover;
                }
                if resp.double_clicked() {
                    dbl_cell = self.hover;
                }
                if resp.hovered() {
                    let dy = ctx.input(|i| i.smooth_scroll_delta.y);
                    self.scroll_acc += dy;
                    if self.scroll_acc > 60.0 {
                        self.scroll_acc = 0.0;
                        if let Some(h) = self.hover {
                            // zoom into the shallowest folder *below* the view root under the cursor
                            zoom_target = self.child_dir_under(&t, h);
                        }
                    } else if self.scroll_acc < -60.0 {
                        self.scroll_acc = 0.0;
                        self.zoom_out(canvas);
                    }
                }
            }
            Mode::D3 => {
                if self.cam_for != Some((canvas.width() as u32, canvas.height() as u32)) {
                    self.cam = render3d::Camera::for_map(canvas.width(), canvas.height());
                    self.cam_for = Some((canvas.width() as u32, canvas.height() as u32));
                }
                if resp.dragged_by(egui::PointerButton::Primary) {
                    let d = resp.drag_delta();
                    self.cam.yaw -= d.x * 0.008;
                    self.cam.pitch = (self.cam.pitch + d.y * 0.008).clamp(0.08, 1.52);
                }
                if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                    let d = resp.drag_delta() * (self.cam.dist / rect.height() * 1.1);
                    let (sy, cy) = (self.cam.yaw.sin(), self.cam.yaw.cos());
                    self.cam.target[0] += -d.x * cy - d.y * sy;
                    self.cam.target[2] += d.x * sy - d.y * cy;
                }
                if resp.hovered() {
                    let dy = ctx.input(|i| i.smooth_scroll_delta.y);
                    if dy != 0.0 {
                        self.cam.dist = (self.cam.dist * (1.0 - dy * 0.0015)).clamp(50.0, 20000.0);
                    }
                }
                self.ensure_boxes(&t, &cc, canvas);
                self.hover = None;
                if let (Some(p), Some(b)) = (pointer, self.boxes.as_ref()) {
                    if !resp.dragged() {
                        let (o, d) = render3d::ray(&self.cam, canvas.size(), p);
                        self.hover = render3d::pick(b, o, d).map(|i| b.cell_of[i] as usize);
                    }
                }
                if resp.clicked() {
                    click_cell = self.hover;
                }
                if resp.double_clicked() {
                    dbl_cell = self.hover;
                }
            }
        }

        if let Some(c) = click_cell {
            self.selected = self.node_of(c).or(self.selected);
            if let Some(c) = self.cells.get(c) {
                if c.kind == CellKind::Lump {
                    self.selected = Some(c.node);
                }
            }
        }
        if let Some(c) = dbl_cell {
            zoom_target = self.dir_of(&t, c);
        }
        if resp.secondary_clicked() && self.mode == Mode::D2 {
            self.ctx_target = self.hover.and_then(|h| self.node_of(h));
        }

        // ── draw
        match self.mode {
            Mode::D2 => self.draw_2d(&painter, rect, &t, &cc),
            Mode::D3 => self.draw_3d(&painter, rect, &t),
        }

        // hover / selection outlines (2D)
        if self.mode == Mode::D2 && self.anim.is_none() {
            if let Some(sel) = self.selected {
                if let Some(c) = self.cells.iter().rev().find(|c| c.node == sel && matches!(c.kind, CellKind::Dir | CellKind::File)) {
                    painter.rect_stroke(cell_rect(rect, c), 0.0, Stroke::new(2.0, Color32::from_rgb(255, 214, 64)), egui::StrokeKind::Inside);
                }
            }
            if let Some(h) = self.hover {
                painter.rect_stroke(cell_rect(rect, &self.cells[h]), 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
            }
        }

        // tooltip
        if self.s.tooltips && resp.hovered() && !resp.dragged() {
            if let Some(h) = self.hover {
                let cell = self.cells[h];
                resp.clone().on_hover_ui_at_pointer(|ui| self.tooltip(ui, &t, &cell));
            }
        }

        // context menu
        #[cfg(not(target_arch = "wasm32"))]
        if self.mode == Mode::D2 {
            let target = self.ctx_target;
            resp.context_menu(|ui| {
                if let Some(n) = target {
                    ui.label(egui::RichText::new(t.name(n)).strong());
                    if t.node(n).is_dir && ui.button("Zoom into folder").clicked() {
                        zoom_target = Some(n);
                        ui.close();
                    }
                    if ui.button("Show in file manager").clicked() {
                        platform::reveal(&self.full_path(&t, n), t.node(n).is_dir);
                        ui.close();
                    }
                    if ui.button("Copy path").clicked() {
                        ui.ctx().copy_text(t.path(n, std::path::MAIN_SEPARATOR));
                        ui.close();
                    }
                    if self.s.delete_enabled && ui.button("Delete…").clicked() {
                        self.confirm_delete = Some(n);
                        ui.close();
                    }
                } else {
                    ui.label("Nothing here");
                }
            });
        }

        if let Some(z) = zoom_target {
            self.zoom_to(z, canvas);
        }
    }

    /// The directory directly below the view root that contains the hovered cell's path.
    fn child_dir_under(&self, t: &Tree, cell: usize) -> Option<NodeId> {
        let mut n = self.node_of(cell)?;
        loop {
            let p = t.node(n).parent;
            if p == NONE {
                return None;
            }
            if p == self.view_root {
                return t.node(n).is_dir.then_some(n);
            }
            n = p;
        }
    }

    fn draw_2d(&mut self, painter: &egui::Painter, rect: Rect, t: &Tree, cc: &ColorCtx) {
        let ctx = painter.ctx().clone();
        if let Some(a) = &self.anim {
            let k = (a.start.elapsed().as_secs_f32() / 0.28).min(1.0);
            let e = 1.0 - (1.0 - k).powi(3);
            let r: Vec<f32> = (0..4).map(|i| a.from[i] + (a.to[i] - a.from[i]) * e).collect();
            let cells = self.layout_in(t, a.root, [0.0, 0.0, r[2].max(2.0), r[3].max(2.0)], None);
            let scene = render2d::build(&cells, t, cc, rect.min + egui::vec2(r[0], r[1]), self.s.bevel, self.s.labels);
            render2d::paint(painter, &scene, Vec2::ZERO);
            if k >= 1.0 {
                self.anim = None;
            }
            ctx.request_repaint();
            return;
        }
        let key = self.cells_key.map_or(0, |k| k.rev ^ (k.root as u64) << 20 ^ (k.w as u64) << 40 ^ (k.h as u64) << 52 ^ k.tree << 8);
        if self.scene.is_none() || self.scene_key != key {
            self.scene = Some(render2d::build(&self.cells, t, cc, Pos2::ZERO, self.s.bevel, self.s.labels));
            self.scene_key = key;
        }
        render2d::paint(painter, self.scene.as_ref().unwrap(), rect.min.to_vec2());
    }

    fn ensure_boxes(&mut self, t: &Tree, cc: &ColorCtx, canvas: Rect) {
        let key = self.cells_key.map_or(0, |k| k.rev ^ (k.root as u64) << 20 ^ k.tree << 8) ^ (self.s.height_scale.to_bits() as u64) << 32;
        if self.boxes.is_none() || self.boxes_key != key {
            let hs = render3d::HeightStyle { by_size: self.s.height_by_size, scale: self.s.height_scale };
            let b = render3d::build_boxes(&self.cells, t, cc, [canvas.width(), canvas.height()], &hs);
            let mut sh = self.shared3d.lock().unwrap();
            sh.data = b.data.clone();
            sh.rev += 1;
            self.boxes = Some(b);
            self.boxes_key = key;
        }
    }

    fn draw_3d(&mut self, painter: &egui::Painter, rect: Rect, _t: &Tree) {
        let Some(b) = self.boxes.as_ref() else { return };
        let inst_of = |cell: Option<usize>| cell.and_then(|c| b.cell_of.iter().position(|&x| x as usize == c)).map_or(-1, |i| i as i32);
        let hover = inst_of(self.hover);
        let sel = self.selected.and_then(|n| self.cells.iter().rposition(|c| c.node == n && matches!(c.kind, CellKind::Dir | CellKind::File)));
        let sel = inst_of(sel);
        {
            let mut sh = self.shared3d.lock().unwrap();
            sh.hover = hover;
            sh.sel = sel;
            sh.mvp = self.cam.view_proj(rect.width() / rect.height());
            let c = if self.s.dark { [0.055, 0.063, 0.086] } else { [0.91, 0.92, 0.945] };
            sh.clear = c;
        }
        render3d::paint(painter, rect, self.shared3d.clone());
        painter.text(rect.left_bottom() + egui::vec2(10.0, -8.0), Align2::LEFT_BOTTOM, "drag: orbit · right-drag: pan · wheel: zoom · double-click: enter folder", FontId::proportional(11.0), Color32::from_white_alpha(120));
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let size = egui::vec2(480.0, 340.0);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(rect.center(), size)).layout(egui::Layout::top_down(egui::Align::Center)));
        if let Some(j) = &self.job {
            let (label, p) = (j.label.clone(), j.progress.clone());
            let el = self.job_started.elapsed().as_secs_f32();
            let (files, dirs, bytes) = (p.files.load(Relaxed), p.dirs.load(Relaxed), p.bytes.load(Relaxed));
            let mut cancel = false;
            card().show(&mut child, |ui| {
                ui.set_width(size.x - 42.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Scanning").size(12.0).color(MUTED));
                    ui.label(egui::RichText::new(&label).strong().size(16.0));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(fmt::size(bytes)).size(40.0).strong().color(ACCENT));
                    ui.add_space(4.0);
                    // indeterminate sweep
                    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, HAIR);
                    let w = r.width() * 0.28;
                    let t = (el * 0.9).fract();
                    let x = r.left() + (r.width() + w) * t - w;
                    let seg = Rect::from_min_max(egui::pos2(x.max(r.left()), r.top()), egui::pos2((x + w).min(r.right()), r.bottom()));
                    if seg.width() > 0.0 {
                        ui.painter().rect_filled(seg, 3.0, ACCENT);
                    }
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(format!("{} files · {} folders · {:.0}s", group(files), group(dirs), el)).color(MUTED));
                    ui.add_space(12.0);
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if cancel {
                self.cancel_job();
            }
            return;
        }
        let mut open = false;
        let mut demo = false;
        #[allow(unused_mut)]
        let mut start: Option<std::path::PathBuf> = None;
        card().show(&mut child, |ui| {
            ui.set_width(size.x - 42.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("SpaceHunter").size(32.0).strong());
                ui.label(egui::RichText::new("See where your disk space went.").color(MUTED));
                ui.add_space(16.0);
                #[cfg(not(target_arch = "wasm32"))]
                let label = "Choose a folder or drive…";
                #[cfg(target_arch = "wasm32")]
                let label = "Choose a folder…";
                if ui.add_sized([ui.available_width(), 42.0], primary_button(label)).clicked() {
                    open = true;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("QUICK SCAN").size(10.5).color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        for (label, p) in platform::roots().into_iter().take(12) {
                            if ui.button(label).clicked() {
                                start = Some(p);
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                if ui.add(tool_button("Try with demo data")).clicked() {
                    demo = true;
                }
                #[cfg(target_arch = "wasm32")]
                ui.label(egui::RichText::new("…or drop a folder onto this page.\nFiles never leave your device.").color(MUTED).small());
            });
        });
        if open {
            self.open_dialog(&ctx);
        }
        if let Some(p) = start {
            self.start_path(&ctx, p);
        }
        if demo {
            self.job = Some(platform::demo(&ctx));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(t) = self.tree() {
                if let Some(h) = self.hover {
                    let c = self.cells[h];
                    match c.kind {
                        CellKind::Free => ui.label(format!("Free space: {}", fmt::size(c.size))),
                        CellKind::Lump => ui.label(format!("{} small items: {}", c.count, fmt::size(c.size))),
                        _ => ui.label(format!("{}   {}", t.path(c.node, std::path::MAIN_SEPARATOR), fmt::size(c.size))),
                    };
                } else {
                    ui.label(format!("{} files · {} total", group(t.node(self.view_root).files as u64), fmt::size(t.node(self.view_root).size)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((total, free)) = self.disk {
                        ui.label(format!("Disk: {} free of {} ({:.1}%)", fmt::size(free), fmt::size(total), free as f64 / total.max(1) as f64 * 100.0));
                        ui.separator();
                    }
                    ui.label(egui::RichText::new(format!("{} blocks · layout {:.1} ms", self.cells.len(), self.layout_ms)).weak());
                });
            } else {
                ui.label("Ready.");
            }
        });
    }
}

fn cell_rect(origin: Rect, c: &Cell) -> Rect {
    Rect::from_min_size(origin.min + egui::vec2(c.x, c.y), egui::vec2(c.w, c.h))
}

// ───────────────────────────── theme ─────────────────────────────

pub const BG: Color32 = Color32::from_rgb(14, 16, 22);
pub const PANEL: Color32 = Color32::from_rgb(19, 22, 30);
pub const CARD: Color32 = Color32::from_rgb(26, 30, 41);
pub const HAIR: Color32 = Color32::from_rgb(38, 43, 57);
pub const TEXT: Color32 = Color32::from_rgb(222, 228, 240);
pub const MUTED: Color32 = Color32::from_rgb(133, 143, 165);
pub const ACCENT: Color32 = Color32::from_rgb(98, 142, 255);
const ACCENT_DIM: Color32 = Color32::from_rgb(40, 58, 110);

fn apply_theme(ctx: &egui::Context) {
    let mut st = (*ctx.global_style()).clone();
    st.spacing.item_spacing = egui::vec2(8.0, 6.0);
    st.spacing.button_padding = egui::vec2(12.0, 6.0);
    st.spacing.interact_size.y = 28.0;
    st.spacing.window_margin = egui::Margin::same(16);
    let v = &mut st.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = CARD;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.window_stroke = Stroke::new(1.0, HAIR);
    v.window_corner_radius = 12.0.into();
    v.menu_corner_radius = 10.0.into();
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) };
    v.window_shadow = v.popup_shadow;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    let r: egui::CornerRadius = 8.0.into();
    for (w, fill) in [
        (&mut v.widgets.noninteractive, PANEL),
        (&mut v.widgets.inactive, CARD),
        (&mut v.widgets.hovered, Color32::from_rgb(36, 42, 58)),
        (&mut v.widgets.active, Color32::from_rgb(44, 52, 74)),
        (&mut v.widgets.open, Color32::from_rgb(36, 42, 58)),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, HAIR);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, HAIR);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(70, 82, 112));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    st.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(20.0));
    st.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.5));
    st.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.5));
    st.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
    ctx.set_global_style(st);
}

fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).strong().color(Color32::WHITE)).fill(ACCENT).stroke(Stroke::NONE)
}

fn tool_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(text.to_owned()).fill(Color32::TRANSPARENT).stroke(Stroke::NONE)
}

fn card() -> egui::Frame {
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, HAIR)).corner_radius(12.0).inner_margin(egui::Margin::same(20))
}

fn pill(ui: &mut egui::Ui, label: &str, sel: bool) -> egui::Response {
    let b = egui::Button::new(egui::RichText::new(label.to_owned()).color(if sel { Color32::WHITE } else { MUTED }))
        .fill(if sel { ACCENT_DIM } else { Color32::TRANSPARENT })
        .stroke(if sel { Stroke::new(1.0, ACCENT) } else { Stroke::NONE })
        .min_size(egui::vec2(40.0, 26.0));
    ui.add(b)
}

pub fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl eframe::App for SpaceHunter {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_job(&ctx);
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // keyboard
        let (open, rescan, back, home, f2, f3, enter) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.key_pressed(egui::Key::F5),
                i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::Num2),
                i.key_pressed(egui::Key::Num3),
                i.key_pressed(egui::Key::Enter),
            )
        });
        let fake_canvas = Rect::from_min_size(Pos2::ZERO, ctx.content_rect().size());
        let canvas = self.cells_key.map_or(fake_canvas, |k| Rect::from_min_size(Pos2::ZERO, egui::vec2(k.w as f32, k.h as f32)));
        if open {
            self.open_dialog(&ctx);
        }
        if rescan {
            self.rescan(&ctx);
        }
        if back {
            self.zoom_out(canvas);
        }
        if home {
            self.zoom_full(canvas);
        }
        if f2 {
            self.mode = Mode::D2;
        }
        if f3 {
            self.mode = Mode::D3;
        }
        if enter {
            if let Some(n) = self.selected {
                self.zoom_to(n, canvas);
            }
        }

        egui::Panel::top("toolbar").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 8)).stroke(Stroke::new(1.0, HAIR))).show(ui, |ui| {
            self.toolbar(ui, canvas);
            if self.tree.is_some() {
                ui.add_space(4.0);
                self.breadcrumb(ui, canvas);
            }
        });
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 5)).stroke(Stroke::new(1.0, HAIR))).show(ui, |ui| {
            self.status_bar(ui);
        });
        if self.show_side && self.tree.is_some() {
            egui::Panel::right("side").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(14)).stroke(Stroke::new(1.0, HAIR))).default_size(330.0).size_range(240.0..=560.0).show(ui, |ui| {
                self.side_panel(ui, canvas);
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            self.canvas(ui);
        });

        self.setup_window(&ctx);
        self.about_window(&ctx);
        self.delete_dialog(&ctx);

        if let Some((msg, t0)) = &self.toast {
            if t0.elapsed().as_secs_f32() < 4.0 {
                egui::Area::new(egui::Id::new("toast")).anchor(Align2::CENTER_BOTTOM, [0.0, -40.0]).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).fill(CARD).corner_radius(10.0).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| ui.label(msg));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            } else {
                self.toast = None;
            }
        }

        self.screenshot_hook(&ctx);
    }

    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl SpaceHunter {
    /// `--screenshot out.png`: render a few frames after the data is in, save and quit (used for testing).
    fn screenshot_hook(&mut self, ctx: &egui::Context) {
        let Some(path) = self.args.screenshot.clone() else { return };
        if self.tree.is_none() {
            return;
        }
        self.frames_since_load += 1;
        ctx.request_repaint();
        if self.args.zoom_first && self.frames_since_load == 3 {
            if let Some(t) = self.tree() {
                if let Some(&c) = t.children(0).iter().find(|&&c| t.node(c).is_dir) {
                    self.anim = None;
                    self.view_root = c;
                }
            }
        }
        if self.frames_since_load == 12 && !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Screenshot { image, .. } = e {
                    let mut buf = Vec::with_capacity(image.pixels.len() * 4);
                    for p in &image.pixels {
                        buf.extend_from_slice(&p.to_array());
                    }
                    if let Some(img) = image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, buf) {
                        let _ = img.save(&path);
                    }
                    std::process::exit(0);
                }
            }
        });
        let _ = path;
    }
}

#[allow(dead_code)]
fn _unused(_: &Mutex<()>) {}
