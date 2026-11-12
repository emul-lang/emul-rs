//! The HTML output. Almost every element is a `div`. Each component becomes one CSS class.
//! That class holds the full resolved style. A reset stops all CSS cascade.

use crate::fonts::FontSet;
use crate::project::{Child, Node};
use crate::props::{self, css_value, Style, L, V};
use crate::values::{Color, Len};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct HtmlOptions {
    pub width: u32,
    pub height: Option<u32>,
    pub background: Option<Color>,
    pub title: String,
}

pub struct HtmlOutput {
    pub html: String,
    /// The files to copy next to the HTML: (source, name in the output).
    pub assets: Vec<(PathBuf, String)>,
}

fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;")
}

const RESET: &str = "html{margin:0;padding:0}\
body,body *{all:unset;display:block;box-sizing:border-box;\
font-family:\"Inter\";font-size:16px;font-weight:400;font-style:normal;line-height:normal;\
color:#000000;white-space:normal;letter-spacing:normal;text-align:start;text-transform:none;\
text-decoration:none;text-shadow:none;visibility:visible;word-break:normal;overflow-wrap:normal;\
-webkit-text-size-adjust:100%;text-size-adjust:100%}\
body{position:relative;margin:0;overflow:hidden}\
img{object-fit:fill;object-position:50% 50%;overflow:clip;overflow-clip-margin:content-box}";

struct Ctx {
    classes: BTreeMap<String, Arc<Style>>,
    assets: Vec<(PathBuf, String)>,
}

impl Ctx {
    fn asset(&mut self, p: &Path) -> String {
        if let Some((_, name)) = self.assets.iter().find(|(src, _)| src == p) {
            return name.clone();
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("bin");
        let mut name = format!("assets/{stem}.{ext}");
        let mut i = 2;
        while self.assets.iter().any(|(_, n)| *n == name) {
            name = format!("assets/{stem}-{i}.{ext}");
            i += 1;
        }
        self.assets.push((p.to_path_buf(), name.clone()));
        name
    }
}

/// True when all children of a block are blocks. Then the HTML arranges the block as a
/// flex column. A flex column never collapses margins.
fn flows_as_column(node: &Node) -> bool {
    let d = node.style.display();
    if d != "block" || node.style.tag() == "img" {
        return false;
    }
    !node.children.is_empty()
        && node.children.iter().all(|c| match c {
            Child::Text(_) => false,
            Child::Node(n) => n.style.display() != "inline" || n.style.kw(L::Position) == "absolute",
        })
}

fn write_node(out: &mut String, node: &Node, ctx: &mut Ctx) {
    ctx.classes.entry(node.class.clone()).or_insert_with(|| node.style.clone());
    let tag = node.style.tag();
    let _ = write!(out, "<{tag} class=\"{}\"", esc_attr(&node.class));
    if flows_as_column(node) {
        out.push_str(" data-emul-flow=\"column\"");
    }
    for (k, v) in &node.attrs {
        let v = if k == "src" {
            node.src.as_ref().map(|p| ctx.asset(p)).unwrap_or_else(|| v.clone())
        } else {
            v.clone()
        };
        let _ = write!(out, " {k}=\"{}\"", esc_attr(&v));
    }
    out.push('>');
    if props::VOID_TAGS.contains(&tag.as_str()) {
        return;
    }
    // Clamped text goes in an inner box. Then the clamp hides the extra lines, also when
    // the layout makes the element taller than its text.
    let clamp = node.style.int(L::LineClamp).is_some();
    if clamp {
        out.push_str("<span data-emul-clamp>");
    }
    for c in &node.children {
        match c {
            Child::Text(t) => out.push_str(&esc_text(t)),
            Child::Node(n) => write_node(out, n, ctx),
        }
    }
    if clamp {
        out.push_str("</span>");
    }
    let _ = write!(out, "</{tag}>");
}

fn class_css(name: &str, style: &Style, ctx: &mut Ctx) -> String {
    let own = style.color();
    let inline = style.display() == "inline";
    let mut decls: Vec<String> = Vec::new();
    for (l, v) in &style.set {
        if *l == L::HtmlTag || (inline && l.is_box()) {
            continue;
        }
        match l {
            L::LineClamp => {
                continue;
            }
            L::GridArea => {
                if let V::Area(Some(n), _) = v {
                    decls.push(format!("grid-area:{n}"));
                } else {
                    decls.push(format!("grid-area:{}", css_value(*l, v, &own, &|p| p.display().to_string())));
                }
                continue;
            }
            L::BackgroundImage => {
                let mut names = Vec::new();
                if let V::Images(imgs) = v {
                    for i in imgs {
                        if let crate::values::Image::Url(p) = i {
                            names.push((p.clone(), ctx.asset(p)));
                        }
                    }
                }
                let lookup = move |p: &Path| names.iter().find(|(s, _)| s == p).map(|(_, n)| n.clone()).unwrap_or_default();
                decls.push(format!("background-image:{}", css_value(*l, v, &own, &lookup)));
                continue;
            }
            _ => {}
        }
        let value = css_value(*l, v, &own, &|p| p.display().to_string());
        decls.push(format!("{}:{}", l.css(), value));
    }
    // The default colour of text decoration and shadows is the own colour of the component.
    if style.get(L::TextDecoration).is_none() && style.get(L::Color).is_some() {
        // Nothing to add: the default text-decoration is none.
    }
    // A block child with auto side margins and an auto width still fills its line.
    let auto_margin = matches!(style.len(L::MarginLeft), Len::Auto) || matches!(style.len(L::MarginRight), Len::Auto);
    let mut extra = String::new();
    if let Some(n) = style.int(L::LineClamp) {
        let _ = write!(
            extra,
            ".{}>[data-emul-clamp]{{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:{n};line-clamp:{n};overflow:hidden}}",
            css_ident(name)
        );
    }
    if auto_margin && matches!(style.len(L::Width), Len::Auto) && !inline {
        let _ = write!(extra, "[data-emul-flow=\"column\"]>.{}{{width:100%}}", css_ident(name));
    }
    format!(".{}{{{}}}{extra}", css_ident(name), decls.join(";"))
}

fn css_ident(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_string() } else { format!("\\{c}") }).collect()
}

pub fn render(roots: &[Node], fonts: &FontSet, opts: &HtmlOptions) -> HtmlOutput {
    let mut ctx = Ctx { classes: BTreeMap::new(), assets: Vec::new() };
    let mut body = String::new();
    for n in roots {
        write_node(&mut body, n, &mut ctx);
    }
    let root_flow = !roots.is_empty() && roots.iter().all(|n| n.style.display() != "inline" || n.style.kw(L::Position) == "absolute");
    let mut css = String::from(RESET);
    let mut body_css = format!("body{{width:{}px", opts.width);
    if let Some(h) = opts.height {
        let _ = write!(body_css, ";height:{h}px");
    }
    if let Some(bg) = &opts.background {
        let _ = write!(body_css, ";background:{}", bg.css);
    }
    if root_flow {
        body_css.push_str(";display:flex;flex-direction:column;align-items:stretch");
    }
    body_css.push('}');
    css.push_str(&body_css);
    let classes: Vec<(String, Arc<Style>)> = ctx.classes.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (name, style) in &classes {
        css.push_str(&class_css(name, style, &mut ctx));
    }
    // Blocks whose children are all blocks are flex columns, which never collapse
    // margins. Their children neither grow, shrink nor align themselves, as in block
    // layout. These rules come after the classes, so that they have priority.
    css.push_str("[data-emul-flow=\"column\"]{display:flex;flex-direction:column;flex-wrap:nowrap;align-items:stretch;justify-content:flex-start}");
    css.push_str("[data-emul-flow=\"column\"]>*,body>*{flex:0 0 auto;align-self:auto}");
    let mut html = String::new();
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    let _ = writeln!(html, "<meta name=\"viewport\" content=\"width={}\">", opts.width);
    let _ = writeln!(html, "<title>{}</title>", esc_text(&opts.title));
    if let Some(url) = fonts.stylesheet_url() {
        let _ = writeln!(html, "<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>");
        let _ = writeln!(html, "<link rel=\"stylesheet\" href=\"{}\">", esc_attr(&url));
    }
    let _ = writeln!(html, "<style>{css}</style>\n</head>");
    let _ = writeln!(html, "<body>{body}</body>\n</html>");
    HtmlOutput { html, assets: ctx.assets }
}
