//! Reads `emul.config.pi`. The config is written in Piton; this reads the subset of
//! Piton it uses: an exported `emul-config` anchor holding settings, lists and nested
//! per-composition sizes.

use crate::diag::{Diags, Span};
use crate::values::{parse_color, tokenize, Color};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Copy)]
pub enum Format {
    Html,
    Svg,
    Png,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "html" => Some(Format::Html),
            "svg" => Some(Format::Svg),
            "png" => Some(Format::Png),
            _ => None,
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Svg => "svg",
            Format::Png => "png",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Height {
    Auto,
    Px(u32),
}

#[derive(Clone, Debug, Default)]
pub struct SizeOverride {
    pub width: Option<u32>,
    pub height: Option<Height>,
    pub scale: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgFonts {
    Outline,
    Embed,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// The directory holding the config file, or the current directory.
    pub base: PathBuf,
    pub root: PathBuf,
    pub entry: PathBuf,
    pub out_dir: PathBuf,
    pub formats: Vec<Format>,
    pub width: u32,
    pub height: Height,
    pub scale: f32,
    pub sizes: HashMap<String, SizeOverride>,
    pub background: Option<Color>,
    pub svg_fonts: SvgFonts,
    pub font_cache: PathBuf,
    pub file: Option<PathBuf>,
}

pub fn default_font_cache() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("emul")
}

impl Config {
    pub fn defaults(base: &Path) -> Config {
        Config {
            base: base.to_path_buf(),
            root: base.to_path_buf(),
            entry: base.join("index.emul"),
            out_dir: base.join("out"),
            formats: vec![Format::Html, Format::Svg, Format::Png],
            width: 1280,
            height: Height::Auto,
            scale: 1.0,
            sizes: HashMap::new(),
            background: None,
            svg_fonts: SvgFonts::Outline,
            font_cache: default_font_cache(),
            file: None,
        }
    }

    /// The size a composition renders at, before command-line flags.
    pub fn size_for(&self, name: &str) -> (u32, Height, f32) {
        let o = self.sizes.get(name);
        (
            o.and_then(|o| o.width).unwrap_or(self.width),
            o.and_then(|o| o.height).unwrap_or(self.height),
            o.and_then(|o| o.scale).unwrap_or(self.scale),
        )
    }
}

#[derive(Debug, Clone)]
enum PVal {
    Scalar(String, Span),
    List(Vec<(String, Span)>),
    Dict(Vec<(String, Span, PVal)>),
}

/// Loads the config at `path`. Errors are reported into `diags`.
pub fn load(path: &Path, diags: &mut Diags) -> Config {
    let base = path.parent().map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p }).unwrap_or(Path::new(".")).to_path_buf();
    let base = std::fs::canonicalize(&base).unwrap_or(base);
    let mut cfg = Config::defaults(&base);
    cfg.file = Some(path.to_path_buf());
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            diags.error("invalid-config", Some(path), None, format!("can't read the config file: {e}"));
            return cfg;
        }
    };
    let lines: Vec<&str> = src.lines().collect();
    // Find the exported emul-config anchor.
    let mut anchors = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if l.starts_with("export ") && t.split_whitespace().nth(1) == Some("emul-config") {
            anchors.push(i);
        }
    }
    if anchors.len() != 1 {
        diags.error(
            "invalid-config",
            Some(path),
            None,
            if anchors.is_empty() { "the config must export one `emul-config` anchor" } else { "the config exports more than one `emul-config` anchor" },
        );
        return cfg;
    }
    let start = anchors[0];
    if !lines[start].trim_end().ends_with(':') {
        diags.error("invalid-config", Some(path), Some(Span::new(start + 1, 1, lines[start].len())), "the `emul-config` anchor must end with a colon");
        return cfg;
    }
    let mut i = start + 1;
    let body = parse_block(&lines, &mut i, indent_of(lines.get(start + 1).copied().unwrap_or("")), path, diags);
    let PVal::Dict(entries) = body else { return cfg };
    let at = |base: &Path, p: &str| -> PathBuf { crate::props::normalize(&base.join(p)) };
    let mut root_set = false;
    let mut entry_set = None;
    for (key, span, val) in entries {
        let scalar = |v: &PVal| -> Option<(String, Span)> {
            if let PVal::Scalar(s, sp) = v { Some((s.clone(), *sp)) } else { None }
        };
        match key.as_str() {
            "root" => match scalar(&val) {
                Some((s, _)) => {
                    cfg.root = at(&base, &s);
                    root_set = true;
                }
                None => bad(diags, path, span, "root is a path"),
            },
            "entry" => match scalar(&val) {
                Some((s, _)) => entry_set = Some(at(&base, &s)),
                None => bad(diags, path, span, "entry is a path"),
            },
            "outDir" => match scalar(&val) {
                Some((s, _)) => cfg.out_dir = at(&base, &s),
                None => bad(diags, path, span, "outDir is a path"),
            },
            "fontCache" => match scalar(&val) {
                Some((s, _)) => cfg.font_cache = at(&base, &s),
                None => bad(diags, path, span, "fontCache is a path"),
            },
            "formats" => {
                let items: Vec<(String, Span)> = match &val {
                    PVal::List(l) => l.clone(),
                    PVal::Scalar(s, sp) => s.trim_matches(['[', ']']).split(',').map(|x| (x.trim().to_string(), *sp)).collect(),
                    _ => Vec::new(),
                };
                let mut f = Vec::new();
                for (s, sp) in items {
                    match Format::parse(&s) {
                        Some(x) => f.push(x),
                        None => diags.error("unknown-format", Some(path), Some(sp), format!("`{s}` is not a format; use html, svg or png")),
                    }
                }
                cfg.formats = f;
            }
            "width" => match scalar(&val).and_then(|(s, sp)| parse_dim(&s).map(|v| (v, sp))) {
                Some((v, _)) => cfg.width = v,
                None => diags.error("invalid-size", Some(path), Some(span), "width must be a whole number from 1 to 16384"),
            },
            "height" => match scalar(&val).and_then(|(s, _)| parse_height(&s)) {
                Some(h) => cfg.height = h,
                None => diags.error("invalid-size", Some(path), Some(span), "height must be `auto` or a whole number from 1 to 16384"),
            },
            "scale" => match scalar(&val).and_then(|(s, _)| s.parse::<f32>().ok()).filter(|v| *v > 0.0) {
                Some(v) => cfg.scale = v,
                None => diags.error("invalid-size", Some(path), Some(span), "scale must be a positive number"),
            },
            "background" => match scalar(&val) {
                Some((s, sp)) if s == "none" || s == "null" => {
                    let _ = sp;
                    cfg.background = None
                }
                Some((s, sp)) => match tokenize(&s).ok().and_then(|t| t.first().cloned()).map(|t| parse_color(&t)) {
                    Some(Ok(c)) => cfg.background = Some(c),
                    Some(Err(e)) => diags.error("invalid-setting", Some(path), Some(sp), format!("background: {e}")),
                    None => diags.error("invalid-setting", Some(path), Some(sp), "background is a colour"),
                },
                None => bad(diags, path, span, "background is a colour"),
            },
            "svgFonts" => match scalar(&val).as_ref().map(|(s, _)| s.as_str()) {
                Some("outline") => cfg.svg_fonts = SvgFonts::Outline,
                Some("embed") => cfg.svg_fonts = SvgFonts::Embed,
                _ => bad(diags, path, span, "svgFonts is `outline` or `embed`"),
            },
            "sizes" => match val {
                PVal::Dict(comps) => {
                    for (name, nspan, v) in comps {
                        let PVal::Dict(settings) = v else {
                            bad(diags, path, nspan, "each entry in sizes holds width, height or scale");
                            continue;
                        };
                        let mut o = SizeOverride::default();
                        for (k, ks, v) in settings {
                            let s = scalar(&v).map(|x| x.0).unwrap_or_default();
                            match k.as_str() {
                                "width" => match parse_dim(&s) {
                                    Some(x) => o.width = Some(x),
                                    None => diags.error("invalid-size", Some(path), Some(ks), "width must be a whole number from 1 to 16384"),
                                },
                                "height" => match parse_height(&s) {
                                    Some(x) => o.height = Some(x),
                                    None => diags.error("invalid-size", Some(path), Some(ks), "height must be `auto` or a whole number from 1 to 16384"),
                                },
                                "scale" => match s.parse::<f32>().ok().filter(|v| *v > 0.0) {
                                    Some(x) => o.scale = Some(x),
                                    None => diags.error("invalid-size", Some(path), Some(ks), "scale must be a positive number"),
                                },
                                other => bad(diags, path, ks, &format!("`{other}` is not a size setting; use width, height or scale")),
                            }
                        }
                        cfg.sizes.insert(name, o);
                    }
                }
                _ => bad(diags, path, span, "sizes holds one entry per composition"),
            },
            other => bad(diags, path, span, &format!("`{other}` is not a config setting")),
        }
    }
    cfg.entry = entry_set.unwrap_or_else(|| cfg.root.join("index.emul"));
    let _ = root_set;
    cfg
}

fn bad(diags: &mut Diags, path: &Path, span: Span, msg: &str) {
    diags.error("invalid-setting", Some(path), Some(span), msg.to_string());
}

pub fn parse_dim(s: &str) -> Option<u32> {
    s.trim().trim_end_matches("px").parse::<u32>().ok().filter(|v| (1..=16384).contains(v))
}

pub fn parse_height(s: &str) -> Option<Height> {
    if s.trim() == "auto" { Some(Height::Auto) } else { parse_dim(s).map(Height::Px) }
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// Parses an indented block at `indent` into a dictionary or list.
fn parse_block(lines: &[&str], i: &mut usize, indent: usize, path: &Path, diags: &mut Diags) -> PVal {
    let mut dict = Vec::new();
    let mut list = Vec::new();
    while *i < lines.len() {
        let l = lines[*i];
        let t = l.trim();
        if t.is_empty() || t.starts_with("//") {
            *i += 1;
            continue;
        }
        let ind = indent_of(l);
        if ind < indent || indent == 0 {
            break;
        }
        let span = Span::new(*i + 1, ind + 1, t.len());
        if let Some(item) = t.strip_prefix("- ") {
            list.push((item.trim().to_string(), span));
            *i += 1;
            continue;
        }
        let Some(colon) = t.find(':') else {
            diags.error("invalid-config", Some(path), Some(span), "expected `name: value`");
            *i += 1;
            continue;
        };
        let key = t[..colon].trim().to_string();
        let rest = t[colon + 1..].trim();
        *i += 1;
        if rest.is_empty() {
            let next_indent = lines[*i..].iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l)).unwrap_or(0);
            if next_indent > ind {
                let v = parse_block(lines, i, next_indent, path, diags);
                dict.push((key, span, v));
            } else {
                dict.push((key, span, PVal::Scalar(String::new(), span)));
            }
        } else {
            let vspan = Span::new(span.line, ind + colon + 2, rest.len());
            dict.push((key, span, PVal::Scalar(rest.to_string(), vspan)));
        }
    }
    if !list.is_empty() && dict.is_empty() { PVal::List(list) } else { PVal::Dict(dict) }
}
