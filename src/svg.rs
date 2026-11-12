//! The SVG output. It is the reference render of Emul. The code draws each box, border,
//! shadow and glyph at its final position. The SVG has no layout of its own.

use crate::config::SvgFonts;
use crate::layout::{LBox, LItem, LText, Laid, Rect};
use crate::props::{self, BgSize, Style, L, V};
use crate::text::{Fonts, SpanStyle};
use crate::values::{resolve_stops, Color, Image, Len, LinearDir, PosAxis};
use base64::Engine;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub struct SvgOptions {
    pub fonts: SvgFonts,
    pub background: Option<Color>,
}

pub fn n(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The fill attributes for a colour. SVG 1.1 viewers need a separate opacity attribute.
fn fill(c: &Color) -> String {
    if c.a >= 1.0 { format!("fill=\"{}\"", c.hex6()) } else { format!("fill=\"{}\" fill-opacity=\"{}\"", c.hex6(), n(c.a)) }
}

fn stroke(c: &Color) -> String {
    if c.a >= 1.0 { format!("stroke=\"{}\"", c.hex6()) } else { format!("stroke=\"{}\" stroke-opacity=\"{}\"", c.hex6(), n(c.a)) }
}

type Radii = [(f32, f32); 4];

/// A path for a rectangle with round corners. The radii are for the top-left, top-right,
/// bottom-right and bottom-left corners. Each radius is (horizontal, vertical).
fn rrect(r: Rect, rad: Radii) -> String {
    let [tl, tr, br, bl] = rad;
    if rad.iter().all(|c| c.0 <= 0.0 || c.1 <= 0.0) {
        return format!("M{} {}H{}V{}H{}Z", n(r.x), n(r.y), n(r.x + r.w), n(r.y + r.h), n(r.x));
    }
    let mut d = String::new();
    let _ = write!(d, "M{} {}", n(r.x + tl.0), n(r.y));
    let _ = write!(d, "H{}", n(r.x + r.w - tr.0));
    if tr.0 > 0.0 && tr.1 > 0.0 {
        let _ = write!(d, "A{} {} 0 0 1 {} {}", n(tr.0), n(tr.1), n(r.x + r.w), n(r.y + tr.1));
    }
    let _ = write!(d, "V{}", n(r.y + r.h - br.1));
    if br.0 > 0.0 && br.1 > 0.0 {
        let _ = write!(d, "A{} {} 0 0 1 {} {}", n(br.0), n(br.1), n(r.x + r.w - br.0), n(r.y + r.h));
    }
    let _ = write!(d, "H{}", n(r.x + bl.0));
    if bl.0 > 0.0 && bl.1 > 0.0 {
        let _ = write!(d, "A{} {} 0 0 1 {} {}", n(bl.0), n(bl.1), n(r.x), n(r.y + r.h - bl.1));
    }
    let _ = write!(d, "V{}", n(r.y + tl.1));
    if tl.0 > 0.0 && tl.1 > 0.0 {
        let _ = write!(d, "A{} {} 0 0 1 {} {}", n(tl.0), n(tl.1), n(r.x + tl.0), n(r.y));
    }
    d.push('Z');
    d
}

/// The used corner radii of a box. When the radii overlap, the code makes them smaller,
/// as CSS does.
fn radii(s: &Style, r: Rect) -> Radii {
    let mut out = [(0.0, 0.0); 4];
    for (i, l) in props::RADII.iter().enumerate() {
        out[i] = match s.len(*l) {
            Len::Px(v) => (v, v),
            Len::Pct(p) => (p / 100.0 * r.w, p / 100.0 * r.h),
            _ => (0.0, 0.0),
        };
    }
    let mut f: f32 = 1.0;
    let sums = [(out[0].0 + out[1].0, r.w), (out[3].0 + out[2].0, r.w), (out[0].1 + out[3].1, r.h), (out[1].1 + out[2].1, r.h)];
    for (sum, side) in sums {
        if sum > side && sum > 0.0 {
            f = f.min(side / sum);
        }
    }
    if f < 1.0 {
        for c in out.iter_mut() {
            c.0 *= f;
            c.1 *= f;
        }
    }
    out
}

/// The radii minus the border widths, for the inner edge of a border.
fn inner_radii(rad: Radii, b: [f32; 4]) -> Radii {
    let [t, r, bo, l] = b;
    [
        ((rad[0].0 - l).max(0.0), (rad[0].1 - t).max(0.0)),
        ((rad[1].0 - r).max(0.0), (rad[1].1 - t).max(0.0)),
        ((rad[2].0 - r).max(0.0), (rad[2].1 - bo).max(0.0)),
        ((rad[3].0 - l).max(0.0), (rad[3].1 - bo).max(0.0)),
    ]
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect { x: r.x - by, y: r.y - by, w: (r.w + 2.0 * by).max(0.0), h: (r.h + 2.0 * by).max(0.0) }
}

fn grow_radii(rad: Radii, by: f32) -> Radii {
    rad.map(|(x, y)| if x > 0.0 || y > 0.0 { ((x + by).max(0.0), (y + by).max(0.0)) } else { (0.0, 0.0) })
}

struct Writer<'a> {
    defs: String,
    body: String,
    next_id: usize,
    fonts: &'a mut Fonts,
    opts: &'a SvgOptions,
    images: HashMap<PathBuf, Option<(String, f32, f32)>>,
    glyphs: HashMap<(cosmic_text::fontdb::ID, u16), Option<GlyphShape>>,
    embedded: BTreeMap<String, String>,
}

#[derive(Clone)]
enum GlyphShape {
    /// An outline in font units (y up), and the units per em of the font.
    Outline(String, f32),
    /// The colour layers: (path in font units, colour), and the units per em.
    Layers(Vec<(String, Option<Color>)>, f32),
    /// A bitmap: PNG data, x, y, width and height. The values are in font units, scaled
    /// from pixels per em.
    Bitmap(String, f32, f32, f32, f32, f32),
}

struct PathBuilder {
    d: String,
}

impl ttf_parser::OutlineBuilder for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "M{x} {y}");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.d, "L{x} {y}");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(self.d, "Q{x1} {y1} {x} {y}");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(self.d, "C{x1} {y1} {x2} {y2} {x} {y}");
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

struct ColorPainter<'f> {
    face: &'f ttf_parser::Face<'f>,
    current: Option<String>,
    layers: Vec<(String, Option<Color>)>,
}

fn rgba(c: ttf_parser::RgbaColor) -> Color {
    Color::rgba(c.red as f32 / 255.0, c.green as f32 / 255.0, c.blue as f32 / 255.0, c.alpha as f32 / 255.0)
}

impl<'a, 'f> ttf_parser::colr::Painter<'a> for ColorPainter<'f> {
    fn outline_glyph(&mut self, glyph_id: ttf_parser::GlyphId) {
        let mut b = PathBuilder { d: String::new() };
        self.current = self.face.outline_glyph(glyph_id, &mut b).map(|_| b.d);
    }
    fn paint(&mut self, paint: ttf_parser::colr::Paint<'a>) {
        let color = match paint {
            ttf_parser::colr::Paint::Solid(c) => Some(rgba(c)),
            ttf_parser::colr::Paint::LinearGradient(g) => g.stops(0, &[]).next().map(|s| rgba(s.color)),
            ttf_parser::colr::Paint::RadialGradient(g) => g.stops(0, &[]).next().map(|s| rgba(s.color)),
            ttf_parser::colr::Paint::SweepGradient(g) => g.stops(0, &[]).next().map(|s| rgba(s.color)),
        };
        if let Some(d) = self.current.clone() {
            self.layers.push((d, color));
        }
    }
    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _clipbox: ttf_parser::colr::ClipBox) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _mode: ttf_parser::colr::CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _transform: ttf_parser::Transform) {}
    fn pop_transform(&mut self) {}
}

/// Changes a path in font units into canvas coordinates.
fn place_path(d: &str, ox: f32, oy: f32, scale: f32, out: &mut String) {
    let mut num = String::new();
    let mut is_x = true;
    let flush = |num: &mut String, is_x: &mut bool, out: &mut String| {
        if num.is_empty() {
            return;
        }
        let v: f32 = num.parse().unwrap_or(0.0);
        let p = if *is_x { ox + v * scale } else { oy - v * scale };
        let _ = write!(out, "{}", n(p));
        *is_x = !*is_x;
        num.clear();
    };
    for c in d.chars() {
        match c {
            'M' | 'L' | 'Q' | 'C' | 'Z' => {
                flush(&mut num, &mut is_x, out);
                out.push(c);
                is_x = true;
            }
            ' ' => {
                flush(&mut num, &mut is_x, out);
                out.push(' ');
            }
            _ => num.push(c),
        }
    }
    flush(&mut num, &mut is_x, out);
}

impl<'a> Writer<'a> {
    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    fn image_data(&mut self, p: &Path) -> Option<(String, f32, f32)> {
        if let Some(v) = self.images.get(p) {
            return v.clone();
        }
        let v = (|| {
            let bytes = std::fs::read(p).ok()?;
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            let mime = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                _ => return None,
            };
            let (w, h) = crate::layout::natural_image_size(&p.to_path_buf())?;
            Some((format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)), w, h))
        })();
        self.images.insert(p.to_path_buf(), v.clone());
        v
    }

    fn clip_path(&mut self, d: &str, evenodd: bool) -> String {
        let id = self.id("c");
        let rule = if evenodd { " clip-rule=\"evenodd\"" } else { "" };
        let _ = write!(self.defs, "<clipPath id=\"{id}\"><path d=\"{d}\"{rule}/></clipPath>");
        id
    }

    fn blur_filter(&mut self, std_dev: f32, region: Rect) -> String {
        let id = self.id("f");
        let _ = write!(
            self.defs,
            "<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><feGaussianBlur stdDeviation=\"{}\"/></filter>",
            n(region.x),
            n(region.y),
            n(region.w),
            n(region.h),
            n(std_dev)
        );
        id
    }

    // ------------------------------------------------------------ paint order

    fn paint_root(&mut self, laid: &Laid) {
        let mut layers: Vec<(i32, usize, &LBox)> = Vec::new();
        let mut order = 0;
        for it in &laid.items {
            if let LItem::Box(b) = it {
                collect_layers(b, &mut layers, &mut order);
            }
        }
        self.paint_layers(&layers, |z| z < 0);
        for it in &laid.items {
            self.paint_flow_item(it);
        }
        self.paint_layers(&layers, |z| z >= 0);
    }

    fn paint_layers(&mut self, layers: &[(i32, usize, &LBox)], pick: impl Fn(i32) -> bool) {
        let mut v: Vec<&(i32, usize, &LBox)> = layers.iter().filter(|(z, _, _)| pick(*z)).collect();
        v.sort_by_key(|(z, o, _)| (*z, *o));
        for (_, _, b) in v {
            self.paint_atomic(b);
        }
    }

    fn paint_flow_item(&mut self, it: &LItem) {
        match it {
            LItem::Text(t) => self.paint_text(t),
            LItem::Box(b) => {
                if b.style.display() == "none" {
                    return;
                }
                match layer_of(b) {
                    Some(_) => {} // A layer pass paints this box.
                    None if is_atomic(b) => self.paint_atomic(b),
                    None => {
                        self.paint_decorations(b);
                        if b.src.is_some() {
                            self.paint_image(b);
                        }
                        for c in &b.children {
                            self.paint_flow_item(c);
                        }
                    }
                }
            }
        }
    }

    /// Paints a box and all its contents as a unit. The box has its own stack context.
    fn paint_atomic(&mut self, b: &LBox) {
        if b.style.display() == "none" {
            return;
        }
        let s = &b.style;
        let mut attrs = format!(" id=\"{}\"", esc(&b.id));
        let opacity = s.num(L::Opacity);
        if opacity < 1.0 {
            let _ = write!(attrs, " opacity=\"{}\"", n(opacity));
        }
        if let Some(t) = transform_attr(s, b.rect) {
            let _ = write!(attrs, " transform=\"{t}\"");
        }
        let _ = write!(self.body, "<g{attrs}>");
        self.paint_decorations(b);
        if b.src.is_some() {
            self.paint_image(b);
        }
        let clip = if s.kw(L::Overflow) == "hidden" {
            let pb = b.padding_box();
            let rad = inner_radii(radii(s, b.rect), b.border);
            let id = self.clip_path(&rrect(pb, rad), false);
            Some(id)
        } else {
            None
        };
        if let Some(c) = &clip {
            let _ = write!(self.body, "<g clip-path=\"url(#{c})\">");
        }
        let mut layers = Vec::new();
        let mut order = 0;
        for c in &b.children {
            if let LItem::Box(cb) = c {
                collect_layers(cb, &mut layers, &mut order);
            }
        }
        self.paint_layers(&layers, |z| z < 0);
        for c in &b.children {
            self.paint_flow_item(c);
        }
        self.paint_layers(&layers, |z| z >= 0);
        if clip.is_some() {
            self.body.push_str("</g>");
        }
        self.body.push_str("</g>");
    }

    // ------------------------------------------------------------ boxes

    fn paint_decorations(&mut self, b: &LBox) {
        let s = &b.style;
        if s.display() == "inline" {
            return;
        }
        let r = b.rect;
        let rad = radii(s, r);
        let own = s.color();
        let shadows = s.shadows(L::BoxShadow);
        // The outer shadows. The first shadow in the list is on top.
        for sh in shadows.iter().rev().filter(|sh| !sh.inset) {
            let color = sh.color.clone().unwrap_or_else(|| own.clone());
            let shape = grow(Rect { x: r.x + sh.x, y: r.y + sh.y, ..r }, sh.spread);
            let d = rrect(shape, grow_radii(rad, sh.spread));
            let reach = sh.blur * 1.5 + sh.spread.abs() + sh.x.abs() + sh.y.abs() + 4.0;
            let big = grow(r, reach + sh.blur * 2.0);
            let clip = self.clip_path(&format!("{}{}", rrect(big, [(0.0, 0.0); 4]), rrect(r, rad)), true);
            let filter = if sh.blur > 0.0 { format!(" filter=\"url(#{})\"", self.blur_filter(sh.blur / 2.0, big)) } else { String::new() };
            let _ = write!(self.body, "<g clip-path=\"url(#{clip})\"><path d=\"{d}\" {}{filter}/></g>", fill(&color));
        }
        // The background colour and images. The code clips them to the border box.
        let border_path = rrect(r, rad);
        if let Some(V::OptColor(Some(c))) = s.get(L::BackgroundColor) {
            let _ = write!(self.body, "<path d=\"{border_path}\" {}/>", fill(c));
        }
        if let Some(V::Images(imgs)) = s.get(L::BackgroundImage) {
            if !imgs.is_empty() {
                let clip = self.clip_path(&border_path, false);
                let _ = write!(self.body, "<g clip-path=\"url(#{clip})\">");
                for img in imgs.iter().rev() {
                    self.paint_background_image(b, img);
                }
                self.body.push_str("</g>");
            }
        }
        // The inset shadows, in the padding box.
        let pb = b.padding_box();
        let irad = inner_radii(rad, b.border);
        for sh in shadows.iter().rev().filter(|sh| sh.inset) {
            let color = sh.color.clone().unwrap_or_else(|| own.clone());
            let hole_rect = grow(Rect { x: pb.x + sh.x, y: pb.y + sh.y, ..pb }, -sh.spread);
            let hole = rrect(hole_rect, grow_radii(irad, -sh.spread));
            let big = grow(pb, sh.blur * 3.0 + sh.spread.abs() + sh.x.abs() + sh.y.abs() + 4.0);
            let clip = self.clip_path(&rrect(pb, irad), false);
            let filter = if sh.blur > 0.0 { format!(" filter=\"url(#{})\"", self.blur_filter(sh.blur / 2.0, big)) } else { String::new() };
            let _ = write!(
                self.body,
                "<g clip-path=\"url(#{clip})\"><path d=\"{}{hole}\" fill-rule=\"evenodd\" {}{filter}/></g>",
                rrect(big, [(0.0, 0.0); 4]),
                fill(&color)
            );
        }
        self.paint_border(b, rad);
    }

    fn paint_border(&mut self, b: &LBox, rad: Radii) {
        let s = &b.style;
        let w = b.border;
        if w.iter().all(|v| *v <= 0.0) {
            return;
        }
        let styles: Vec<&str> = (0..4).map(|i| s.kw(props::BORDER_STYLE[i])).collect();
        let colors: Vec<Color> = (0..4).map(|i| s.border_color(i)).collect();
        let r = b.rect;
        let inner = b.padding_box();
        let irad = inner_radii(rad, w);
        let uniform = (1..4).all(|i| styles[i] == styles[0] && colors[i] == colors[0] && (w[i] > 0.0) == (w[0] > 0.0));
        let ring = |outer: Rect, orad: Radii, inn: Rect, inrad: Radii| format!("{}{}", rrect(outer, orad), rrect(inn, inrad));
        // A trapezoid for each side, from the outer corners to the inner corners.
        let side_poly = |i: usize| -> String {
            let (ox0, oy0, ox1, oy1) = (r.x, r.y, r.x + r.w, r.y + r.h);
            let (ix0, iy0, ix1, iy1) = (inner.x, inner.y, inner.x + inner.w, inner.y + inner.h);
            let pts = match i {
                0 => [(ox0, oy0), (ox1, oy0), (ix1, iy0), (ix0, iy0)],
                1 => [(ox1, oy0), (ox1, oy1), (ix1, iy1), (ix1, iy0)],
                2 => [(ox1, oy1), (ox0, oy1), (ix0, iy1), (ix1, iy1)],
                _ => [(ox0, oy1), (ox0, oy0), (ix0, iy0), (ix0, iy1)],
            };
            format!("M{} {}L{} {}L{} {}L{} {}Z", n(pts[0].0), n(pts[0].1), n(pts[1].0), n(pts[1].1), n(pts[2].0), n(pts[2].1), n(pts[3].0), n(pts[3].1))
        };
        let sides: Vec<usize> = if uniform { vec![4] } else { (0..4).filter(|&i| w[i] > 0.0 && styles[i] != "none").collect() };
        for i in sides {
            let (style, color) = if i == 4 { (styles[0], colors[0].clone()) } else { (styles[i], colors[i].clone()) };
            if style == "none" {
                continue;
            }
            let clip = if i == 4 { None } else { Some(self.clip_path(&side_poly(i), false)) };
            if let Some(c) = &clip {
                let _ = write!(self.body, "<g clip-path=\"url(#{c})\">");
            }
            match style {
                "double" => {
                    let third = [w[0] / 3.0, w[1] / 3.0, w[2] / 3.0, w[3] / 3.0];
                    let mid1 = inset(r, third);
                    let mid2 = inset(r, [third[0] * 2.0, third[1] * 2.0, third[2] * 2.0, third[3] * 2.0]);
                    let d1 = ring(r, rad, mid1, inner_radii(rad, third));
                    let d2 = ring(mid2, inner_radii(rad, third.map(|t| t * 2.0)), inner, irad);
                    let _ = write!(self.body, "<path d=\"{d1}{d2}\" fill-rule=\"evenodd\" {}/>", fill(&color));
                }
                "dashed" | "dotted" => {
                    let sw = if i == 4 { w[0] } else { w[i] };
                    let mid = inset(r, [w[0] / 2.0, w[1] / 2.0, w[2] / 2.0, w[3] / 2.0]);
                    let mrad = inner_radii(rad, [w[0] / 2.0, w[1] / 2.0, w[2] / 2.0, w[3] / 2.0]);
                    let dash = if style == "dashed" {
                        format!(" stroke-dasharray=\"{} {}\"", n(sw * 3.0), n(sw * 3.0))
                    } else {
                        format!(" stroke-dasharray=\"0 {}\" stroke-linecap=\"round\"", n(sw * 2.0))
                    };
                    let _ = write!(self.body, "<path d=\"{}\" fill=\"none\" {} stroke-width=\"{}\"{dash}/>", rrect(mid, mrad), stroke(&color), n(sw));
                }
                _ => {
                    let d = ring(r, rad, inner, irad);
                    let _ = write!(self.body, "<path d=\"{d}\" fill-rule=\"evenodd\" {}/>", fill(&color));
                }
            }
            if clip.is_some() {
                self.body.push_str("</g>");
            }
        }
    }

    fn paint_background_image(&mut self, b: &LBox, img: &Image) {
        let s = &b.style;
        let area = b.padding_box();
        let (iw, ih, href) = match img {
            Image::Url(p) => match self.image_data(p) {
                Some((h, w, hh)) => (w, hh, Some(h)),
                None => return,
            },
            _ => (area.w, area.h, None),
        };
        // The size of one tile.
        let (tw, th) = match s.get(L::BackgroundSize) {
            Some(V::BgSize(BgSize::Cover)) if href.is_some() => {
                let k = (area.w / iw).max(area.h / ih);
                (iw * k, ih * k)
            }
            Some(V::BgSize(BgSize::Contain)) if href.is_some() => {
                let k = (area.w / iw).min(area.h / ih);
                (iw * k, ih * k)
            }
            Some(V::BgSize(BgSize::Explicit(a, bb))) => {
                let w = a.resolve(area.w);
                let h = bb.resolve(area.h);
                match (w, h) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => (w, if iw > 0.0 { w * ih / iw } else { area.h }),
                    (None, Some(h)) => (if ih > 0.0 { h * iw / ih } else { area.w }, h),
                    (None, None) => (iw, ih),
                }
            }
            _ => (iw, ih),
        };
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        let (px, py) = match s.get(L::BackgroundPosition) {
            Some(V::Pos(x, y)) => (*x, *y),
            _ => (PosAxis { pct: 0.0, px: 0.0 }, PosAxis { pct: 0.0, px: 0.0 }),
        };
        let x = area.x + px.place(area.w, tw);
        let y = area.y + py.place(area.h, th);
        let repeat = s.kw(L::BackgroundRepeat);
        let content = match (img, &href) {
            (Image::Url(_), Some(h)) => format!("<image href=\"{h}\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"/>", n(tw), n(th)),
            _ => {
                let gid = self.gradient(img, Rect { x: 0.0, y: 0.0, w: tw, h: th });
                format!("<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"url(#{gid})\"/>", n(tw), n(th))
            }
        };
        let br = b.rect;
        if repeat == "no-repeat" {
            let _ = write!(self.body, "<g transform=\"translate({} {})\">{content}</g>", n(x), n(y));
            return;
        }
        let (rx, ry) = match repeat {
            "repeat-x" => (true, false),
            "repeat-y" => (false, true),
            _ => (true, true),
        };
        let pid = self.id("p");
        let pw = if rx { tw } else { br.w + (x - br.x).abs() + tw };
        let ph = if ry { th } else { br.h + (y - br.y).abs() + th };
        let _ = write!(
            self.defs,
            "<pattern id=\"{pid}\" patternUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\">{content}</pattern>",
            n(x),
            n(y),
            n(pw),
            n(ph)
        );
        // Without repeat on an axis, the code paints only the band that the tile covers.
        let fx = if rx { br.x } else { x };
        let fy = if ry { br.y } else { y };
        let fw = if rx { br.w } else { tw };
        let fh = if ry { br.h } else { th };
        let _ = write!(self.body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#{pid})\"/>", n(fx), n(fy), n(fw), n(fh));
    }

    /// Defines a gradient that fills `r`. Returns the id of the gradient.
    fn gradient(&mut self, img: &Image, r: Rect) -> String {
        let id = self.id("g");
        let stops_xml = |stops: Vec<(f32, Color)>| -> String {
            stops
                .iter()
                .map(|(o, c)| {
                    let op = if c.a < 1.0 { format!(" stop-opacity=\"{}\"", n(c.a)) } else { String::new() };
                    format!("<stop offset=\"{}\" stop-color=\"{}\"{op}/>", n(o.clamp(0.0, 1.0)), c.hex6())
                })
                .collect()
        };
        match img {
            Image::Linear { angle, stops } => {
                let (sx, sy) = match angle {
                    LinearDir::Angle(a) => {
                        let ar = a.to_radians();
                        (ar.sin(), -ar.cos())
                    }
                    LinearDir::Corner(cx, cy) => {
                        let (dx, dy) = (cx * r.h, cy * r.w);
                        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
                        (dx / l, dy / l)
                    }
                };
                let len = (r.w * sx).abs() + (r.h * sy).abs();
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let (x1, y1) = (cx - sx * len / 2.0, cy - sy * len / 2.0);
                let (x2, y2) = (cx + sx * len / 2.0, cy + sy * len / 2.0);
                let st = stops_xml(resolve_stops(stops, len));
                let _ = write!(
                    self.defs,
                    "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{st}</linearGradient>",
                    n(x1),
                    n(y1),
                    n(x2),
                    n(y2)
                );
            }
            Image::Radial { circle, at, stops } => {
                let cx = r.x + at.0.place(r.w, 0.0);
                let cy = r.y + at.1.place(r.h, 0.0);
                let fx = (cx - r.x).max(r.x + r.w - cx);
                let fy = (cy - r.y).max(r.y + r.h - cy);
                let (rx, ry) = if *circle {
                    let rr = (fx * fx + fy * fy).sqrt();
                    (rr, rr)
                } else {
                    (fx * std::f32::consts::SQRT_2, fy * std::f32::consts::SQRT_2)
                };
                let st = stops_xml(resolve_stops(stops, rx));
                let tr = if (rx - ry).abs() > 0.01 && rx > 0.0 {
                    format!(" gradientTransform=\"translate({} {}) scale(1 {}) translate({} {})\"", n(cx), n(cy), ry / rx, n(-cx), n(-cy))
                } else {
                    String::new()
                };
                let _ = write!(
                    self.defs,
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\"{tr}>{st}</radialGradient>",
                    n(cx),
                    n(cy),
                    n(rx.max(0.01))
                );
            }
            Image::Url(_) => {}
        }
        id
    }

    fn paint_image(&mut self, b: &LBox) {
        let Some(src) = &b.src else { return };
        let Some((href, iw, ih)) = self.image_data(src) else { return };
        let c = b.content_box();
        if c.w <= 0.0 || c.h <= 0.0 || iw <= 0.0 || ih <= 0.0 {
            return;
        }
        let s = &b.style;
        let (dw, dh) = match s.kw(L::ObjectFit) {
            "contain" => {
                let k = (c.w / iw).min(c.h / ih);
                (iw * k, ih * k)
            }
            "cover" => {
                let k = (c.w / iw).max(c.h / ih);
                (iw * k, ih * k)
            }
            "none" => (iw, ih),
            "scale-down" => {
                let k = (c.w / iw).min(c.h / ih).min(1.0);
                (iw * k, ih * k)
            }
            _ => (c.w, c.h),
        };
        let (px, py) = match s.get(L::ObjectPosition) {
            Some(V::Pos(x, y)) => (*x, *y),
            _ => (PosAxis { pct: 50.0, px: 0.0 }, PosAxis { pct: 50.0, px: 0.0 }),
        };
        let x = c.x + px.place(c.w, dw);
        let y = c.y + py.place(c.h, dh);
        let rad = inner_radii(inner_radii(radii(s, b.rect), b.border), b.padding);
        let clip = self.clip_path(&rrect(c, rad), false);
        let _ = write!(
            self.body,
            "<g clip-path=\"url(#{clip})\"><image href=\"{href}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"/></g>",
            n(x),
            n(y),
            n(dw),
            n(dh)
        );
    }

    // ------------------------------------------------------------ text

    fn glyph(&mut self, font_id: cosmic_text::fontdb::ID, gid: u16) -> Option<GlyphShape> {
        if let Some(g) = self.glyphs.get(&(font_id, gid)) {
            return g.clone();
        }
        let data = self.fonts.faces.get(&font_id).map(|f| f.data.clone());
        let shape = data.and_then(|data| {
            let face = ttf_parser::Face::parse(&data, 0).ok()?;
            let upem = face.units_per_em() as f32;
            let g = ttf_parser::GlyphId(gid);
            if face.is_color_glyph(g) {
                let mut p = ColorPainter { face: &face, current: None, layers: Vec::new() };
                if face.paint_color_glyph(g, 0, ttf_parser::RgbaColor::new(0, 0, 0, 255), &mut p).is_some() && !p.layers.is_empty() {
                    return Some(GlyphShape::Layers(p.layers, upem));
                }
            }
            if let Some(img) = face.glyph_raster_image(g, u16::MAX) {
                if img.format == ttf_parser::RasterImageFormat::PNG {
                    let k = upem / img.pixels_per_em as f32;
                    let href = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(img.data));
                    return Some(GlyphShape::Bitmap(href, img.x as f32 * k, img.y as f32 * k, img.width as f32 * k, img.height as f32 * k, upem));
                }
            }
            let mut b = PathBuilder { d: String::new() };
            face.outline_glyph(g, &mut b)?;
            Some(GlyphShape::Outline(b.d, upem))
        });
        self.glyphs.insert((font_id, gid), shape.clone());
        shape
    }

    fn paint_text(&mut self, t: &LText) {
        let Some(shaped) = &t.shaped else { return };
        // Collect the glyphs and their positions:
        // (span, font, glyph, x, baseline y, size, advance).
        struct G {
            span: usize,
            font: cosmic_text::fontdb::ID,
            gid: u16,
            x: f32,
            y: f32,
            size: f32,
            w: f32,
            text: String,
        }
        let mut glyphs = Vec::new();
        let keep_spaces = matches!(t.para.white_space, "pre" | "pre-wrap");
        for run in shaped.buffer.layout_runs() {
            // The code does not draw spaces at the start of a wrapped line.
            // It moves the line to the left by the width of those spaces.
            let mut lead = 0usize;
            let mut shift = 0.0f32;
            if !keep_spaces {
                while lead < run.glyphs.len() && run.text.get(run.glyphs[lead].start..run.glyphs[lead].end).is_some_and(|s| s.chars().all(|c| c == ' ')) {
                    shift += run.glyphs[lead].w;
                    lead += 1;
                }
            }
            let shift = match t.para.align {
                "center" => shift / 2.0,
                "right" | "end" => 0.0,
                _ => shift,
            };
            for g in &run.glyphs[lead..] {
                glyphs.push(G {
                    span: g.metadata,
                    font: g.font_id,
                    gid: g.glyph_id,
                    x: t.rect.x + g.x + g.x_offset - shift,
                    y: t.rect.y + run.line_y + g.y - g.y_offset,
                    size: g.font_size,
                    w: g.w,
                    text: run.text.get(g.start..g.end).unwrap_or("").to_string(),
                });
            }
        }
        let styles = &t.styles;
        let style_of = |i: usize| -> &SpanStyle { styles.get(i).unwrap_or(&t.para.strut) };

        // Draw the shadows first, behind the text.
        let max_span = styles.len().max(1);
        for si in 0..max_span {
            let st = style_of(si).clone();
            for sh in st.shadows.iter().rev() {
                let mut d = String::new();
                for g in glyphs.iter().filter(|g| g.span == si) {
                    let outline = match self.glyph(g.font, g.gid) {
                        Some(GlyphShape::Outline(p, u)) => Some((p, u)),
                        Some(GlyphShape::Layers(l, u)) => Some((l.into_iter().map(|x| x.0).collect::<Vec<_>>().join(""), u)),
                        _ => None,
                    };
                    if let Some((p, upem)) = outline {
                        place_path(&p, g.x + sh.x, g.y + sh.y, g.size / upem, &mut d);
                    }
                }
                if d.is_empty() {
                    continue;
                }
                let color = sh.color.clone().unwrap_or_else(|| st.color.clone());
                let region = grow(t.rect, sh.blur * 3.0 + sh.x.abs() + sh.y.abs() + st.size);
                let filter = if sh.blur > 0.0 { format!(" filter=\"url(#{})\"", self.blur_filter(sh.blur / 2.0, region)) } else { String::new() };
                let _ = write!(self.body, "<path d=\"{d}\" {}{filter}/>", fill(&color));
            }
        }

        let embed = self.opts.fonts == SvgFonts::Embed;
        for si in 0..max_span {
            let st = style_of(si).clone();
            let mine: Vec<&G> = glyphs.iter().filter(|g| g.span == si).collect();
            if mine.is_empty() {
                continue;
            }
            if embed {
                self.embed_text(&mine.iter().map(|g| (g.font, g.x, g.y, g.size, g.text.clone())).collect::<Vec<_>>(), &st);
            } else {
                let mut d = String::new();
                let mut extra = String::new();
                for g in &mine {
                    match self.glyph(g.font, g.gid) {
                        Some(GlyphShape::Outline(p, upem)) => place_path(&p, g.x, g.y, g.size / upem, &mut d),
                        Some(GlyphShape::Layers(layers, upem)) => {
                            for (p, c) in layers {
                                let mut ld = String::new();
                                place_path(&p, g.x, g.y, g.size / upem, &mut ld);
                                let c = c.unwrap_or_else(|| st.color.clone());
                                let _ = write!(extra, "<path d=\"{ld}\" {}/>", fill(&c));
                            }
                        }
                        Some(GlyphShape::Bitmap(href, bx, by, bw, bh, upem)) => {
                            let k = g.size / upem;
                            let _ = write!(
                                extra,
                                "<image href=\"{href}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                                n(g.x + bx * k),
                                n(g.y - (by + bh) * k),
                                n(bw * k),
                                n(bh * k)
                            );
                        }
                        None => {}
                    }
                }
                if !d.is_empty() {
                    let _ = write!(self.body, "<path d=\"{d}\" {}/>", fill(&st.color));
                }
                self.body.push_str(&extra);
            }
            // The decoration lines, for each line of text.
            if let Some(dec) = &st.decoration {
                let font = mine[0].font;
                let data = self.fonts.faces.get(&font).map(|f| f.data.clone());
                let (ul_pos, ul_th, st_pos, st_th, asc, upem) = data
                    .and_then(|d| {
                        let f = ttf_parser::Face::parse(&d, 0).ok()?;
                        let u = f.underline_metrics();
                        let s = f.strikeout_metrics();
                        Some((
                            u.map(|m| m.position as f32).unwrap_or(-100.0),
                            u.map(|m| m.thickness as f32).unwrap_or(50.0),
                            s.map(|m| m.position as f32).unwrap_or(250.0),
                            s.map(|m| m.thickness as f32).unwrap_or(50.0),
                            f.ascender() as f32,
                            f.units_per_em() as f32,
                        ))
                    })
                    .unwrap_or((-100.0, 50.0, 250.0, 50.0, 900.0, 1000.0));
                let color = dec.color.clone().unwrap_or_else(|| st.color.clone());
                // Put the glyphs into line segments, one segment for each baseline.
                let mut lines: Vec<(f32, f32, f32, f32)> = Vec::new(); // y, x0, x1, size
                for g in &mine {
                    match lines.last_mut() {
                        Some(l) if (l.0 - g.y).abs() < 0.5 => l.2 = l.2.max(g.x + g.w),
                        _ => lines.push((g.y, g.x, g.x + g.w, g.size)),
                    }
                }
                for (y, x0, x1, size) in lines {
                    let k = size / upem;
                    let mut put = |pos: f32, th: f32| {
                        let th = (th * k).max(1.0);
                        let ly = y - pos * k;
                        self.decoration_line(x0, x1, ly, th, dec.style, &color);
                    };
                    if dec.underline {
                        put(ul_pos - ul_th / 2.0, ul_th);
                    }
                    if dec.line_through {
                        put(st_pos + st_th / 2.0, st_th);
                    }
                    if dec.overline {
                        put(asc, ul_th);
                    }
                }
            }
        }
    }

    fn decoration_line(&mut self, x0: f32, x1: f32, y: f32, th: f32, style: &str, color: &Color) {
        match style {
            "double" => {
                let _ = write!(self.body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {}/>", n(x0), n(y), n(x1 - x0), n(th), fill(color));
                let _ = write!(self.body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {}/>", n(x0), n(y + th * 2.0), n(x1 - x0), n(th), fill(color));
            }
            "dotted" | "dashed" => {
                let dash = if style == "dashed" { format!("{} {}", n(th * 3.0), n(th * 3.0)) } else { format!("{} {}", n(th), n(th)) };
                let _ = write!(
                    self.body,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {} stroke-width=\"{}\" stroke-dasharray=\"{dash}\"/>",
                    n(x0),
                    n(y + th / 2.0),
                    n(x1),
                    n(y + th / 2.0),
                    stroke(color),
                    n(th)
                );
            }
            "wavy" => {
                let amp = th * 1.5;
                let wl = th * 4.0;
                let mut d = format!("M{} {}", n(x0), n(y + th / 2.0));
                let mut x = x0;
                let mut up = true;
                while x < x1 {
                    let nx = (x + wl / 2.0).min(x1);
                    let cy = if up { y - amp } else { y + amp + th };
                    let _ = write!(d, "Q{} {} {} {}", n((x + nx) / 2.0), n(cy), n(nx), n(y + th / 2.0));
                    x = nx;
                    up = !up;
                }
                let _ = write!(self.body, "<path d=\"{d}\" fill=\"none\" {} stroke-width=\"{}\"/>", stroke(color), n(th));
            }
            _ => {
                let _ = write!(self.body, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {}/>", n(x0), n(y), n(x1 - x0), n(th), fill(color));
            }
        }
    }

    fn embed_text(&mut self, glyphs: &[(cosmic_text::fontdb::ID, f32, f32, f32, String)], st: &SpanStyle) {
        let Some(&(font, _, _, size, _)) = glyphs.first() else { return };
        let fam = match self.fonts.faces.get(&font) {
            Some(f) => {
                let name = format!("emul-{}-{}{}", f.key.family.replace(' ', "-"), f.key.weight, if f.key.italic { "i" } else { "" });
                if !self.embedded.contains_key(&name) {
                    let data = base64::engine::general_purpose::STANDARD.encode(f.data.as_slice());
                    self.embedded.insert(name.clone(), format!("@font-face{{font-family:\"{name}\";src:url(data:font/ttf;base64,{data}) format(\"truetype\");}}"));
                }
                name
            }
            None => return,
        };
        let mut out = format!("<text font-family=\"{fam}\" font-size=\"{}\" {}>", n(size), fill(&st.color));
        for (_, x, y, _, text) in glyphs {
            if text.is_empty() {
                continue;
            }
            let _ = write!(out, "<tspan x=\"{}\" y=\"{}\">{}</tspan>", n(*x), n(*y), esc(text));
        }
        out.push_str("</text>");
        self.body.push_str(&out);
    }
}

fn inset(r: Rect, by: [f32; 4]) -> Rect {
    let [t, ri, b, l] = by;
    Rect { x: r.x + l, y: r.y + t, w: (r.w - l - ri).max(0.0), h: (r.h - t - b).max(0.0) }
}

fn transform_attr(s: &Style, r: Rect) -> Option<String> {
    let (tx, ty) = match s.get(L::Translate) {
        Some(V::Translate(x, y)) => (x.resolve(r.w).unwrap_or(0.0), y.resolve(r.h).unwrap_or(0.0)),
        _ => (0.0, 0.0),
    };
    let rot = match s.get(L::Rotate) {
        Some(V::Rotate(a)) => *a,
        _ => 0.0,
    };
    let (sx, sy) = match s.get(L::Scale) {
        Some(V::Scale(a, b)) => (*a, *b),
        _ => (1.0, 1.0),
    };
    if tx == 0.0 && ty == 0.0 && rot == 0.0 && sx == 1.0 && sy == 1.0 {
        return None;
    }
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    Some(format!(
        "translate({} {}) rotate({}) scale({} {}) translate({} {})",
        n(cx + tx),
        n(cy + ty),
        n(rot),
        n(sx),
        n(sy),
        n(-cx),
        n(-cy)
    ))
}

/// The paint layer of a box. It is `None` for normal flow. It is the z-index for a
/// positioned box and for a box with its own stack context.
fn layer_of(b: &LBox) -> Option<i32> {
    let s = &b.style;
    if s.is_positioned() {
        return Some(s.int(L::ZIndex).unwrap_or(0));
    }
    if s.num(L::Opacity) < 1.0 || transform_attr(s, b.rect).is_some() {
        return Some(0);
    }
    None
}

fn is_atomic(b: &LBox) -> bool {
    layer_of(b).is_some() || b.style.kw(L::Overflow) == "hidden"
}

/// Collects the boxes that the layer passes paint. It does not go into a box that the
/// code paints as a unit.
fn collect_layers<'a>(b: &'a LBox, out: &mut Vec<(i32, usize, &'a LBox)>, order: &mut usize) {
    *order += 1;
    if let Some(z) = layer_of(b) {
        out.push((z, *order, b));
        return;
    }
    if is_atomic(b) {
        return;
    }
    for c in &b.children {
        if let LItem::Box(cb) = c {
            collect_layers(cb, out, order);
        }
    }
}

pub fn render(laid: &Laid, fonts: &mut Fonts, opts: &SvgOptions) -> String {
    let mut w = Writer {
        defs: String::new(),
        body: String::new(),
        next_id: 0,
        fonts,
        opts,
        images: HashMap::new(),
        glyphs: HashMap::new(),
        embedded: BTreeMap::new(),
    };
    if let Some(bg) = &opts.background {
        let _ = write!(w.body, "<rect width=\"{}\" height=\"{}\" {}/>", n(laid.width), n(laid.height), fill(bg));
    }
    w.paint_root(laid);
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        n(laid.width),
        n(laid.height),
        n(laid.width),
        n(laid.height)
    );
    if !w.embedded.is_empty() {
        out.push_str("<style>");
        for v in w.embedded.values() {
            out.push_str(v);
        }
        out.push_str("</style>");
    }
    if !w.defs.is_empty() {
        let _ = write!(out, "<defs>{}</defs>", w.defs);
    }
    out.push_str(&w.body);
    out.push_str("</svg>\n");
    out
}
