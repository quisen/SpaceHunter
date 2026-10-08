use crate::i18n::Language;
macro_rules! localized_format {
    ($lang:expr, $en:literal, $pt:literal $(, $arg:expr)* $(,)?) => {
        if $lang == Language::PtBr { format!($pt $(, $arg)*) } else { format!($en $(, $arg)*) }
    };
}
use crate::colors::{max_file_size, ColorCtx, Scheme};
use crate::platform::{self, Job, Loaded};
use crate::{render2d, render3d};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use spacehunter_core::{
    fmt, hit_test, layout, layout_classic, Cell, CellKind, ClassicOptions, LayoutOptions, NodeId,
    Tree, DENSITY, NONE,
};
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
    pub overview: bool,
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

const DENSITY_AREA: [f32; 6] = [3.0, 10.0, 22.0, 40.0, 110.0, 320.0];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            overview: true,
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
    mode: Mode,
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
    language: Language,
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
            language: Language::detect(),
            args,
        };
        #[cfg(target_arch = "wasm32")]
        {
            crate::web::install_drop(&cc.egui_ctx, app.drop_slot.clone());
            // `?setup` opens the settings panel straight away (handy for screenshots).
            let search = web_sys::window()
                .and_then(|w| w.location().search().ok())
                .unwrap_or_default();
            app.show_setup = search.contains("setup");
        }
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
        self.job = Some(platform::start_scan(
            ctx,
            p,
            platform::default_opts(self.s.size_on_disk),
        ));
    }
    #[cfg(target_arch = "wasm32")]
    fn start_path(&mut self, ctx: &egui::Context, _p: std::path::PathBuf) {
        self.job = Some(platform::demo(ctx));
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = platform::pick_folder(self.language.text("Choose a folder or drive…")) {
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
        }
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let lang = self.language;
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
            self.toast = Some((
                if sz == 0 {
                    localized_format!(lang, "Scan finished but found no readable files ({n} items) – check permissions", "Análise concluída sem arquivos legíveis ({n} itens) – verifique as permissões")
                } else {
                    localized_format!(
                        lang,
                        "Loaded {} items, {}",
                        "Carregados {} itens, {}",
                        group(n as u64),
                        fmt::size(sz)
                    )
                },
                Instant::now(),
            ));
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
                    self.toast = Some((
                        localized_format!(
                            lang,
                            "Could not load: {e}",
                            "Não foi possível carregar: {e}"
                        ),
                        Instant::now(),
                    ));
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
        if self.s.animated && self.mode == Mode::D2 && !self.s.overview {
            // grow from the folder's current rectangle
            if let Some(c) = self
                .cells
                .iter()
                .find(|c| c.node == node && c.kind == CellKind::Dir)
            {
                self.anim = Some(Anim {
                    root: node,
                    from: [c.x, c.y, c.w, c.h],
                    to: [0.0, 0.0, canvas.width(), canvas.height()],
                    start: Instant::now(),
                    zoom_out: false,
                });
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
        if self.s.animated && self.mode == Mode::D2 && !self.s.overview {
            // relayout the parent now to find where `old` sits, then shrink into it
            let cells = self.compute_layout(&t, p, canvas);
            if let Some(c) = cells
                .iter()
                .find(|c| c.node == old && c.kind == CellKind::Dir)
            {
                self.anim = Some(Anim {
                    root: old,
                    from: [0.0, 0.0, canvas.width(), canvas.height()],
                    to: [c.x, c.y, c.w, c.h],
                    start: Instant::now(),
                    zoom_out: true,
                });
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
        let free = if self.s.show_free && root == 0 {
            self.disk.map(|d| d.1)
        } else {
            None
        };
        self.layout_in(t, root, [0.0, 0.0, canvas.width(), canvas.height()], free)
    }

    fn layout_in(&self, t: &Tree, root: NodeId, rect: [f32; 4], free: Option<u64>) -> Vec<Cell> {
        if self.mode == Mode::D2 && self.s.overview {
            let mut cells = layout(
                t,
                root,
                rect,
                free,
                &LayoutOptions {
                    max_depth: 1,
                    min_area: (rect[2] * rect[3] * 0.008).max(1200.0),
                    header: 0.0,
                    pad: 0.0,
                    ..Default::default()
                },
            );
            // The root is a background frame; only its children are interactive.
            cells.retain(|c| c.kind != CellKind::Dir || c.node != root);
            return cells;
        }
        match self.s.algo {
            Algo::Squarified => layout(t, root, rect, free, &self.layout_opts()),
            Algo::Classic => {
                let (min_w, min_h) = DENSITY[self.s.density.min(5)];
                // bias slider: -1..1 -> original -20..20
                layout_classic(
                    t,
                    root,
                    rect,
                    free,
                    &ClassicOptions {
                        min_w,
                        min_h,
                        bias: (self.s.bias * 20.0).round() as i32,
                    },
                )
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
        ColorCtx {
            depth_base: t.depth(self.view_root) as u16,
            scheme: self.s.scheme,
            dark: self.s.dark,
            max_size,
        }
    }

    fn ensure_layout(&mut self, t: &Tree, canvas: Rect) {
        let key = LayoutKey {
            mode: self.mode,
            root: self.view_root,
            w: canvas.width() as u32,
            h: canvas.height() as u32,
            rev: self.rev,
            tree: self.tree_id,
        };
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
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, NodeId)>> =
            Default::default();
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

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        let ctx = ui.ctx().clone();
        let has = self.tree.is_some();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Space Hunter").strong().size(15.0).color(TEXT));
            ui.add_space(12.0);
            #[cfg(not(target_arch = "wasm32"))]
            {
                if ui.add(primary_button(lang.text("Open folder"))).on_hover_text(lang.text("Choose a folder or drive (Ctrl+O)")).clicked() {
                    self.open_dialog(&ctx);
                }
                ui.menu_button(lang.text("Drives"), |ui| {
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
                if ui.add(primary_button(lang.text("Open folder"))).on_hover_text(lang.text("Choose a folder to analyse (Ctrl+O). Nothing is uploaded – everything stays in your browser.")).clicked() {
                    self.open_dialog(&ctx);
                }
                if ui.add(tool_button(lang.text("Demo"))).clicked() {
                    self.job = Some(platform::demo(&ctx));
                }
            }
            if has && ui.add(tool_button(lang.text("Refresh"))).on_hover_text(lang.text("Scan again (F5)")).clicked() {
                self.rescan(&ctx);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill(ui, lang.text("Settings"), self.show_setup).clicked() {
                    self.show_setup = !self.show_setup;
                }
                if has {
                    if pill(ui, lang.text("Details"), self.show_side && !self.show_setup).clicked() {
                        if self.show_setup {
                            self.show_setup = false;
                            self.show_side = true;
                        } else {
                            self.show_side = !self.show_side;
                        }
                    }
                    ui.add_space(6.0);
                    segmented(ui, &mut self.mode, &[(Mode::D2, "2D"), (Mode::D3, "3D")]);
                }
            });
        });
    }

    fn zoom_full(&mut self, canvas: Rect) {
        while self.view_root != 0 {
            let p = self
                .tree
                .as_ref()
                .map_or(0, |t| t.node(self.view_root).parent);
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
        let lang = self.language;
        let mut chain = Vec::new();
        let mut n = self.view_root;
        while n != NONE {
            chain.push(n);
            n = t.node(n).parent;
        }
        chain.reverse();
        let mut target = None;
        let mut up = false;
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                self.view_root != 0,
                egui::Button::new(egui::RichText::new("‹").size(20.0))
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .min_size(egui::vec2(30.0, 26.0)),
            );
            if r.on_hover_text(lang.text("Zoom out (Backspace)")).clicked() {
                up = true;
            }
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, &n) in chain.iter().enumerate() {
                if i > 0 {
                    ui.label(egui::RichText::new("/").color(HAIR_STRONG));
                }
                let last = i + 1 == chain.len();
                let text = egui::RichText::new(t.name(n).to_owned()).color(if last {
                    TEXT
                } else {
                    MUTED
                });
                let text = if last { text.strong() } else { text };
                if ui.add(egui::Button::new(text).frame(false)).clicked() && !last {
                    target = Some(n);
                }
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new(fmt::size(t.node(self.view_root).size)).color(MUTED));
            if self.mode == Mode::D2 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut ov = self.s.overview;
                    if segmented(
                        ui,
                        &mut ov,
                        &[
                            (true, lang.text("Overview")),
                            (false, lang.text("All files")),
                        ],
                    ) {
                        self.s.overview = ov;
                        self.rev += 1;
                        self.anim = None;
                    }
                });
            }
        });
        if up {
            self.zoom_out(canvas);
        }
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
        let lang = self.language;
        let Some(t) = self.tree() else {
            ui.label(lang.text("Nothing loaded."));
            return;
        };
        let mut go_into: Option<NodeId> = None;
        #[allow(unused_mut)]
        let mut run = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(n) = self.selected.or(Some(self.view_root)) {
                    let node = t.node(n);
                    ui.label(
                        egui::RichText::new(t.name(n).to_owned())
                            .size(17.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(t.path(n, std::path::MAIN_SEPARATOR))
                            .small()
                            .color(MUTED),
                    );
                    ui.add_space(8.0);
                    let rootsz = t.node(self.view_root).size.max(1);
                    let pct = node.size as f64 / rootsz as f64 * 100.0;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(fmt::size(node.size))
                                .size(26.0)
                                .strong()
                                .color(ACCENT),
                        )
                        .on_hover_text(format!("{} bytes", group(node.size)));
                        if n != self.view_root {
                            ui.label(
                                egui::RichText::new(localized_format!(
                                    lang,
                                    "{:.1}% of this view",
                                    "{:.1}% desta vista",
                                    pct
                                ))
                                .color(MUTED),
                            );
                        }
                    });
                    let mut facts = Vec::new();
                    if node.is_dir {
                        let (tid, cn, v) = self.dirs_cache.get();
                        let dirs = if tid == self.tree_id && cn == n {
                            v
                        } else {
                            let v = t.dir_count(n);
                            self.dirs_cache.set((self.tree_id, n, v));
                            v
                        };
                        facts.push(localized_format!(
                            lang,
                            "{} files",
                            "{} arquivos",
                            group(node.files as u64)
                        ));
                        facts.push(localized_format!(
                            lang,
                            "{} folders",
                            "{} pastas",
                            group(dirs)
                        ));
                    }
                    let d = fmt::date(node.mtime);
                    if !d.is_empty() {
                        facts.push(localized_format!(
                            lang,
                            "modified {}",
                            "modificado em {}",
                            d
                        ));
                    }
                    if !facts.is_empty() {
                        ui.label(egui::RichText::new(facts.join(" · ")).color(MUTED));
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if node.is_dir
                            && n != self.view_root
                            && ui.button(lang.text("Open this folder")).clicked()
                        {
                            go_into = Some(n);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if self.root_path.is_some() && n != 0 {
                            let label = if node.is_dir {
                                lang.text("Show in file manager")
                            } else {
                                lang.text("Open file")
                            };
                            if ui.button(label).clicked() {
                                run = true;
                            }
                            if self.s.delete_enabled
                                && ui
                                    .add(egui::Button::new(
                                        egui::RichText::new(lang.text("Delete…")).color(DANGER),
                                    ))
                                    .clicked()
                            {
                                self.confirm_delete = Some(n);
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                section(ui, lang.text("IN THIS FOLDER"));
                let vr = t.node(self.view_root).size.max(1);
                let mut go = None;
                for &c in t.children(self.view_root).iter().take(40) {
                    let node = t.node(c);
                    let frac = node.size as f32 / vr as f32;
                    let (r, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 24.0),
                        Sense::click(),
                    );
                    let p = ui.painter_at(r);
                    let bar = Rect::from_min_size(
                        r.min,
                        egui::vec2(r.width() * frac.max(0.004), r.height()),
                    );
                    p.rect_filled(
                        bar,
                        5.0,
                        if node.is_dir {
                            Color32::from_rgba_unmultiplied(98, 142, 255, 60)
                        } else {
                            Color32::from_rgba_unmultiplied(240, 170, 90, 55)
                        },
                    );
                    if resp.hovered() || self.selected == Some(c) {
                        p.rect_stroke(r, 5.0, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
                    }
                    p.text(
                        r.left_center() + egui::vec2(8.0, 0.0),
                        Align2::LEFT_CENTER,
                        if node.is_dir {
                            format!("{}/", t.name(c))
                        } else {
                            t.name(c).to_owned()
                        },
                        FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    p.text(
                        r.right_center() - egui::vec2(8.0, 0.0),
                        Align2::RIGHT_CENTER,
                        fmt::size(node.size),
                        FontId::proportional(11.0),
                        ui.visuals().weak_text_color(),
                    );
                    if resp.clicked() {
                        self.selected = Some(c);
                    }
                    if resp.double_clicked() && node.is_dir {
                        go = Some(c);
                    }
                }
                if let Some(g) = go.or(go_into.take()) {
                    self.zoom_to(g, canvas);
                }
                section(ui, lang.text("LARGEST FILES"));
                self.top_files(&t);
                for &f in &self.top_files.clone() {
                    let node = t.node(f);
                    let r = ui.selectable_label(
                        self.selected == Some(f),
                        format!("{}  —  {}", fmt::size(node.size), t.name(f)),
                    );
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
        #[cfg(not(target_arch = "wasm32"))]
        if run {
            self.run_selected();
        }
        let _ = run;
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(lang.text("Settings"))
                    .size(18.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("×").size(20.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .min_size(egui::vec2(30.0, 30.0)),
                    )
                    .on_hover_text(lang.text("Close"))
                    .clicked()
                {
                    self.show_setup = false;
                }
            });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                section(ui, lang.text("MAP"));
                field(
                    ui,
                    lang.text("Layout"),
                    lang.text("Classic is SpaceMonger's look. Squares are easier to compare."),
                );
                changed |= segmented(
                    ui,
                    &mut self.s.algo,
                    &[
                        (Algo::Classic, lang.text("Classic")),
                        (Algo::Squarified, lang.text("Squares")),
                    ],
                );
                field(
                    ui,
                    lang.text("Detail"),
                    lang.text("How many small files get a block of their own."),
                );
                let mut d = match self.s.density {
                    0 => 3,
                    1 | 2 => 2,
                    3 => 1,
                    _ => 0,
                };
                if segmented(
                    ui,
                    &mut d,
                    &[
                        (0, lang.text("Less")),
                        (1, lang.text("Normal")),
                        (2, lang.text("More")),
                        (3, lang.text("Max")),
                    ],
                ) {
                    self.s.density = [4, 3, 1, 0][d];
                    changed = true;
                }
                changed |= switch_row(ui, lang.text("Folder titles"), None, &mut self.s.headers);
                let free_hint = if self.disk.is_some() {
                    None
                } else {
                    Some(lang.text("Available when you scan a whole drive."))
                };
                ui.add_enabled_ui(self.disk.is_some(), |ui| {
                    changed |= switch_row(
                        ui,
                        lang.text("Show free space"),
                        free_hint,
                        &mut self.s.show_free,
                    );
                });

                section(ui, lang.text("APPEARANCE"));
                field(ui, lang.text("Colors"), "");
                ui.horizontal_wrapped(|ui| {
                    for sc in Scheme::ALL {
                        if pill(ui, lang.text(sc.label()), self.s.scheme == sc).clicked()
                            && self.s.scheme != sc
                        {
                            self.s.scheme = sc;
                            changed = true;
                        }
                    }
                });
                changed |= switch_row(ui, lang.text("Names on blocks"), None, &mut self.s.labels);
                changed |= switch_row(ui, lang.text("Shading"), None, &mut self.s.bevel);
                switch_row(ui, lang.text("Animations"), None, &mut self.s.animated);
                switch_row(ui, lang.text("Tips on hover"), None, &mut self.s.tooltips);

                section(ui, lang.text("3D VIEW"));
                changed |= switch_row(
                    ui,
                    lang.text("Height by size"),
                    Some(lang.text("Bigger files become taller towers.")),
                    &mut self.s.height_by_size,
                );
                field(ui, lang.text("Tower height"), "");
                let mut h = if self.s.height_scale < 0.75 {
                    0
                } else if self.s.height_scale < 1.5 {
                    1
                } else {
                    2
                };
                if segmented(
                    ui,
                    &mut h,
                    &[
                        (0, lang.text("Low")),
                        (1, lang.text("Medium")),
                        (2, lang.text("High")),
                    ],
                ) {
                    self.s.height_scale = [0.5, 1.0, 2.0][h];
                    changed = true;
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    section(ui, lang.text("SCAN"));
                    switch_row(
                        ui,
                        lang.text("Size on disk"),
                        Some(lang.text(
                            "Counts the space really used on the drive. Applies on the next scan.",
                        )),
                        &mut self.s.size_on_disk,
                    );
                    switch_row(
                        ui,
                        lang.text("Allow deleting"),
                        Some(lang.text(
                            "Adds a Delete button to the details panel. Deleting is permanent.",
                        )),
                        &mut self.s.delete_enabled,
                    );
                }

                section(ui, lang.text("LANGUAGE"));
                let mut l = self.language;
                if segmented(
                    ui,
                    &mut l,
                    &[(Language::PtBr, "Português"), (Language::EnUs, "English")],
                ) {
                    self.language = l;
                    l.save();
                    self.scene = None;
                    changed = true;
                }

                ui.add_space(18.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(localized_format!(
                        lang,
                        "Space Hunter {} · free and open source",
                        "Space Hunter {} · gratuito e de código aberto",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .color(MUTED)
                    .small(),
                );
                ui.label(
                    egui::RichText::new(lang.text(
                        "Double-click: open folder · Backspace: back · Ctrl+O: open · F5: refresh",
                    ))
                    .color(MUTED)
                    .small(),
                );
                ui.hyperlink_to(
                    egui::RichText::new("github.com/quisen/SpaceHunter").small(),
                    "https://github.com/quisen/SpaceHunter",
                );
            });
        if changed {
            self.rev += 1;
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        let lang = self.language;
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(n), Some(t)) = (self.confirm_delete, self.tree()) {
            let path = self.full_path(&t, n);
            let mut close = false;
            egui::Window::new(lang.text("Delete permanently?"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(path.display().to_string());
                    ui.label(localized_format!(
                        lang,
                        "{} will be removed and cannot be recovered.",
                        "{} serão excluídos permanentemente.",
                        fmt::size(t.node(n).size)
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                egui::RichText::new(lang.text("Delete"))
                                    .color(Color32::from_rgb(240, 90, 90)),
                            )
                            .clicked()
                        {
                            match platform::delete(&path, t.node(n).is_dir) {
                                Ok(()) => {
                                    self.toast = Some((
                                        localized_format!(
                                            lang,
                                            "Deleted {}",
                                            "Excluído: {}",
                                            path.display()
                                        ),
                                        Instant::now(),
                                    ));
                                    self.rescan(ctx);
                                }
                                Err(e) => {
                                    self.toast = Some((
                                        localized_format!(
                                            lang,
                                            "Windows failed to delete file: {e}",
                                            "Falha ao excluir arquivo: {e}"
                                        ),
                                        Instant::now(),
                                    ))
                                }
                            }
                            close = true;
                        }
                        if ui.button(lang.text("Cancel")).clicked() {
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
        let lang = self.language;
        let vr = t.node(self.view_root).size.max(1);
        match cell.kind {
            CellKind::Free => {
                ui.strong(lang.text("Free space"));
                ui.label(fmt::size(cell.size));
            }
            CellKind::Lump => {
                ui.strong(localized_format!(
                    lang,
                    "{} small items",
                    "{} itens pequenos",
                    cell.count
                ));
                ui.label(localized_format!(
                    lang,
                    "{} in total",
                    "{} no total",
                    fmt::size(cell.size)
                ));
                ui.label(
                    egui::RichText::new(lang.text(if self.s.overview && self.mode == Mode::D2 {
                        "Double-click to see these items in All files."
                    } else {
                        "Increase density in Setup, or zoom in to see them."
                    }))
                    .weak(),
                );
            }
            _ => {
                let n = t.node(cell.node);
                ui.strong(t.name(cell.node));
                ui.label(
                    egui::RichText::new(t.path(cell.node, std::path::MAIN_SEPARATOR))
                        .small()
                        .weak(),
                );
                ui.label(localized_format!(
                    lang,
                    "{}  ({:.2}% of view)",
                    "{}  ({:.2}% da visualização)",
                    fmt::size(n.size),
                    n.size as f64 / vr as f64 * 100.0
                ));
                if n.is_dir {
                    ui.label(localized_format!(
                        lang,
                        "{} files",
                        "{} arquivos",
                        group(n.files as u64)
                    ));
                }
                let d = fmt::date(n.mtime);
                if !d.is_empty() {
                    ui.label(localized_format!(
                        lang,
                        "Modified {d} UTC",
                        "Modificado em {d} UTC"
                    ));
                }
            }
        }
    }

    // ───────────────────────────── canvas ─────────────────────────────

    fn canvas(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        let lang = self.language;
        let ctx = ui.ctx().clone();
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let bg = if self.s.dark {
            Color32::from_rgb(14, 16, 22)
        } else {
            Color32::from_rgb(232, 235, 241)
        };
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
                self.hover = if self.anim.is_some() {
                    None
                } else {
                    pointer.and_then(|p| hit_test(&self.cells, p.x, p.y))
                };
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
                if resp.dragged_by(egui::PointerButton::Secondary)
                    || resp.dragged_by(egui::PointerButton::Middle)
                {
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
            if self.s.overview && self.cells[c].kind == CellKind::Lump {
                self.s.overview = false;
                self.rev += 1;
                self.anim = None;
            } else {
                zoom_target = self.dir_of(&t, c);
            }
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
                if let Some(c) = self
                    .cells
                    .iter()
                    .rev()
                    .find(|c| c.node == sel && matches!(c.kind, CellKind::Dir | CellKind::File))
                {
                    painter.rect_stroke(
                        cell_rect(rect, c),
                        0.0,
                        Stroke::new(2.0, Color32::from_rgb(255, 214, 64)),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            if let Some(h) = self.hover {
                painter.rect_stroke(
                    cell_rect(rect, &self.cells[h]),
                    0.0,
                    Stroke::new(1.5, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
        }

        // tooltip
        if self.s.tooltips && resp.hovered() && !resp.dragged() {
            if let Some(h) = self.hover {
                let cell = self.cells[h];
                resp.clone()
                    .on_hover_ui_at_pointer(|ui| self.tooltip(ui, &t, &cell));
            }
        }

        // context menu
        #[cfg(not(target_arch = "wasm32"))]
        if self.mode == Mode::D2 {
            let target = self.ctx_target;
            resp.context_menu(|ui| {
                if let Some(n) = target {
                    ui.label(egui::RichText::new(t.name(n)).strong());
                    if t.node(n).is_dir && ui.button(lang.text("Zoom into folder")).clicked() {
                        zoom_target = Some(n);
                        ui.close();
                    }
                    if ui.button(lang.text("Show in file manager")).clicked() {
                        platform::reveal(&self.full_path(&t, n), t.node(n).is_dir);
                        ui.close();
                    }
                    if ui.button(lang.text("Copy path")).clicked() {
                        ui.ctx().copy_text(t.path(n, std::path::MAIN_SEPARATOR));
                        ui.close();
                    }
                    if self.s.delete_enabled && ui.button(lang.text("Delete…")).clicked() {
                        self.confirm_delete = Some(n);
                        ui.close();
                    }
                } else {
                    ui.label(lang.text("Nothing here"));
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
        if self.s.overview {
            if self.scene.is_none() {
                self.scene = Some(render2d::build_overview(
                    &self.cells,
                    t,
                    cc.dark,
                    self.s.labels,
                    self.language,
                ));
            }
            render2d::paint(painter, self.scene.as_ref().unwrap(), rect.min.to_vec2());
            return;
        }
        if let Some(a) = &self.anim {
            let k = (a.start.elapsed().as_secs_f32() / 0.28).min(1.0);
            let e = 1.0 - (1.0 - k).powi(3);
            let r: Vec<f32> = (0..4)
                .map(|i| a.from[i] + (a.to[i] - a.from[i]) * e)
                .collect();
            let cells = self.layout_in(t, a.root, [0.0, 0.0, r[2].max(2.0), r[3].max(2.0)], None);
            let scene = render2d::build(
                &cells,
                t,
                cc,
                rect.min + egui::vec2(r[0], r[1]),
                self.s.bevel,
                self.s.labels,
                self.language,
            );
            render2d::paint(painter, &scene, Vec2::ZERO);
            if k >= 1.0 {
                self.anim = None;
            }
            ctx.request_repaint();
            return;
        }
        let key = self.cells_key.map_or(0, |k| {
            k.rev ^ (k.root as u64) << 20 ^ (k.w as u64) << 40 ^ (k.h as u64) << 52 ^ k.tree << 8
        });
        if self.scene.is_none() || self.scene_key != key {
            self.scene = Some(render2d::build(
                &self.cells,
                t,
                cc,
                Pos2::ZERO,
                self.s.bevel,
                self.s.labels,
                self.language,
            ));
            self.scene_key = key;
        }
        render2d::paint(painter, self.scene.as_ref().unwrap(), rect.min.to_vec2());
    }

    fn ensure_boxes(&mut self, t: &Tree, cc: &ColorCtx, canvas: Rect) {
        let key = self
            .cells_key
            .map_or(0, |k| k.rev ^ (k.root as u64) << 20 ^ k.tree << 8)
            ^ (self.s.height_scale.to_bits() as u64) << 32;
        if self.boxes.is_none() || self.boxes_key != key {
            let hs = render3d::HeightStyle {
                by_size: self.s.height_by_size,
                scale: self.s.height_scale,
            };
            let b =
                render3d::build_boxes(&self.cells, t, cc, [canvas.width(), canvas.height()], &hs);
            let mut sh = self.shared3d.lock().unwrap();
            sh.data = b.data.clone();
            sh.rev += 1;
            self.boxes = Some(b);
            self.boxes_key = key;
        }
    }

    fn draw_3d(&mut self, painter: &egui::Painter, rect: Rect, t: &Tree) {
        let lang = self.language;
        let Some(b) = self.boxes.as_ref() else { return };
        let inst_of = |cell: Option<usize>| {
            cell.and_then(|c| b.cell_of.iter().position(|&x| x as usize == c))
                .map_or(-1, |i| i as i32)
        };
        let hover = inst_of(self.hover);
        let sel = self.selected.and_then(|n| {
            self.cells
                .iter()
                .rposition(|c| c.node == n && matches!(c.kind, CellKind::Dir | CellKind::File))
        });
        let sel = inst_of(sel);
        {
            let mut sh = self.shared3d.lock().unwrap();
            sh.hover = hover;
            sh.sel = sel;
            sh.mvp = self.cam.view_proj(rect.width() / rect.height());
            let c = if self.s.dark {
                [0.055, 0.063, 0.086]
            } else {
                [0.91, 0.92, 0.945]
            };
            sh.clear = c;
        }
        render3d::paint(painter, rect, self.shared3d.clone());
        if self.s.labels {
            self.labels_3d(painter, rect, t);
        }
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -8.0),
            Align2::LEFT_BOTTOM,
            lang.text("drag: orbit · right-drag: pan · wheel: zoom · double-click: enter folder"),
            FontId::proportional(11.0),
            Color32::from_white_alpha(120),
        );
    }

    /// Project the top face of each box to the screen and draw names where they fit (largest first, no overlaps).
    fn labels_3d(&self, painter: &egui::Painter, rect: Rect, t: &Tree) {
        let lang = self.language;
        let Some(b) = self.boxes.as_ref() else { return };
        let m = self.cam.view_proj(rect.width() / rect.height());
        let proj = |x: f32, y: f32, z: f32| -> Option<Pos2> {
            let w = m[3] * x + m[7] * y + m[11] * z + m[15];
            if w <= 1e-4 {
                return None;
            }
            let nx = (m[0] * x + m[4] * y + m[8] * z + m[12]) / w;
            let ny = (m[1] * x + m[5] * y + m[9] * z + m[13]) / w;
            Some(Pos2::new(
                rect.min.x + (nx * 0.5 + 0.5) * rect.width(),
                rect.min.y + (0.5 - ny * 0.5) * rect.height(),
            ))
        };
        // (priority, position, width available, text)
        let mut cand: Vec<(f32, Pos2, f32, String, bool)> = Vec::new();
        for (i, d) in b.data.chunks_exact(9).enumerate() {
            let c = self.cells[b.cell_of[i] as usize];
            let (x, z, w, dd, top) = (d[0], d[1], d[2], d[3], d[4] + d[5]);
            let (Some(a), Some(bb), Some(cc), Some(dd2)) = (
                proj(x, top, z),
                proj(x + w, top, z),
                proj(x + w, top, z + dd),
                proj(x, top, z + dd),
            ) else {
                continue;
            };
            let min = Pos2::new(
                a.x.min(bb.x).min(cc.x).min(dd2.x),
                a.y.min(bb.y).min(cc.y).min(dd2.y),
            );
            let max = Pos2::new(
                a.x.max(bb.x).max(cc.x).max(dd2.x),
                a.y.max(bb.y).max(cc.y).max(dd2.y),
            );
            let (sw, sh) = (max.x - min.x, max.y - min.y);
            if sw < 46.0 || sh < 13.0 || !rect.intersects(Rect::from_min_max(min, max)) {
                continue;
            }
            let text = match c.kind {
                CellKind::Free => lang.text("Free space").to_owned(),
                CellKind::Lump => {
                    localized_format!(lang, "{} small items", "{} itens pequenos", c.count)
                }
                _ => t.name(c.node).to_owned(),
            };
            let prio = sw * sh * if c.kind == CellKind::Dir { 4.0 } else { 1.0 };
            // folders: label the near-left corner of the slab; leaves: centred on the top face
            let pos = if c.kind == CellKind::Dir {
                Pos2::new(min.x + 4.0, min.y + 2.0)
            } else {
                Pos2::new(min.x + sw / 2.0, min.y + sh / 2.0)
            };
            cand.push((prio, pos, sw - 6.0, text, c.kind == CellKind::Dir));
        }
        cand.sort_by(|a, b| b.0.total_cmp(&a.0));
        let font = FontId::proportional(11.0);
        let mut placed: Vec<Rect> = Vec::new();
        for (_, pos, avail, text, left) in cand.into_iter().take(1500) {
            if placed.len() >= 70 {
                break;
            }
            let max_chars = (avail / 6.0) as usize;
            if max_chars < 4 {
                continue;
            }
            let shown = if text.chars().count() > max_chars {
                text.chars().take(max_chars - 1).collect::<String>() + "…"
            } else {
                text
            };
            let w = shown.chars().count() as f32 * 6.0;
            let x0 = if left { pos.x } else { pos.x - w / 2.0 };
            let r = Rect::from_min_size(
                Pos2::new(x0, pos.y - 7.0 + if left { 7.0 } else { 0.0 }),
                egui::vec2(w, 14.0),
            );
            let r = if r.min.x < rect.min.x || r.max.x > rect.max.x {
                continue;
            } else {
                r
            };
            if placed.iter().any(|p| p.intersects(r.expand(2.0))) {
                continue;
            }
            painter.text(
                r.center() + egui::vec2(1.0, 1.0),
                Align2::CENTER_CENTER,
                &shown,
                font.clone(),
                Color32::from_black_alpha(190),
            );
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                &shown,
                font.clone(),
                Color32::from_rgb(240, 243, 250),
            );
            placed.push(r);
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let lang = self.language;
        let ctx = ui.ctx().clone();
        let size = egui::vec2(480.0, 340.0);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(rect.center(), size))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        if let Some(j) = &self.job {
            let (label, p) = (j.label.clone(), j.progress.clone());
            let el = self.job_started.elapsed().as_secs_f32();
            let (files, dirs, bytes) = (
                p.files.load(Relaxed),
                p.dirs.load(Relaxed),
                p.bytes.load(Relaxed),
            );
            let mut cancel = false;
            card().show(&mut child, |ui| {
                ui.set_width(size.x - 42.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(lang.text("Scanning"))
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.label(egui::RichText::new(&label).strong().size(16.0));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(fmt::size(bytes))
                            .size(40.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(4.0);
                    // indeterminate sweep
                    let (r, _) = ui
                        .allocate_exact_size(egui::vec2(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, HAIR);
                    let w = r.width() * 0.28;
                    let t = (el * 0.9).fract();
                    let x = r.left() + (r.width() + w) * t - w;
                    let seg = Rect::from_min_max(
                        egui::pos2(x.max(r.left()), r.top()),
                        egui::pos2((x + w).min(r.right()), r.bottom()),
                    );
                    if seg.width() > 0.0 {
                        ui.painter().rect_filled(seg, 3.0, ACCENT);
                    }
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(localized_format!(
                            lang,
                            "{} files · {} folders · {:.0}s",
                            "{} arquivos · {} pastas · {:.0}s",
                            group(files),
                            group(dirs),
                            el
                        ))
                        .color(MUTED),
                    );
                    ui.add_space(12.0);
                    if ui.button(lang.text("Cancel")).clicked() {
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
                ui.label(egui::RichText::new("Space Hunter").size(32.0).strong());
                ui.label(
                    egui::RichText::new(lang.text("See where your disk space went.")).color(MUTED),
                );
                ui.add_space(16.0);
                #[cfg(not(target_arch = "wasm32"))]
                let label = lang.text("Choose a folder or drive…");
                #[cfg(target_arch = "wasm32")]
                let label = lang.text("Choose a folder…");
                if ui
                    .add_sized([ui.available_width(), 42.0], primary_button(label))
                    .clicked()
                {
                    open = true;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(lang.text("QUICK SCAN"))
                            .size(10.5)
                            .color(MUTED),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (label, p) in platform::roots().into_iter().take(12) {
                            if ui.button(label).clicked() {
                                start = Some(p);
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                if ui
                    .add(tool_button(lang.text("Try with demo data")))
                    .clicked()
                {
                    demo = true;
                }
                #[cfg(target_arch = "wasm32")]
                ui.label(
                    egui::RichText::new(
                        lang.text(
                            "…or drop a folder onto this page.\nFiles never leave your device.",
                        ),
                    )
                    .color(MUTED)
                    .small(),
                );
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
        let lang = self.language;
        ui.horizontal(|ui| {
            if let Some(t) = self.tree() {
                if let Some(h) = self.hover {
                    let c = self.cells[h];
                    match c.kind {
                        CellKind::Free => ui.label(localized_format!(
                            lang,
                            "Free space: {}",
                            "Espaço livre: {}",
                            fmt::size(c.size)
                        )),
                        CellKind::Lump => ui.label(localized_format!(
                            lang,
                            "{} small items: {}",
                            "{} itens pequenos: {}",
                            c.count,
                            fmt::size(c.size)
                        )),
                        _ => ui.label(format!(
                            "{}   {}",
                            t.path(c.node, std::path::MAIN_SEPARATOR),
                            fmt::size(c.size)
                        )),
                    };
                } else {
                    ui.label(localized_format!(
                        lang,
                        "{} files · {} total",
                        "{} arquivos · {} no total",
                        group(t.node(self.view_root).files as u64),
                        fmt::size(t.node(self.view_root).size)
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((total, free)) = self.disk {
                        ui.label(localized_format!(
                            lang,
                            "Disk: {} free of {} ({:.1}%)",
                            "Disco: {} livres de {} ({:.1}%)",
                            fmt::size(free),
                            fmt::size(total),
                            free as f64 / total.max(1) as f64 * 100.0
                        ));
                        ui.label(egui::RichText::new("·").color(MUTED));
                    }
                    ui.label(
                        egui::RichText::new(
                            lang.text("Click for details · Double-click to explore"),
                        )
                        .color(MUTED),
                    );
                });
            } else {
                ui.label(lang.text("Ready."));
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
pub const ACCENT: Color32 = Color32::from_rgb(171, 137, 239);
pub const DANGER: Color32 = Color32::from_rgb(240, 100, 100);
const HAIR_STRONG: Color32 = Color32::from_rgb(70, 78, 98);
const ACCENT_DIM: Color32 = Color32::from_rgb(64, 43, 100);

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
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
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
    st.text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(20.0));
    st.text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.5));
    st.text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.5));
    st.text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(11.5));
    ctx.set_global_style(st);
}

fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.to_owned())
            .strong()
            .color(Color32::WHITE),
    )
    .fill(ACCENT)
    .stroke(Stroke::NONE)
}

fn tool_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(text.to_owned())
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, HAIR))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(20))
}

fn pill(ui: &mut egui::Ui, label: &str, sel: bool) -> egui::Response {
    let b = egui::Button::new(egui::RichText::new(label.to_owned()).color(if sel {
        Color32::WHITE
    } else {
        MUTED
    }))
    .fill(if sel {
        ACCENT_DIM
    } else {
        Color32::TRANSPARENT
    })
    .stroke(if sel {
        Stroke::new(1.0, ACCENT)
    } else {
        Stroke::NONE
    })
    .min_size(egui::vec2(40.0, 26.0));
    ui.add(b)
}

/// Small uppercase heading for a settings group.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title).size(11.0).color(ACCENT).strong());
}

/// Title (and optional hint) above a control that gets its own line.
fn field(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(2.0);
    ui.label(title);
    if !hint.is_empty() {
        ui.add_space(-6.0);
        ui.label(egui::RichText::new(hint).small().color(MUTED));
    }
}

/// "Label ........ [switch]" row. Returns true when toggled.
fn switch_row(ui: &mut egui::Ui, title: &str, hint: Option<&str>, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on).changed();
        });
    });
    if let Some(h) = hint {
        ui.add_space(-6.0);
        ui.label(egui::RichText::new(h).small().color(MUTED));
    }
    changed
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// iOS-style on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let dim = if ui.is_enabled() { 1.0 } else { 0.4 };
    let track = mix(HAIR_STRONG, ACCENT, t).gamma_multiply(dim);
    let p = ui.painter();
    p.rect_filled(rect, 11.0, track);
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    p.circle_filled(
        egui::pos2(x, rect.center().y),
        8.0,
        Color32::WHITE.gamma_multiply(dim),
    );
    resp
}

/// Joined button group; always drawn left-to-right, whatever the parent layout.
fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = FontId::proportional(13.0);
    let (pad, h) = (12.0, 28.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), TEXT)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad * 2.0).collect();
    let total = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, h), Sense::hover());
    let id = ui.id().with(("seg", rect.min.x as i32, rect.min.y as i32));
    ui.painter().rect(
        rect,
        8.0,
        BG,
        Stroke::new(1.0, HAIR),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, (opt, _)) in options.iter().enumerate() {
        let r = Rect::from_min_size(
            egui::pos2(x, rect.top() + 2.0),
            egui::vec2(widths[i], h - 4.0),
        );
        x += widths[i];
        let resp = ui.interact(r, id.with(i), Sense::click());
        let sel = *value == *opt;
        if sel {
            ui.painter().rect_filled(r, 6.0, ACCENT_DIM);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 6.0, CARD);
        }
        let col = if sel {
            Color32::WHITE
        } else if resp.hovered() {
            TEXT
        } else {
            MUTED
        };
        let g = ui
            .painter()
            .layout_no_wrap(options[i].1.to_string(), font.clone(), col);
        ui.painter().galley(r.center() - g.size() / 2.0, g, col);
        if resp.clicked() && !sel {
            *value = *opt;
            changed = true;
        }
    }
    changed
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
        let canvas = self.cells_key.map_or(fake_canvas, |k| {
            Rect::from_min_size(Pos2::ZERO, egui::vec2(k.w as f32, k.h as f32))
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_setup = false;
        }
        if open {
            self.open_dialog(&ctx);
        }
        if rescan && self.tree.is_some() {
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

        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, HAIR)),
            )
            .show(ui, |ui| {
                self.toolbar(ui);
            });
        if self.tree.is_some() {
            egui::Panel::top("crumbs")
                .frame(
                    egui::Frame::new()
                        .fill(BG)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .stroke(Stroke::new(1.0, HAIR)),
                )
                .show(ui, |ui| {
                    self.breadcrumb(ui, canvas);
                });
        }
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 5))
                    .stroke(Stroke::new(1.0, HAIR)),
            )
            .show(ui, |ui| {
                self.status_bar(ui);
            });
        if self.show_setup {
            egui::Panel::right("settings")
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::same(18))
                        .stroke(Stroke::new(1.0, HAIR)),
                )
                .resizable(false)
                .default_size(330.0)
                .show(ui, |ui| {
                    self.settings_panel(ui);
                });
        } else if self.show_side && self.tree.is_some() {
            egui::Panel::right("side")
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::same(14))
                        .stroke(Stroke::new(1.0, HAIR)),
                )
                .default_size(330.0)
                .size_range(240.0..=560.0)
                .show(ui, |ui| {
                    self.side_panel(ui, canvas);
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                self.canvas(ui);
            });

        self.delete_dialog(&ctx);

        if let Some((msg, t0)) = &self.toast {
            if t0.elapsed().as_secs_f32() < 4.0 {
                egui::Area::new(egui::Id::new("toast"))
                    .anchor(Align2::CENTER_BOTTOM, [0.0, -40.0])
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(CARD)
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(16, 10))
                            .show(ui, |ui| ui.label(msg));
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
        let Some(path) = self.args.screenshot.clone() else {
            return;
        };
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
                    if let Some(img) =
                        image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, buf)
                    {
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
