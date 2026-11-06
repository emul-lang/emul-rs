//! Preview mode: renders the project in memory, shows it in a native egui window,
//! serves the HTML view on a local web server, and rebuilds whenever a source file,
//! image or the config is saved. Nothing is written to the output directory.

use crate::build::{self, BuildResult, Overrides, Rendered};
use crate::config::{Format, SvgFonts};
use crate::diag::{display_path, Diagnostic, Diags, Severity};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct PreviewOptions {
    pub config: Option<PathBuf>,
    pub overrides: Overrides,
    pub port: u16,
    pub open_html: bool,
}

/// How many ports to try, starting with the requested one.
const PORT_TRIES: u16 = 20;
/// How long to wait for more file events before rebuilding.
const DEBOUNCE: Duration = Duration::from_millis(150);

// ---------------------------------------------------------------------------
// Shared state

/// The last build that rendered without errors.
struct Good {
    /// The generation of the build that produced it.
    generation: u64,
    rendered: Vec<Rendered>,
}

/// State shared between the build thread, the web server and the window.
struct State {
    /// Increases every time a build finishes, with or without errors.
    generation: u64,
    building: bool,
    good: Option<Arc<Good>>,
    /// Diagnostics of the latest build.
    diags: Diags,
    /// The latest build's diagnostics rendered as text, with source lines.
    diag_text: String,
    /// The base directory paths are shown relative to.
    base: PathBuf,
    /// Set once the window exists, so the build thread can ask it to redraw.
    ctx: Option<egui::Context>,
}

type Shared = Arc<Mutex<State>>;

fn lock(s: &Shared) -> std::sync::MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Entry point

pub fn run(opts: PreviewOptions) -> Result<(), String> {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let shared: Shared = Arc::new(Mutex::new(State {
        generation: 0,
        building: true,
        good: None,
        diags: Diags::new(),
        diag_text: String::new(),
        base: base.clone(),
        ctx: None,
    }));

    // The window and the browser always need every format, whatever the config says.
    let mut ov = opts.overrides.clone();
    ov.formats = Some(vec![Format::Html, Format::Svg, Format::Png]);
    // The window rasterizes the SVG with resvg, which draws outlined text exactly;
    // embedded web fonts would not be used by it.
    ov.svg_fonts = Some(SvgFonts::Outline);

    // The web server starts first, so the HTML view works even without a display.
    let server = start_server(opts.port, shared.clone());
    let url = match &server {
        Ok(port) => {
            let u = format!("http://127.0.0.1:{port}/");
            eprintln!("preview: HTML view served at {u}");
            Some(u)
        }
        Err(e) => {
            eprintln!("preview: the HTML view is not available: {e}");
            None
        }
    };

    {
        let shared = shared.clone();
        let config = opts.config.clone();
        std::thread::Builder::new()
            .name("emul-preview-build".into())
            .spawn(move || build_loop(config, ov, shared))
            .map_err(|e| format!("can't start the build thread: {e}"))?;
    }

    if opts.open_html {
        if let Some(u) = &url {
            open_in_browser(u);
        }
    }

    match run_window(shared.clone(), url.clone()) {
        Ok(()) => Ok(()),
        Err(e) => match &url {
            Some(u) => {
                eprintln!("preview: the preview window could not be opened ({e}).");
                eprintln!("preview: the HTML view is still served at {u}; press Ctrl-C to stop.");
                loop {
                    std::thread::park();
                }
            }
            None => Err(format!(
                "the preview window could not be opened ({e}), and the HTML view could not be served either; \
                 check that a display is available (DISPLAY or WAYLAND_DISPLAY) or free up a port with --port"
            )),
        },
    }
}

// ---------------------------------------------------------------------------
// Building and watching

struct Watched {
    /// Every file the last build read, normalized.
    inputs: HashSet<PathBuf>,
    /// The config file, normalized, whether or not it exists yet.
    config: PathBuf,
    /// The source root, normalized.
    root: PathBuf,
}

fn build_once(config: &Option<PathBuf>, ov: &Overrides) -> (BuildResult, Watched) {
    let mut diags = Diags::new();
    let mut cfg = build::load_config(config.as_deref(), &mut diags);
    ov.apply(&mut cfg);
    let res = build::render(&cfg, ov, None, diags);
    let config_path = match (config, &cfg.file) {
        (Some(p), _) => absolute(p),
        (None, Some(f)) => absolute(f),
        (None, None) => absolute(&cfg.base.join("emul.config.pi")),
    };
    let watched = Watched {
        inputs: res.inputs.iter().map(|p| norm(p)).collect(),
        config: norm(&config_path),
        root: norm(&cfg.root),
    };
    (res, watched)
}

fn publish(shared: &Shared, res: BuildResult) {
    let ctx;
    {
        let mut st = lock(shared);
        st.generation += 1;
        st.building = false;
        st.diag_text = res.diags.render(&res.sources, &st.base);
        if !res.diags.has_errors() {
            st.good = Some(Arc::new(Good { generation: st.generation, rendered: res.rendered }));
        }
        let (e, w) = (res.diags.error_count(), res.diags.warning_count());
        if !st.diag_text.is_empty() {
            eprint!("{}", st.diag_text);
        }
        eprintln!(
            "preview: build {} finished: {} error{}, {} warning{}",
            st.generation,
            e,
            if e == 1 { "" } else { "s" },
            w,
            if w == 1 { "" } else { "s" }
        );
        st.diags = res.diags;
        ctx = st.ctx.clone();
    }
    if let Some(c) = ctx {
        c.request_repaint();
    }
}

fn build_loop(config: Option<PathBuf>, ov: Overrides, shared: Shared) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("preview: files can't be watched, so the preview won't refresh by itself: {e}");
            None
        }
    };
    let mut watching: HashSet<(PathBuf, bool)> = HashSet::new();

    loop {
        let (res, watched) = build_once(&config, &ov);
        if let Some(w) = watcher.as_mut() {
            update_watches(w, &mut watching, &watched);
        }
        publish(&shared, res);
        if watcher.is_none() {
            return;
        }

        // Wait for a relevant change.
        loop {
            match rx.recv() {
                Ok(Ok(ev)) if relevant(&ev, &watched) => break,
                Ok(_) => {}
                Err(_) => return,
            }
        }
        // Debounce: rebuild once things have been quiet for a moment.
        let mut deadline = Instant::now() + DEBOUNCE;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_timeout(deadline - now) {
                Ok(Ok(ev)) if relevant(&ev, &watched) => deadline = Instant::now() + DEBOUNCE,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        lock(&shared).building = true;
        if let Some(c) = lock(&shared).ctx.clone() {
            c.request_repaint();
        }
    }
}

/// Watches the source root recursively, and the directory of every input outside it
/// and of the config. Directories are watched rather than files, so editors that save
/// by replacing the file are still noticed.
fn update_watches(w: &mut notify::RecommendedWatcher, watching: &mut HashSet<(PathBuf, bool)>, watched: &Watched) {
    use notify::{RecursiveMode, Watcher};
    let mut wanted: HashSet<(PathBuf, bool)> = HashSet::new();
    if watched.root.is_dir() {
        wanted.insert((watched.root.clone(), true));
    }
    let mut add_parent = |p: &Path| {
        if p.starts_with(&watched.root) && watched.root.is_dir() {
            return;
        }
        if let Some(parent) = p.parent() {
            if parent.is_dir() {
                wanted.insert((parent.to_path_buf(), false));
            }
        }
    };
    for p in &watched.inputs {
        add_parent(p);
    }
    add_parent(&watched.config);

    for (p, rec) in watching.difference(&wanted).cloned().collect::<Vec<_>>() {
        let _ = w.unwatch(&p);
        watching.remove(&(p, rec));
    }
    for (p, rec) in wanted {
        if watching.contains(&(p.clone(), rec)) {
            continue;
        }
        let mode = if rec { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
        match w.watch(&p, mode) {
            Ok(()) => {
                watching.insert((p, rec));
            }
            Err(e) => eprintln!("preview: can't watch {}: {e}", p.display()),
        }
    }
}

fn relevant(ev: &notify::Event, watched: &Watched) -> bool {
    use notify::event::{AccessKind, AccessMode, EventKind, MetadataKind, ModifyKind};
    let kind_ok = match ev.kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => false,
        EventKind::Modify(_) => true,
        // Reads by the build itself show up as access events; only finished writes count.
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Any | EventKind::Other => true,
    };
    if !kind_ok {
        return false;
    }
    ev.paths.iter().any(|p| {
        let p = norm(p);
        watched.inputs.contains(&p)
            || p == watched.config
            || (p.starts_with(&watched.root) && p.extension().is_some_and(|e| e == "emul"))
    })
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Canonicalizes a path, or its parent when the file itself doesn't exist (yet or any more).
fn norm(p: &Path) -> PathBuf {
    let p = absolute(p);
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) => dir.canonicalize().map(|d| d.join(name)).unwrap_or(p.clone()),
        _ => p,
    }
}

// ---------------------------------------------------------------------------
// Web server for the HTML view

fn start_server(port: u16, shared: Shared) -> Result<u16, String> {
    let mut last_err = String::new();
    for p in port..=port.saturating_add(PORT_TRIES - 1) {
        match tiny_http::Server::http(("127.0.0.1", p)) {
            Ok(server) => {
                std::thread::Builder::new()
                    .name("emul-preview-http".into())
                    .spawn(move || {
                        for req in server.incoming_requests() {
                            let shared = shared.clone();
                            // Requests are small and quick; serve each on its own thread so a
                            // slow client can't hold up the reload polling.
                            std::thread::spawn(move || handle(req, &shared));
                        }
                    })
                    .map_err(|e| format!("can't start the web server thread: {e}"))?;
                return Ok(p);
            }
            Err(e) => last_err = e.to_string(),

        }
        if p == u16::MAX {
            break;
        }
    }
    Err(format!("no free port from {port} to {} ({last_err})", port.saturating_add(PORT_TRIES - 1)))
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn respond_text(req: tiny_http::Request, status: u16, ctype: &str, body: String) {
    let resp = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn handle(req: tiny_http::Request, shared: &Shared) {
    let raw = req.url().to_string();
    let path = raw.split(['?', '#']).next().unwrap_or("/");
    let path = percent_decode(path);
    let (generation, building, good, diag_text, has_errors) = {
        let st = lock(shared);
        (st.generation, st.building, st.good.clone(), st.diag_text.clone(), st.diags.has_errors())
    };

    if path == "/__emul/version" {
        return respond_text(req, 200, "text/plain; charset=utf-8", generation.to_string());
    }
    if path == "/" || path == "/index.html" {
        let mut body = String::from("<h1>Emul preview</h1>\n");
        match &good {
            Some(g) if !g.rendered.is_empty() => {
                body.push_str("<ul>\n");
                for r in &g.rendered {
                    body.push_str(&format!(
                        "<li><a href=\"/{0}\">{1}</a> <small>{2}×{3}</small></li>\n",
                        url_escape(&r.name),
                        html_escape(&r.name),
                        r.width,
                        r.height
                    ));
                }
                body.push_str("</ul>\n");
            }
            Some(_) => body.push_str("<p>The entry file exports no compositions.</p>\n"),
            None if building => body.push_str("<p>Building…</p>\n"),
            None => body.push_str("<p>Nothing has rendered yet.</p>\n"),
        }
        let page = simple_page("Emul preview", &body);
        let page = inject(&page, generation, has_errors.then_some(diag_text.as_str()));
        return respond_text(req, 200, "text/html; charset=utf-8", page);
    }
    if let Some(rel) = path.strip_prefix('/') {
        if rel.starts_with("assets/") {
            let src = good.as_ref().and_then(|g| {
                g.rendered
                    .iter()
                    .filter_map(|r| r.html.as_ref())
                    .flat_map(|h| h.assets.iter())
                    .find(|(_, name)| name == rel)
                    .map(|(src, _)| src.clone())
            });
            if let Some(src) = src {
                match std::fs::read(&src) {
                    Ok(data) => {
                        let resp = tiny_http::Response::from_data(data)
                            .with_header(header("Content-Type", content_type(&src)))
                            .with_header(header("Cache-Control", "no-store"));
                        let _ = req.respond(resp);
                    }
                    Err(e) => respond_text(req, 404, "text/plain; charset=utf-8", format!("can't read {}: {e}", src.display())),
                }
                return;
            }
            return respond_text(req, 404, "text/plain; charset=utf-8", format!("no asset {rel}"));
        }
        let name = rel.strip_suffix(".html").unwrap_or(rel);
        let found = good.as_ref().and_then(|g| g.rendered.iter().find(|r| r.name == name).and_then(|r| r.html.as_ref()));
        let errors = has_errors.then_some(diag_text.as_str());
        if let Some(h) = found {
            return respond_text(req, 200, "text/html; charset=utf-8", inject(&h.html, generation, errors));
        }
        // Unknown composition, or nothing good yet: a page that waits for the next build.
        let msg = if good.is_none() && building {
            "Building…".to_string()
        } else if good.is_none() {
            "Nothing has rendered yet.".to_string()
        } else {
            format!("`{}` is not a composition exported from the entry file. <a href=\"/\">All compositions</a>", html_escape(name))
        };
        let page = simple_page("Emul preview", &format!("<p>{msg}</p>"));
        let status = if good.is_none() { 200 } else { 404 };
        return respond_text(req, status, "text/html; charset=utf-8", inject(&page, generation, errors));
    }
    respond_text(req, 404, "text/plain; charset=utf-8", "not found".into());
}

fn simple_page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:2em;}}</style></head>\n<body>\n{body}\n</body></html>\n",
        html_escape(title)
    )
}

/// Adds the reload script, and the error overlay when the latest build failed.
fn inject(html: &str, generation: u64, errors: Option<&str>) -> String {
    let mut extra = String::new();
    if let Some(text) = errors {
        extra.push_str(
            "<div id=\"__emul_errors\" style=\"position:fixed;left:0;right:0;top:0;max-height:60vh;overflow:auto;\
             z-index:2147483647;background:rgba(30,0,0,0.94);color:#ffd7d7;padding:16px 20px;margin:0;\
             font:13px/1.45 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;box-shadow:0 2px 12px rgba(0,0,0,.5)\">\
             <div style=\"font-weight:bold;color:#ff8080;margin-bottom:8px\">The latest build has errors; showing the last good rendering.</div>\
             <pre style=\"margin:0;white-space:pre-wrap\">",
        );
        extra.push_str(&html_escape(text));
        extra.push_str("</pre></div>\n");
    }
    extra.push_str(&format!(
        "<script>(function(){{var v=\"{generation}\";setInterval(function(){{\
         fetch(\"/__emul/version\",{{cache:\"no-store\"}}).then(function(r){{return r.text()}})\
         .then(function(t){{if(t.trim()!==v)location.reload()}}).catch(function(){{}})}},500)}})();</script>\n"
    ));
    match html.rfind("</body>") {
        Some(i) => format!("{}{}{}", &html[..i], extra, &html[i..]),
        None => format!("{html}{extra}"),
    }
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        _ => "application/octet-stream",
    }
}

fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn url_escape(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    match r {
        // Reap the launcher in the background so it doesn't linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("preview: can't open the browser ({e}); open {url} yourself"),
    }
}

// ---------------------------------------------------------------------------
// Window

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum View {
    Svg,
    Png,
    Both,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Pane {
    Svg,
    Png,
}

struct SvgTex {
    generation: u64,
    /// The scale the texture was rasterized at (zoom × pixels per point, maybe clamped).
    scale: f32,
    tex: egui::TextureHandle,
}

struct PngTex {
    generation: u64,
    tex: egui::TextureHandle,
}

struct App {
    shared: Shared,
    url: Option<String>,
    selected: Option<String>,
    view: View,
    zoom: f32,
    /// Set until the first rendering is shown, so it starts fitted.
    initial_fit: bool,
    last_zoom_change: Instant,
    /// Zoom to apply to the scroll offsets of each pane, when the zoom changed: (old, new).
    zoom_changed: Option<(f32, f32)>,
    trees: HashMap<String, (u64, Option<usvg::Tree>)>,
    svg_tex: HashMap<String, SvgTex>,
    png_tex: HashMap<String, PngTex>,
    /// Scroll offset and viewport size of each pane, from the last frame.
    scroll: HashMap<(String, Pane), (egui::Vec2, egui::Vec2)>,
    status: Option<(String, Instant)>,
    show_warnings: bool,
    errors_collapsed: bool,
}

fn run_window(shared: Shared, url: Option<String>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Emul preview")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    let app_shared = shared.clone();
    eframe::run_native(
        "Emul preview",
        options,
        Box::new(move |cc| {
            lock(&app_shared).ctx = Some(cc.egui_ctx.clone());
            Ok(Box::new(App {
                shared: app_shared,
                url,
                selected: None,
                view: View::Svg,
                zoom: 1.0,
                initial_fit: true,
                last_zoom_change: Instant::now() - Duration::from_secs(10),
                zoom_changed: None,
                trees: HashMap::new(),
                svg_tex: HashMap::new(),
                png_tex: HashMap::new(),
                scroll: HashMap::new(),
                status: None,
                show_warnings: false,
                errors_collapsed: false,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

impl App {
    fn set_zoom(&mut self, z: f32) {
        let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
        if (z - self.zoom).abs() > f32::EPSILON {
            let old = self.zoom;
            self.zoom_changed = Some(match self.zoom_changed {
                Some((o, _)) => (o, z),
                None => (old, z),
            });
            self.zoom = z;
            self.last_zoom_change = Instant::now();
        }
    }

    fn fit_zoom(&self, comp: &Rendered, avail: egui::Vec2) -> f32 {
        let panes = if self.view == View::Both { 2.0 } else { 1.0 };
        let gap = 16.0;
        let w = ((avail.x - gap * panes) / panes).max(10.0);
        let h = (avail.y - gap).max(10.0);
        (w / comp.width.max(1) as f32).min(h / comp.height.max(1) as f32)
    }

    fn tree(&mut self, comp: &Rendered, generation: u64) -> Option<&usvg::Tree> {
        let fresh = matches!(self.trees.get(&comp.name), Some((g, _)) if *g == generation);
        if !fresh {
            let tree = comp.svg.as_deref().and_then(|s| usvg::Tree::from_str(s, &usvg::Options::default()).ok());
            self.trees.insert(comp.name.clone(), (generation, tree));
        }
        self.trees.get(&comp.name).and_then(|(_, t)| t.as_ref())
    }

    fn svg_texture(&mut self, ctx: &egui::Context, comp: &Rendered, generation: u64) -> Option<egui::TextureId> {
        let ppp = ctx.pixels_per_point();
        let max_side = ctx.input(|i| i.max_texture_side).min(16384) as f32;
        let biggest = comp.width.max(comp.height).max(1) as f32;
        let scale = (self.zoom * ppp).min(max_side / biggest).max(0.001);
        if let Some(t) = self.svg_tex.get(&comp.name) {
            let same = t.generation == generation && (t.scale - scale).abs() < 1e-4;
            // While the zoom is still changing, stretch the old texture rather than
            // rasterizing every frame.
            let settling = t.generation == generation && self.last_zoom_change.elapsed() < DEBOUNCE;
            if same || settling {
                if settling && !same {
                    ctx.request_repaint_after(DEBOUNCE);
                }
                return Some(t.tex.id());
            }
        }
        let name = comp.name.clone();
        let tree = self.tree(comp, generation)?;
        let size = tree.size();
        let w = (size.width() * scale).round().max(1.0) as u32;
        let h = (size.height() * scale).round().max(1.0) as u32;
        let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
        resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data());
        let tex = ctx.load_texture(format!("svg:{name}"), image, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.svg_tex.insert(name, SvgTex { generation, scale, tex });
        Some(id)
    }

    fn png_texture(&mut self, ctx: &egui::Context, comp: &Rendered, generation: u64) -> Option<egui::TextureId> {
        if let Some(t) = self.png_tex.get(&comp.name) {
            if t.generation == generation {
                return Some(t.tex.id());
            }
        }
        let pm = comp.pixmap.as_ref()?;
        let image = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, ..egui::TextureOptions::LINEAR };
        let tex = ctx.load_texture(format!("png:{}", comp.name), image, options);
        let id = tex.id();
        self.png_tex.insert(comp.name.clone(), PngTex { generation, tex });
        Some(id)
    }

    fn pane(&mut self, ui: &mut egui::Ui, comp: &Rendered, generation: u64, pane: Pane) {
        let ctx = ui.ctx().clone();
        let tex = match pane {
            Pane::Svg => self.svg_texture(&ctx, comp, generation),
            Pane::Png => self.png_texture(&ctx, comp, generation),
        };
        let size = egui::vec2(comp.width as f32 * self.zoom, comp.height as f32 * self.zoom);
        let key = (comp.name.clone(), pane);

        let mut area = egui::ScrollArea::both()
            .id_salt(("emul-pane", comp.name.as_str(), pane as u8))
            .auto_shrink(false)
            .scroll_source(egui::containers::scroll_area::ScrollSource {
                scroll_bar: true,
                drag: egui::containers::scroll_area::DragScroll::Always,
                mouse_wheel: true,
            });
        if let (Some((old, new)), Some((offset, view))) = (self.zoom_changed, self.scroll.get(&key)) {
            // Keep the point at the centre of the view where it was.
            let ratio = new / old;
            let centre = *offset + *view * 0.5;
            let o = centre * ratio - *view * 0.5;
            area = area.scroll_offset(o.max(egui::Vec2::ZERO));
        }
        let out = area.show(ui, |ui| {
            let avail = ui.available_size();
            let margin = 8.0;
            let full = egui::vec2(size.x.max(avail.x - 2.0 * margin) + 2.0 * margin, size.y.max(avail.y - 2.0 * margin) + 2.0 * margin);
            let (rect, _) = ui.allocate_exact_size(full, egui::Sense::hover());
            let min = egui::pos2(
                rect.min.x + ((rect.width() - size.x) / 2.0).max(margin),
                rect.min.y + ((rect.height() - size.y) / 2.0).max(margin),
            );
            let img = egui::Rect::from_min_size(min, size);
            let p = ui.painter();
            draw_checkerboard(p, img);
            match tex {
                Some(t) => {
                    p.image(t, img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                None => {
                    p.text(
                        img.center(),
                        egui::Align2::CENTER_CENTER,
                        "This view isn't available for this composition.",
                        egui::FontId::proportional(14.0),
                        ui.visuals().weak_text_color(),
                    );
                }
            }
            p.rect_stroke(img, 0.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.4)), egui::StrokeKind::Outside);
        });
        self.scroll.insert(key, (out.state.clamped_offset(), out.inner_rect.size()));

        // Ctrl/Cmd + wheel (or pinch) zooms while the pointer is over the pane.
        if ui.rect_contains_pointer(out.inner_rect) {
            let dz = ui.input(|i| i.zoom_delta());
            if (dz - 1.0).abs() > 1e-3 {
                let z = self.zoom * dz;
                self.set_zoom(z);
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, comp: Option<&Rendered>, avail: egui::Vec2, building: bool, generation: u64) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.view, View::Svg, "SVG");
            ui.selectable_value(&mut self.view, View::Png, "PNG");
            ui.selectable_value(&mut self.view, View::Both, "SVG | PNG");
            ui.separator();
            if ui.button("−").on_hover_text("Zoom out").clicked() {
                self.set_zoom(self.zoom / 1.25);
            }
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                self.set_zoom(self.zoom * 1.25);
            }
            if ui.button("Fit").on_hover_text("Fit the canvas to the window").clicked() {
                if let Some(c) = comp {
                    let z = self.fit_zoom(c, avail);
                    self.set_zoom(z);
                }
            }
            if ui.button("100%").clicked() {
                self.set_zoom(1.0);
            }
            if let Some(c) = comp {
                if self.view != View::Svg && c.pixmap.is_some() {
                    let ppp = ui.ctx().pixels_per_point();
                    if ui
                        .button("1:1 px")
                        .on_hover_text("Show one PNG pixel per screen pixel")
                        .clicked()
                    {
                        self.set_zoom(c.scale / ppp);
                    }
                }
            }
            ui.separator();
            match &self.url {
                Some(base) => {
                    let target = match comp {
                        Some(c) => format!("{base}{}", url_escape(&c.name)),
                        None => base.clone(),
                    };
                    if ui.button("Open HTML in browser").on_hover_text(target.as_str()).clicked() {
                        open_in_browser(&target);
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Open HTML in browser"))
                        .on_disabled_hover_text("The HTML view could not be served.");
                }
            }
            ui.separator();
            if building {
                ui.spinner();
                ui.weak("Building…");
            } else if let Some(c) = comp {
                let mut s = format!("{}×{}", c.width, c.height);
                if let Some(pm) = &c.pixmap {
                    s.push_str(&format!("  ·  PNG {}×{} @{}x", pm.width(), pm.height(), c.scale));
                }
                s.push_str(&format!("  ·  build {generation}"));
                ui.weak(s);
            }
        });
    }

    fn error_overlay(&mut self, ctx: &egui::Context, area: egui::Rect, diags: &[Diagnostic], base: &Path) {
        let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        if errors.is_empty() {
            return;
        }
        let width = (area.width() - 24.0).max(200.0);
        egui::Area::new(egui::Id::unique("emul-error-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.min + egui::vec2(12.0, 12.0))
            .show(ctx, |ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(60, 8, 8, 240))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 60, 60)))
                    .corner_radius(6.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.set_width(width - 20.0);
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 140, 140),
                                egui::RichText::new(format!(
                                    "{} error{} — showing the last good rendering",
                                    errors.len(),
                                    if errors.len() == 1 { "" } else { "s" }
                                ))
                                .strong(),
                            );
                            let label = if self.errors_collapsed { "Show" } else { "Hide" };
                            if ui.small_button(label).clicked() {
                                self.errors_collapsed = !self.errors_collapsed;
                            }
                        });
                        if self.errors_collapsed {
                            return;
                        }
                        egui::ScrollArea::vertical().max_height(area.height() * 0.45).show(ui, |ui| {
                            for d in errors {
                                let line = diag_line(d, base);
                                let resp = ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(&line).monospace().color(egui::Color32::from_rgb(255, 215, 215)),
                                        )
                                        .wrap()
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text("Click to copy");
                                if resp.clicked() {
                                    ctx.copy_text(line.clone());
                                    self.status = Some((format!("Copied: {}", diag_location(d, base)), Instant::now()));
                                }
                            }
                        });
                    });
            });
    }
}

fn diag_location(d: &Diagnostic, base: &Path) -> String {
    match (&d.file, d.span) {
        (Some(f), Some(s)) => format!("{}:{}:{}", display_path(f, base), s.line, s.col),
        (Some(f), None) => display_path(f, base),
        _ => String::new(),
    }
}

fn diag_line(d: &Diagnostic, base: &Path) -> String {
    let loc = diag_location(d, base);
    if loc.is_empty() { format!("{} [{}]", d.message, d.code) } else { format!("{loc}: {} [{}]", d.message, d.code) }
}

fn draw_checkerboard(p: &egui::Painter, rect: egui::Rect) {
    // A plain light backdrop, so transparent canvases are visible against dark themes.
    p.rect_filled(rect, 0.0, egui::Color32::from_gray(236));
    let cell = 8.0;
    let clip = p.clip_rect().intersect(rect);
    if !clip.is_positive() {
        return;
    }
    let x0 = ((clip.min.x - rect.min.x) / cell).floor() as i64;
    let x1 = ((clip.max.x - rect.min.x) / cell).ceil() as i64;
    let y0 = ((clip.min.y - rect.min.y) / cell).floor() as i64;
    let y1 = ((clip.max.y - rect.min.y) / cell).ceil() as i64;
    if (x1 - x0) * (y1 - y0) > 40_000 {
        return;
    }
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                continue;
            }
            let r = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + x as f32 * cell, rect.min.y + y as f32 * cell),
                egui::vec2(cell, cell),
            )
            .intersect(rect);
            p.rect_filled(r, 0.0, egui::Color32::from_gray(214));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (generation, building, good, diags, base) = {
            let st = lock(&self.shared);
            (st.generation, st.building, st.good.clone(), st.diags.sorted(), st.base.clone())
        };
        let good_gen = good.as_ref().map(|g| g.generation).unwrap_or(0);

        // Drop cached textures of compositions that are gone or out of date.
        if let Some(g) = &good {
            let names: HashSet<&str> = g.rendered.iter().map(|r| r.name.as_str()).collect();
            self.svg_tex.retain(|n, t| names.contains(n.as_str()) && t.generation == good_gen);
            self.png_tex.retain(|n, t| names.contains(n.as_str()) && t.generation == good_gen);
            self.trees.retain(|n, (g2, _)| names.contains(n.as_str()) && *g2 == good_gen);
            let keep = self.selected.as_ref().is_some_and(|s| names.contains(s.as_str()));
            if !keep {
                self.selected = g.rendered.first().map(|r| r.name.clone());
            }
        }
        let comp: Option<&Rendered> =
            good.as_ref().and_then(|g| g.rendered.iter().find(|r| Some(&r.name) == self.selected.as_ref()));

        let warnings: Vec<&Diagnostic> = diags.iter().filter(|d| d.severity == Severity::Warning).collect();

        // Compositions.
        egui::Panel::left("emul-compositions").default_size(200.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("Compositions");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| match &good {
                Some(g) if !g.rendered.is_empty() => {
                    for r in &g.rendered {
                        let sel = self.selected.as_deref() == Some(r.name.as_str());
                        if ui.selectable_label(sel, r.name.as_str()).clicked() && !sel {
                            self.selected = Some(r.name.clone());
                            self.initial_fit = true;
                        }
                    }
                }
                Some(_) => {
                    ui.weak("The entry file exports no compositions.");
                }
                None if building => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Building…");
                    });
                }
                None => {
                    ui.weak("Nothing has rendered yet.");
                }
            });
        });

        // Status bar: warnings and short messages.
        if let Some((_, t)) = &self.status {
            if t.elapsed() > Duration::from_secs(4) {
                self.status = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        if !warnings.is_empty() || self.status.is_some() {
            egui::Panel::bottom("emul-status").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if !warnings.is_empty() {
                        let label = format!(
                            "{} warning{} {}",
                            warnings.len(),
                            if warnings.len() == 1 { "" } else { "s" },
                            if self.show_warnings { "▾" } else { "▸" }
                        );
                        if ui.add(egui::Button::new(egui::RichText::new(label).color(ui.visuals().warn_fg_color)).frame(false)).clicked() {
                            self.show_warnings = !self.show_warnings;
                        }
                    }
                    if let Some((s, _)) = &self.status {
                        ui.weak(s.as_str());
                    }
                });
                if self.show_warnings {
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        for d in &warnings {
                            let line = diag_line(d, &base);
                            let r = ui
                                .add(egui::Label::new(egui::RichText::new(&line).monospace()).wrap().sense(egui::Sense::click()))
                                .on_hover_text("Click to copy");
                            if r.clicked() {
                                ctx.copy_text(line.clone());
                                self.status = Some((format!("Copied: {}", diag_location(d, &base)), Instant::now()));
                            }
                        }
                    });
                }
            });
        }

        // Toolbar. The space left for the canvas is known only after it is laid out, so
        // estimate it from what remains now minus the toolbar's height.
        let remaining = ui.available_rect_before_wrap();
        let toolbar_h = 34.0;
        let canvas_avail = egui::vec2(remaining.width(), remaining.height() - toolbar_h);
        egui::Panel::top("emul-toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            self.toolbar(ui, comp, canvas_avail, building && good.is_some(), good_gen);
            ui.add_space(3.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            match comp {
                Some(c) => {
                    if self.initial_fit {
                        self.initial_fit = false;
                        let z = self.fit_zoom(c, rect.size()).min(1.0);
                        self.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
                        self.zoom_changed = None;
                    }
                    match self.view {
                        View::Svg => self.pane(ui, c, good_gen, Pane::Svg),
                        View::Png => self.pane(ui, c, good_gen, Pane::Png),
                        View::Both => {
                            let gap = 6.0;
                            let half = (rect.width() - gap) / 2.0;
                            let left = egui::Rect::from_min_size(rect.min, egui::vec2(half, rect.height()));
                            let right = egui::Rect::from_min_size(egui::pos2(rect.min.x + half + gap, rect.min.y), egui::vec2(half, rect.height()));
                            for (r, pane, label) in [(left, Pane::Svg, "SVG"), (right, Pane::Png, "PNG")] {
                                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                                    ui.weak(label);
                                    self.pane(ui, c, good_gen, pane);
                                });
                            }
                            ui.painter().vline(
                                rect.min.x + half + gap / 2.0,
                                rect.y_range(),
                                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                            );
                        }
                    }
                    self.zoom_changed = None;
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        if good.is_none() && building {
                            ui.spinner();
                        } else if good.is_none() {
                            ui.weak("Nothing has rendered yet. Fix the errors and save to try again.");
                        } else {
                            ui.weak("The entry file exports no compositions.");
                        }
                    });
                }
            }
            if generation > 0 {
                self.error_overlay(&ctx, rect, &diags, &base);
            }
        });
    }
}
