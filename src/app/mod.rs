//! The viewer window.

mod commands;
mod draw;
mod hud;
mod layer;
mod sheet_panel;
mod view;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Key, Modifiers, Pos2, Sense, Vec2};
use serde::{Deserialize, Serialize};

use crate::analysis::{DiffMetrics, SheetGrid, Stats};
use crate::decode::Decoded;
use crate::ipc::{self, Incoming, Response};
use crate::loader::Loader;
use layer::Layer;
use view::View;

pub const CANVAS: Color32 = Color32::from_rgb(0x13, 0x13, 0x15);
pub const ACCENT: Color32 = Color32::from_rgb(0xf5, 0xb8, 0x4a);
pub const SHEET: Color32 = Color32::from_rgb(0x6e, 0xa8, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xe9, 0xec);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8d, 0x8d, 0x95);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Bg {
    DarkChecker,
    LightChecker,
    Black,
    Gray,
    White,
    Magenta,
    Custom([u8; 3]),
}

impl Bg {
    pub const CYCLE: [Bg; 6] = [Bg::DarkChecker, Bg::LightChecker, Bg::Black, Bg::Gray, Bg::White, Bg::Magenta];
    pub fn name(self) -> String {
        match self {
            Bg::DarkChecker => "checker".into(),
            Bg::LightChecker => "light-checker".into(),
            Bg::Black => "black".into(),
            Bg::Gray => "gray".into(),
            Bg::White => "white".into(),
            Bg::Magenta => "magenta".into(),
            Bg::Custom(c) => format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
        }
    }
    pub fn is_light(self) -> bool {
        match self {
            Bg::LightChecker | Bg::White | Bg::Magenta => true,
            Bg::Custom(c) => (c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000 > 140,
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum HudMode {
    Auto,
    Always,
    Hidden,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub bg: Bg,
    pub smooth: bool,
    pub grid: bool,
    pub hud: HudMode,
    pub info: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { bg: Bg::DarkChecker, smooth: false, grid: true, hud: HudMode::Auto, info: false }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CmpMode {
    Diff,
    Side,
    Onion,
    Swipe,
}

impl CmpMode {
    pub const ALL: [CmpMode; 4] = [CmpMode::Diff, CmpMode::Side, CmpMode::Onion, CmpMode::Swipe];
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "diff" => CmpMode::Diff,
            "side" | "side-by-side" => CmpMode::Side,
            "onion" | "onion-skin" | "blend" => CmpMode::Onion,
            "swipe" | "split" => CmpMode::Swipe,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            CmpMode::Diff => "Diff",
            CmpMode::Side => "Side by side",
            CmpMode::Onion => "Onion skin",
            CmpMode::Swipe => "Swipe",
        }
    }
}

/// The image being shown.
pub struct Doc {
    pub dec: Arc<Decoded>,
    pub layer: Layer,
    pub exposure: f32,
    pub stats: Arc<Mutex<Option<Stats>>>,
    pub stats_started: bool,
}

pub struct Compare {
    pub a: Arc<Decoded>,
    pub b: Arc<Decoded>,
    pub la: Layer,
    pub lb: Layer,
    pub ldiff: Layer,
    pub metrics: DiffMetrics,
    pub mode: CmpMode,
    pub split: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Sheet {
    pub grid: SheetGrid,
    /// Cell range [from, to] to preview as an animation.
    pub play: Option<[u32; 2]>,
    pub fps: f32,
}

#[derive(Default)]
pub struct Anim {
    pub frame: usize,
    pub playing: bool,
    pub accum_ms: f64,
    pub speed: f32,
    /// Loop only inside this frame range (an Aseprite tag).
    pub range: Option<(usize, usize)>,
    pub sheet_cell: u32,
    pub sheet_accum: f64,
}

struct SnapReq {
    since: f64,
    path: PathBuf,
    clean: bool,
    reply: mpsc::Sender<Response>,
}

#[derive(Default)]
struct Palette {
    open: bool,
    text: String,
    selected: usize,
    just_opened: bool,
}

pub struct GlintApp {
    ipc_rx: Receiver<Incoming>,
    loader: Loader,
    settings: Settings,

    files: Vec<PathBuf>,
    index: usize,
    current: Option<PathBuf>,
    list_dirty: bool,

    doc: Option<Doc>,
    error: Option<(PathBuf, String)>,
    view: View,
    anim: Anim,
    compare: Option<Compare>,
    pending_compare: Option<(PathBuf, PathBuf, CmpMode)>,
    compare_inbox: Vec<(PathBuf, f64)>,
    sheet: Option<Sheet>,
    sheet_ui: sheet_panel::SheetUi,

    palette: Palette,
    help: bool,
    confirm_delete: Option<PathBuf>,
    toast: Option<(String, f64)>,
    last_activity: f64,
    last_time: f64,
    hover_px: Option<Vec2>,
    snaps: Vec<SnapReq>,
    title: String,
    checker: Option<egui::TextureHandle>,

    _watcher: Option<notify::RecommendedWatcher>,
    watch_rx: Receiver<PathBuf>,
    watch_tx: mpsc::Sender<PathBuf>,
    watched_dir: Option<PathBuf>,
    reload_at: Option<f64>,

    startup: Vec<Vec<String>>,
    quit: bool,
    view_animating: bool,
    snap_armed: bool,
    /// Set while a clean capture is in flight; eframe grabs the frame *after* the request.
    snap_hide: Arc<std::sync::atomic::AtomicBool>,
    ctx: egui::Context,
}

const SETTINGS_KEY: &str = "glint-settings";

/// Start the GUI. `startup` holds commands (argv form) to run on the first frame.
pub fn run(server: ipc::Server, startup: Vec<Vec<String>>) -> eframe::Result {
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
    let ipc_rx = server.spawn(ctx_cell.clone());

    let icon = image::load_from_memory(include_bytes!("../../assets/icon-256.png"))
        .map(|i| {
            let i = i.to_rgba8();
            egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }
        })
        .ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Glint")
        .with_app_id("glint")
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([360.0, 240.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "Glint",
        options,
        Box::new(move |cc| {
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            crate::win::style_titlebar(cc, [CANVAS.r(), CANVAS.g(), CANVAS.b()]);
            setup_style(&cc.egui_ctx);
            let settings = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
            Ok(Box::new(GlintApp::new(cc.egui_ctx.clone(), ipc_rx, settings, startup)))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let try_font = |file: &str| std::fs::read(Path::new(&windir).join("Fonts").join(file)).ok();
    if let Some(data) = try_font("SegUIVar.ttf").or_else(|| try_font("segoeui.ttf")) {
        fonts.font_data.insert("segoe".into(), Arc::new(egui::FontData::from_owned(data)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    if let Some(data) = try_font("CascadiaMono.ttf").or_else(|| try_font("consola.ttf")) {
        fonts.font_data.insert("mono".into(), Arc::new(egui::FontData::from_owned(data)));
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "mono".into());
    }
    if let Some(data) = try_font("seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), Arc::new(egui::FontData::from_owned(data)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push("symbols".into());
        }
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = CANVAS;
    visuals.window_fill = Color32::from_rgb(0x1c, 0x1c, 0x20);
    visuals.window_stroke = egui::Stroke::new(1.0, Color32::from_white_alpha(14));
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.hyperlink_color = ACCENT;
    visuals.override_text_color = Some(TEXT);
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(6);
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::from_white_alpha(8);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_white_alpha(20);
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
    });
}

impl GlintApp {
    fn new(ctx: egui::Context, ipc_rx: Receiver<Incoming>, settings: Settings, startup: Vec<Vec<String>>) -> Self {
        let (watch_tx, watch_rx) = mpsc::channel();
        Self {
            ctx: ctx.clone(),
            ipc_rx,
            loader: Loader::new(ctx),
            settings,
            files: Vec::new(),
            index: 0,
            current: None,
            list_dirty: false,
            doc: None,
            error: None,
            view: View::default(),
            anim: Anim { speed: 1.0, ..Default::default() },
            compare: None,
            pending_compare: None,
            compare_inbox: Vec::new(),
            sheet: None,
            sheet_ui: Default::default(),
            palette: Palette::default(),
            help: false,
            confirm_delete: None,
            toast: None,
            last_activity: 0.0,
            last_time: 0.0,
            hover_px: None,
            snaps: Vec::new(),
            title: String::new(),
            checker: None,
            _watcher: None,
            watch_rx,
            watch_tx,
            watched_dir: None,
            reload_at: None,
            startup,
            quit: false,
            view_animating: false,
            snap_armed: false,
            snap_hide: Default::default(),
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.last_time));
    }

    // ---------------------------------------------------------------- files

    pub fn open(&mut self, path: &Path) {
        let path = absolute(path);
        if self.list_dirty || !self.files.iter().any(|p| p == &path) {
            self.files = crate::folder::siblings(&path);
            self.list_dirty = false;
        }
        self.index = self.files.iter().position(|p| p == &path).unwrap_or(0);
        self.compare = None;
        self.pending_compare = None;
        self.show(path);
    }

    /// Open an explicit list of files as the browsing set.
    pub fn open_list(&mut self, paths: Vec<PathBuf>) {
        let paths: Vec<PathBuf> = paths.iter().map(|p| absolute(p)).collect();
        if paths.len() == 1 {
            return self.open(&paths[0]);
        }
        let Some(first) = paths.first().cloned() else { return };
        self.files = paths;
        self.index = 0;
        self.compare = None;
        self.show(first);
    }

    fn show(&mut self, path: PathBuf) {
        self.watch_dir(&path);
        self.current = Some(path.clone());
        match self.loader.get(&path) {
            Some(res) => self.accept(res, &path),
            None => self.loader.request(&path, true),
        }
        self.prefetch();
    }

    fn prefetch(&mut self) {
        let n = self.files.len();
        if n < 2 {
            return;
        }
        let near: Vec<PathBuf> = [1isize, -1, 2]
            .iter()
            .map(|d| self.files[(self.index as isize + d).rem_euclid(n as isize) as usize].clone())
            .collect();
        let current = self.current.clone();
        self.loader.retain_queued(|p| near.iter().any(|q| q == p) || Some(p) == current.as_deref());
        for p in &near {
            self.loader.request(p, false);
        }
    }

    fn accept(&mut self, res: crate::loader::LoadResult, path: &Path) {
        match res {
            Ok(dec) => {
                let same_dims =
                    self.doc.as_ref().is_some_and(|d| d.dec.width() == dec.width() && d.dec.height() == dec.height());
                let reload = self.doc.as_ref().is_some_and(|d| d.dec.path == dec.path);
                let animated = dec.is_animated();
                let keep_frame = reload && self.anim.frame < dec.frames.len();
                self.doc = Some(Doc {
                    layer: Layer::new(dec.frames.iter().map(|f| f.image.clone()).collect()),
                    dec,
                    exposure: 0.0,
                    stats: Arc::new(Mutex::new(None)),
                    stats_started: false,
                });
                self.error = None;
                if !keep_frame {
                    self.anim.frame = 0;
                    self.anim.range = None;
                    self.anim.playing = animated;
                    self.anim.accum_ms = 0.0;
                }
                // Same-size images (screenshot series, sprite variants) keep the zoom and pan.
                if !same_dims {
                    self.view.fit = true;
                    self.view.snap_next = true;
                    // A pixel grid only fits images of the same layout; with the panel open,
                    // re-apply its settings (a columns×rows grid adapts to the new size).
                    if self.sheet_ui.open {
                        self.apply_sheet_ui();
                    } else {
                        self.sheet = None;
                    }
                }
            }
            Err(e) => {
                self.doc = None;
                self.error = Some((path.to_path_buf(), e));
            }
        }
    }

    pub fn step(&mut self, delta: isize) {
        if self.list_dirty {
            if let Some(cur) = self.current.clone() {
                self.files = crate::folder::siblings(&cur);
                self.index = self.files.iter().position(|p| p == &cur).unwrap_or(0);
            }
            self.list_dirty = false;
        }
        let n = self.files.len();
        if n == 0 || self.compare.is_some() {
            return;
        }
        self.index = (self.index as isize + delta).rem_euclid(n as isize) as usize;
        let p = self.files[self.index].clone();
        self.show(p);
    }

    pub fn goto_index(&mut self, i: usize) {
        if i < self.files.len() {
            self.index = i;
            let p = self.files[i].clone();
            self.show(p);
        }
    }

    pub fn reload(&mut self) {
        if let Some(cur) = self.current.clone() {
            self.loader.invalidate(&cur);
            self.loader.request(&cur, true);
        }
        if let Some(c) = &self.compare {
            let (a, b, m) = (c.a.path.clone(), c.b.path.clone(), c.mode);
            self.loader.invalidate(&a);
            self.loader.invalidate(&b);
            self.start_compare(a, b, m);
        }
    }

    fn watch_dir(&mut self, path: &Path) {
        let Some(dir) = path.parent().map(Path::to_path_buf) else { return };
        if self.watched_dir.as_ref() == Some(&dir) {
            return;
        }
        use notify::Watcher;
        let tx = self.watch_tx.clone();
        let ctx = self.ctx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    let _ = tx.send(p);
                }
                ctx.request_repaint();
            }
        });
        if let Ok(mut w) = watcher {
            if w.watch(&dir, notify::RecursiveMode::NonRecursive).is_ok() {
                self._watcher = Some(w);
                self.watched_dir = Some(dir);
            }
        }
    }

    fn poll_watcher(&mut self, now: f64) {
        while let Ok(p) = self.watch_rx.try_recv() {
            let is_current = self.current.as_deref() == Some(p.as_path());
            let in_compare = self.compare.as_ref().is_some_and(|c| c.a.path == p || c.b.path == p);
            if is_current || in_compare {
                // Editors write files in several steps; wait until they settle.
                self.reload_at = Some(now + 0.15);
            } else {
                self.loader.invalidate(&p);
                self.list_dirty = true;
            }
        }
        match self.reload_at {
            Some(t) if now >= t => {
                self.reload_at = None;
                self.reload();
            }
            Some(t) => self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(t - now)),
            None => {}
        }
    }

    // -------------------------------------------------------------- compare

    pub fn start_compare(&mut self, a: PathBuf, b: PathBuf, mode: CmpMode) {
        let (a, b) = (absolute(&a), absolute(&b));
        self.loader.request(&a, true);
        self.loader.request(&b, true);
        self.watch_dir(&a);
        self.pending_compare = Some((a, b, mode));
    }

    fn poll_compare(&mut self) {
        let Some((a, b, mode)) = self.pending_compare.clone() else { return };
        let (Some(ra), Some(rb)) = (self.loader.get(&a), self.loader.get(&b)) else {
            // Re-request in case a cached entry went stale while waiting.
            if !self.loader.is_loading(&a) {
                self.loader.request(&a, true);
            }
            if !self.loader.is_loading(&b) {
                self.loader.request(&b, true);
            }
            return;
        };
        self.pending_compare = None;
        match (ra, rb) {
            (Ok(da), Ok(db)) => {
                let (ia, ib) = (da.frames[0].image.clone(), db.frames[0].image.clone());
                let metrics = crate::analysis::diff(&ia, &ib);
                let diff = Arc::new(crate::analysis::diff_image(&ia, &ib));
                let keep = self.compare.as_ref().map(|c| c.split).unwrap_or(0.5);
                let same = self
                    .compare
                    .as_ref()
                    .is_some_and(|c| c.a.width() == da.width() && c.a.height() == da.height());
                self.compare = Some(Compare {
                    la: Layer::new(vec![ia]),
                    lb: Layer::new(vec![ib]),
                    ldiff: Layer::new(vec![diff]),
                    a: da,
                    b: db,
                    metrics,
                    mode,
                    split: keep,
                });
                self.error = None;
                if !same {
                    self.view.fit = true;
                    self.view.snap_next = true;
                }
            }
            (Err(e), _) => self.error = Some((a, e)),
            (_, Err(e)) => self.error = Some((b, e)),
        }
    }

    /// Explorer launches one process per selected file for "Compare with Glint";
    /// pair up arrivals that land within a couple of seconds of each other.
    fn compare_add(&mut self, p: PathBuf, now: f64) {
        self.compare_inbox.retain(|(_, t)| now - *t < 3.0);
        self.compare_inbox.push((absolute(&p), now));
        if self.compare_inbox.len() >= 2 {
            let b = self.compare_inbox.pop().unwrap().0;
            let a = self.compare_inbox.pop().unwrap().0;
            self.compare_inbox.clear();
            self.start_compare(a, b, CmpMode::Swipe);
        } else {
            self.toast("Select a second image to compare");
        }
    }

    // ------------------------------------------------------------- per frame

    fn poll(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = (now - self.last_time).clamp(0.0, 0.25);
        self.last_time = now;

        for p in self.loader.poll() {
            if self.current.as_deref() == Some(p.as_path()) && self.pending_compare.is_none() {
                if let Some(res) = self.loader.get(&p) {
                    self.accept(res, &p);
                }
            }
        }
        self.poll_compare();
        self.poll_watcher(now);
        self.tick_animation(ctx, dt);

        if let Some((_, t)) = &self.toast {
            if now - t > 1.8 {
                self.toast = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    fn tick_animation(&mut self, ctx: &egui::Context, dt: f64) {
        if let Some(doc) = &self.doc {
            let n = doc.dec.frames.len();
            if self.anim.playing && n > 1 && self.compare.is_none() {
                let (lo, hi) = self.anim.range.unwrap_or((0, n - 1));
                self.anim.accum_ms += dt * 1000.0 * self.anim.speed as f64;
                let mut guard = 0;
                loop {
                    let d = doc.dec.frames[self.anim.frame.min(n - 1)].delay_ms.max(10) as f64;
                    if self.anim.accum_ms < d || guard > n {
                        let left = (d - self.anim.accum_ms) / self.anim.speed.max(0.01) as f64;
                        ctx.request_repaint_after(std::time::Duration::from_millis(left.max(1.0) as u64));
                        break;
                    }
                    self.anim.accum_ms -= d;
                    self.anim.frame = if self.anim.frame >= hi || self.anim.frame < lo { lo } else { self.anim.frame + 1 };
                    guard += 1;
                }
            }
        }
        if let (Some(sheet), Some(doc)) = (&self.sheet, &self.doc) {
            if let Some([a, b]) = sheet.play {
                let cells = sheet.grid.columns(doc.dec.width()) * sheet.grid.rows(doc.dec.height());
                let (a, b) = (a.min(cells.saturating_sub(1)), b.min(cells.saturating_sub(1)));
                let step = 1000.0 / sheet.fps.max(0.1) as f64;
                self.anim.sheet_accum += dt * 1000.0;
                while self.anim.sheet_accum >= step {
                    self.anim.sheet_accum -= step;
                    self.anim.sheet_cell =
                        if self.anim.sheet_cell >= b || self.anim.sheet_cell < a { a } else { self.anim.sheet_cell + 1 };
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(((step - self.anim.sheet_accum).max(1.0)) as u64));
            }
        }
    }

    fn handle_ipc(&mut self, ctx: &egui::Context) {
        while let Ok(inc) = self.ipc_rx.try_recv() {
            let argv = inc.req.argv.clone();
            let cwd = inc.req.cwd.clone();
            if argv.first().map(String::as_str) == Some("snap") {
                match commands::parse_snap(&argv[1..], cwd.as_deref()) {
                    Ok((path, clean)) => {
                        if ctx.input(|i| i.viewport().minimized) == Some(true) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        }
                        self.snaps.push(SnapReq { since: self.last_time, path, clean, reply: inc.reply });
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        let _ = inc.reply.send(Response::err(e));
                    }
                }
                continue;
            }
            let raise = matches!(argv.first().map(String::as_str), Some("open" | "compare" | "compare-add" | "focus"));
            let res = self.exec(ctx, &argv, cwd.as_deref());
            // Scripted changes land instantly so a following `snap` sees the final view.
            self.view.snap_next = true;
            if raise && res.is_ok() {
                if ctx.input(|i| i.viewport().minimized) == Some(true) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            let _ = inc.reply.send(res.into());
        }
    }

    // ------------------------------------------------------------- keyboard

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.palette.open || self.help || self.confirm_delete.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut cmds: Vec<&str> = Vec::new();
        let mut steps = 0isize;
        ctx.input_mut(|i| {
            let ctrl = Modifiers::CTRL;
            let shift = Modifiers::SHIFT;
            if i.consume_key(ctrl, Key::P) || i.consume_key(ctrl, Key::K) || i.consume_key(Modifiers::NONE, Key::Colon) {
                cmds.push("palette");
            }
            if i.consume_key(ctrl, Key::C) || i.consume_key(Modifiers::NONE, Key::C) {
                cmds.push("copy image");
            }
            if i.consume_key(shift, Key::C) {
                cmds.push("copy hex");
            }
            if i.consume_key(ctrl | shift, Key::C) {
                cmds.push("copy path");
            }
            if i.consume_key(ctrl, Key::R) || i.consume_key(Modifiers::NONE, Key::F5) {
                cmds.push("reload");
            }
            if i.consume_key(shift, Key::E) {
                cmds.push("exposure -0.5");
            }
            if i.consume_key(Modifiers::NONE, Key::E) {
                cmds.push("exposure +0.5");
            }
            if i.consume_key(shift, Key::G) {
                cmds.push("sheet panel");
            }
            for (key, cmd) in [
                (Key::G, "grid toggle"),
                (Key::B, "bg next"),
                (Key::S, "smooth toggle"),
                (Key::H, "hud toggle"),
                (Key::I, "info toggle"),
                (Key::F, "fullscreen toggle"),
                (Key::F11, "fullscreen toggle"),
                (Key::Space, "play toggle"),
                (Key::Comma, "frame prev"),
                (Key::Period, "frame next"),
                (Key::Num0, "zoom fit"),
                (Key::Plus, "zoom in"),
                (Key::Equals, "zoom in"),
                (Key::Minus, "zoom out"),
                (Key::M, "mode next"),
                (Key::Tab, "swap"),
                (Key::F1, "help"),
                (Key::Questionmark, "help"),
                (Key::Delete, "delete"),
                (Key::Escape, "escape"),
            ] {
                if i.consume_key(Modifiers::NONE, key) {
                    cmds.push(cmd);
                }
            }
            for (k, n) in [
                (Key::Num1, "zoom 100"),
                (Key::Num2, "zoom 200"),
                (Key::Num3, "zoom 300"),
                (Key::Num4, "zoom 400"),
                (Key::Num5, "zoom 500"),
                (Key::Num6, "zoom 600"),
                (Key::Num7, "zoom 700"),
                (Key::Num8, "zoom 800"),
                (Key::Num9, "zoom 900"),
            ] {
                if i.consume_key(Modifiers::NONE, k) {
                    cmds.push(n);
                }
            }
            for k in [Key::ArrowRight, Key::PageDown, Key::ArrowDown] {
                if i.consume_key(Modifiers::NONE, k) {
                    steps += 1;
                }
            }
            for k in [Key::ArrowLeft, Key::PageUp, Key::ArrowUp] {
                if i.consume_key(Modifiers::NONE, k) {
                    steps -= 1;
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                cmds.push("first");
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                cmds.push("last");
            }
            if i.pointer.button_pressed(egui::PointerButton::Extra1) {
                steps -= 1;
            }
            if i.pointer.button_pressed(egui::PointerButton::Extra2) {
                steps += 1;
            }
        });
        if steps != 0 {
            self.step(steps);
        }
        for c in cmds {
            let argv: Vec<String> = c.split_whitespace().map(String::from).collect();
            if let Err(e) = self.exec(ctx, &argv, None) {
                self.toast(e);
            }
        }
    }

    // ------------------------------------------------------------- snapshot

    fn finish_snaps(&mut self, ctx: &egui::Context) {
        if self.snaps.is_empty() {
            return;
        }
        // Wait for pending decodes and zoom animations so the capture shows the settled view.
        let waited = self.snaps.iter().map(|s| self.last_time - s.since).fold(0.0, f64::max);
        if (self.is_busy() || self.view_animating) && waited < 8.0 {
            self.snap_armed = false;
            ctx.request_repaint();
            return;
        }
        // Capture on the frame *after* a settled, overlay-free frame, so nothing
        // from the previous frame (fading HUD, zoom step) leaks into the image.
        if !self.snap_armed {
            self.snap_armed = true;
            ctx.request_repaint();
            return;
        }
        self.snap_armed = false;
        let snaps = std::mem::take(&mut self.snaps);
        let ctx2 = ctx.clone();
        let hide = self.snap_hide.clone();
        hide.store(snaps.iter().any(|s| s.clean), std::sync::atomic::Ordering::SeqCst);
        ctx.request_screenshot(move |img: Arc<egui::ColorImage>| {
            hide.store(false, std::sync::atomic::Ordering::SeqCst);
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
            let res = image::RgbaImage::from_raw(w as u32, h as u32, raw).ok_or_else(|| "bad screenshot buffer".to_string());
            for s in snaps {
                let r = res.as_ref().map_err(Clone::clone).and_then(|buf| {
                    if let Some(dir) = s.path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    buf.save(&s.path).map_err(|e| format!("couldn't write {}: {e}", s.path.display()))?;
                    Ok(serde_json::json!({ "path": s.path, "width": w, "height": h }))
                });
                let _ = s.reply.send(r.into());
            }
            ctx2.request_repaint();
        });
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let t = if let Some(c) = &self.compare {
            format!("{}  ⟷  {}  —  Glint", name(&c.a.path), name(&c.b.path))
        } else if let Some(p) = &self.current {
            format!("{}  —  Glint", name(p))
        } else {
            "Glint".into()
        };
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        match dropped.len() {
            0 => {}
            2 => self.start_compare(dropped[0].clone(), dropped[1].clone(), CmpMode::Swipe),
            _ => self.open_list(dropped),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.pending_compare.is_some()
            || self.current.as_ref().is_some_and(|p| {
                self.loader.is_loading(p) || (self.compare.is_none() && self.doc.as_ref().is_none_or(|d| &d.dec.path != p) && self.error.is_none())
            })
    }

    /// Size of the content being shown, in image pixels.
    pub fn content_size(&self) -> Option<Vec2> {
        if let Some(c) = &self.compare {
            let (w, h) = (c.a.width().max(c.b.width()), c.a.height().max(c.b.height()));
            return Some(egui::vec2(w as f32, h as f32));
        }
        self.doc.as_ref().map(|d| {
            let img = d.layer.frame(self.anim.frame);
            egui::vec2(img.width() as f32, img.height() as f32)
        })
    }
}

impl eframe::App for GlintApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.startup.is_empty() {
            for argv in std::mem::take(&mut self.startup) {
                if let Err(e) = self.exec(ctx, &argv, None) {
                    self.toast(e);
                }
            }
        }
        self.handle_ipc(ctx);
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.handle_drops(&ctx);

        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, CANVAS);
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());

        if ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_down()) {
            self.last_activity = self.last_time;
        }

        let clean = self.snaps.iter().any(|s| s.clean) || self.snap_hide.load(std::sync::atomic::Ordering::SeqCst);
        self.handle_keys(&ctx);
        self.handle_canvas_input(&ctx, &resp, rect);
        self.draw_canvas(ui, rect);
        // The error card is content (what the user sees instead of an image), so snaps keep it.
        self.draw_error(&ctx, rect);
        if !clean {
            self.draw_overlays(&ctx, rect);
        }
        self.context_menu(&resp);
        self.palette_ui(&ctx);
        self.modals(&ctx);
        self.update_title(&ctx);
        self.finish_snaps(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        CANVAS.to_normalized_gamma_f32()
    }
}

pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Pos2 helper shared by submodules.
pub fn p2(v: Vec2) -> Pos2 {
    Pos2::new(v.x, v.y)
}
