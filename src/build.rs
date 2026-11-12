//! The build pipeline: load the project, fetch fonts, arrange and render every
//! composition, and write the results.

use crate::config::{Config, Format, Height, SvgFonts};
use crate::diag::Diags;
use crate::fonts::{self, FontNeeds, FontSet};
use crate::html;
use crate::layout;
use crate::png;
use crate::project::{self, Child, Node, Project};
use crate::props::L;
use crate::svg;
use crate::text::Fonts;
use crate::values::Color;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Settings from command-line flags, which win over the config.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub width: Option<u32>,
    pub height: Option<Height>,
    pub scale: Option<f32>,
    pub background: Option<Color>,
    pub formats: Option<Vec<Format>>,
    pub svg_fonts: Option<SvgFonts>,
    pub out_dir: Option<PathBuf>,
}

impl Overrides {
    pub fn apply(&self, cfg: &mut Config) {
        if let Some(b) = &self.background {
            cfg.background = Some(b.clone());
        }
        if let Some(f) = &self.formats {
            cfg.formats = f.clone();
        }
        if let Some(s) = self.svg_fonts {
            cfg.svg_fonts = s;
        }
        if let Some(o) = &self.out_dir {
            cfg.out_dir = o.clone();
        }
    }
}

pub struct Rendered {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub html: Option<html::HtmlOutput>,
    pub svg: Option<String>,
    pub png: Option<Vec<u8>>,
    pub pixmap: Option<tiny_skia::Pixmap>,
}

pub struct BuildResult {
    pub diags: Diags,
    pub sources: HashMap<PathBuf, String>,
    pub rendered: Vec<Rendered>,
    /// Every file that the build read. The preview watches these files.
    pub inputs: Vec<PathBuf>,
}

fn gather(node: &Node, needs: &mut FontNeeds) {
    let has_text = node.children.iter().any(|c| match c {
        Child::Text(_) => true,
        Child::Node(n) => n.style.display() == "inline",
    });
    if has_text || node.style.display() == "inline" {
        let s = &node.style;
        needs.uses.insert((s.families(), s.weight(), s.italic()));
    }
    for c in &node.children {
        if let Child::Node(n) = c {
            gather(n, needs);
        }
    }
}

fn collect_images(node: &Node, out: &mut Vec<PathBuf>) {
    if let Some(p) = &node.src {
        out.push(p.clone());
    }
    if let Some(crate::props::V::Images(imgs)) = node.style.get(L::BackgroundImage) {
        for i in imgs {
            if let crate::values::Image::Url(p) = i {
                out.push(p.clone());
            }
        }
    }
    for c in &node.children {
        if let Child::Node(n) = c {
            collect_images(n, out);
        }
    }
}

/// Checks the project. It does not fetch fonts and does not render.
pub fn check(cfg: &Config, only: Option<&[String]>) -> Project {
    project::load(&cfg.entry, &cfg.root, only)
}

/// Loads and renders everything in memory. If there are errors, it renders nothing.
pub fn render(cfg: &Config, ov: &Overrides, only: Option<&[String]>, mut diags: Diags) -> BuildResult {
    let proj = check(cfg, only);
    diags.extend(proj.diags.clone());
    let mut inputs: Vec<PathBuf> = proj.files.iter().map(|f| f.path.clone()).collect();
    for c in &proj.compositions {
        for n in &c.root {
            collect_images(n, &mut inputs);
        }
    }
    if let Some(f) = &cfg.file {
        inputs.push(f.clone());
    }
    let sources = proj.sources.clone();
    if let Some(only) = only {
        for name in only {
            if !proj.compositions.iter().any(|c| &c.name == name) && !diags.has_errors() {
                diags.error("unknown-composition", None, None, format!("`{name}` is not a composition exported from the entry file"));
            }
        }
    }
    if diags.has_errors() {
        return BuildResult { diags, sources, rendered: Vec::new(), inputs };
    }
    // Fonts.
    let mut needs = FontNeeds::default();
    for c in &proj.compositions {
        for n in &c.root {
            gather(n, &mut needs);
        }
    }
    let font_set = if needs.uses.is_empty() {
        FontSet::empty()
    } else {
        match fonts::resolve(&needs, &cfg.font_cache) {
            Ok(s) => s,
            Err(errs) => {
                for (code, msg, _) in errs {
                    diags.error(code, None, None, msg);
                }
                return BuildResult { diags, sources, rendered: Vec::new(), inputs };
            }
        }
    };
    let mut fonts = Fonts::new(&font_set);
    let mut rendered = Vec::new();
    for comp in &proj.compositions {
        let (mut w, mut h, mut scale) = cfg.size_for(&comp.name);
        if let Some(x) = ov.width {
            w = x;
        }
        if let Some(x) = ov.height {
            h = x;
        }
        if let Some(x) = ov.scale {
            scale = x;
        }
        let fixed_h = match h {
            Height::Px(p) => Some(p as f32),
            Height::Auto => None,
        };
        let laid = layout::layout(&comp.root, w as f32, fixed_h, &mut fonts);
        let canvas_h = laid.height.ceil() as u32;
        let wants = |f: Format| cfg.formats.contains(&f);
        let html = wants(Format::Html).then(|| {
            html::render(
                &comp.root,
                &font_set,
                &html::HtmlOptions { width: w, height: fixed_h.map(|v| v as u32), background: cfg.background.clone(), title: comp.name.clone() },
            )
        });
        let outline = svg::render(&laid, &mut fonts, &svg::SvgOptions { fonts: SvgFonts::Outline, background: cfg.background.clone() });
        let svg_out = wants(Format::Svg).then(|| {
            if cfg.svg_fonts == SvgFonts::Embed {
                svg::render(&laid, &mut fonts, &svg::SvgOptions { fonts: SvgFonts::Embed, background: cfg.background.clone() })
            } else {
                outline.clone()
            }
        });
        let mut png_out = None;
        let mut pixmap = None;
        if wants(Format::Png) {
            match png::rasterize(&outline, scale) {
                Ok((p, pm)) => {
                    png_out = Some(p);
                    pixmap = Some(pm);
                }
                Err(e) => diags.error("output-too-large", None, None, format!("{}: {e}", comp.name)),
            }
        }
        rendered.push(Rendered { name: comp.name.clone(), width: w, height: canvas_h, scale, html, svg: svg_out, png: png_out, pixmap });
    }
    BuildResult { diags, sources, rendered, inputs }
}

/// Writes the rendered compositions into the output directory.
pub fn write(result: &BuildResult, out_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = Vec::new();
    for r in &result.rendered {
        if let Some(h) = &r.html {
            let p = out_dir.join(format!("{}.html", r.name));
            std::fs::write(&p, &h.html)?;
            written.push(p);
            for (src, name) in &h.assets {
                let dest = out_dir.join(name);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(src, &dest)?;
            }
        }
        if let Some(s) = &r.svg {
            let p = out_dir.join(format!("{}.svg", r.name));
            std::fs::write(&p, s)?;
            written.push(p);
        }
        if let Some(png) = &r.png {
            let p = out_dir.join(format!("{}.png", r.name));
            std::fs::write(&p, png)?;
            written.push(p);
        }
    }
    Ok(written)
}

/// Finds and loads the config: an explicit path, `emul.config.pi` in the current
/// directory, or the defaults.
pub fn load_config(explicit: Option<&Path>, diags: &mut Diags) -> Config {
    if let Some(p) = explicit {
        return crate::config::load(p, diags);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let p = cwd.join("emul.config.pi");
    if p.is_file() {
        crate::config::load(&p, diags)
    } else {
        Config::defaults(&cwd)
    }
}
