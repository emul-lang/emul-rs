//! Text shaping and line breaking, using cosmic-text with the project's Google Fonts.

use crate::fonts::FontSet;
use crate::props::{Decoration, LineHeight, Style, L};
use crate::values::{Color, Shadow};
use cosmic_text::{Attrs, Buffer, Ellipsize, EllipsizeHeightLimit, Family, FontSystem, Metrics, Shaping, Wrap};
use std::collections::HashMap;
use std::sync::Arc;

/// Text styled by one component: a run of text and how it looks.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

#[derive(Clone, Debug)]
pub struct SpanStyle {
    pub families: Vec<String>,
    pub weight: u16,
    pub italic: bool,
    pub size: f32,
    pub color: Color,
    pub letter_spacing: f32,
    pub decoration: Option<Decoration>,
    pub shadows: Vec<Shadow>,
    pub line_height: LineHeight,
}

impl SpanStyle {
    pub fn of(style: &Style) -> SpanStyle {
        SpanStyle {
            families: style.families(),
            weight: style.weight(),
            italic: style.italic(),
            size: style.font_size(),
            color: style.color(),
            letter_spacing: match style.len(L::LetterSpacing) {
                crate::values::Len::Px(v) => v,
                _ => 0.0,
            },
            decoration: style.decoration(),
            shadows: style.shadows(L::TextShadow),
            line_height: style.line_height(),
        }
    }
}

/// A block of text laid out together: the inline content of one element.
#[derive(Clone, Debug)]
pub struct Paragraph {
    pub spans: Vec<Span>,
    /// The containing element's own text style, which sets the minimum line height.
    pub strut: SpanStyle,
    pub align: &'static str,
    pub white_space: &'static str,
    pub clamp: Option<usize>,
    /// An ellipsis on a single unwrapped line, from textOverflow.
    pub ellipsis: bool,
}

pub struct Fonts {
    pub system: FontSystem,
    /// Google Fonts family name to the family name inside the font files.
    pub names: HashMap<String, String>,
    /// Line metrics per family: ascent + descent + gap, as a fraction of the size.
    metrics: HashMap<String, f32>,
    /// Each loaded font, by its id in the font database.
    pub faces: HashMap<cosmic_text::fontdb::ID, crate::fonts::LoadedFace>,
}

impl Fonts {
    pub fn new(set: &FontSet) -> Fonts {
        let mut db = cosmic_text::fontdb::Database::new();
        let mut names = HashMap::new();
        let mut metrics = HashMap::new();
        let mut faces = HashMap::new();
        for face in &set.faces {
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = face.data.clone();
            let ids = db.load_font_source(cosmic_text::fontdb::Source::Binary(data));
            for id in ids {
                faces.insert(id, face.clone());
                if let Some(info) = db.face(id) {
                    if let Some((name, _)) = info.families.first() {
                        names.entry(face.key.family.clone()).or_insert_with(|| name.clone());
                    }
                }
            }
            if let Ok(f) = ttf_parser::Face::parse(&face.data, 0) {
                let upem = f.units_per_em() as f32;
                let (asc, desc, gap) = if f.tables().os2.is_some_and(|o| o.use_typographic_metrics()) {
                    (f.typographic_ascender().unwrap_or(f.ascender()), f.typographic_descender().unwrap_or(f.descender()), f.typographic_line_gap().unwrap_or(f.line_gap()))
                } else {
                    (f.ascender(), f.descender(), f.line_gap())
                };
                metrics.insert(face.key.family.clone(), (asc as f32 - desc as f32 + gap as f32) / upem);
            }
        }
        db.set_sans_serif_family("Inter");
        let system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
        Fonts { system, names, metrics, faces }
    }

    fn db_family(&self, family: &str) -> String {
        self.names.get(family).cloned().unwrap_or_else(|| family.to_string())
    }

    /// The used line height in px for a style, following CSS: `normal` uses the font's
    /// own line spacing.
    pub fn line_height(&self, s: &SpanStyle) -> f32 {
        match s.line_height {
            LineHeight::Normal => {
                let fam = s.families.first().map(String::as_str).unwrap_or("Inter");
                self.metrics.get(fam).copied().unwrap_or(1.2) * s.size
            }
            LineHeight::Number(n) => n * s.size,
            LineHeight::Px(p) => p,
            LineHeight::Pct(p) => p / 100.0 * s.size,
        }
    }
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' || c == '\n' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

pub fn transform(s: &str, t: &str) -> String {
    match t {
        "uppercase" => s.to_uppercase(),
        "lowercase" => s.to_lowercase(),
        "capitalize" => {
            let mut out = String::new();
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    if c.is_whitespace() {
                        start = true;
                    }
                    out.push(c);
                }
            }
            out
        }
        _ => s.to_string(),
    }
}

/// Applies whitespace processing across the spans of a paragraph: adjacent spans are
/// joined before collapsing, and spaces at the start and end are trimmed.
pub fn process_whitespace(spans: &[Span], white_space: &str) -> Vec<Span> {
    let keep = matches!(white_space, "pre" | "pre-wrap");
    if keep {
        return spans.to_vec();
    }
    let mut out: Vec<Span> = Vec::new();
    let mut prev_space = true; // trims leading spaces
    for sp in spans {
        let mut t = String::new();
        for c in collapse(&sp.text).chars() {
            if c == ' ' {
                if prev_space {
                    continue;
                }
                prev_space = true;
            } else {
                prev_space = false;
            }
            t.push(c);
        }
        out.push(Span { text: t, style: sp.style.clone() });
    }
    // Trim trailing spaces.
    for sp in out.iter_mut().rev() {
        let trimmed = sp.text.trim_end_matches(' ').to_string();
        let was_empty = trimmed.is_empty();
        sp.text = trimmed;
        if !was_empty {
            break;
        }
    }
    out.retain(|s| !s.text.is_empty());
    out
}

pub struct Shaped {
    pub buffer: Buffer,
    pub width: f32,
    pub height: f32,
}

/// Shapes a paragraph at a given width; `None` means unlimited (max-content).
pub fn shape(fonts: &mut Fonts, para: &Paragraph, width: Option<f32>) -> Shaped {
    let strut_lh = fonts.line_height(&para.strut);
    let metrics = Metrics::new(para.strut.size, strut_lh.max(0.01));
    let mut buffer = Buffer::new(&mut fonts.system, metrics);
    let wrap = match para.white_space {
        "nowrap" | "pre" => Wrap::None,
        _ => Wrap::Word,
    };
    buffer.set_wrap(wrap);
    let ellipsize = if let Some(n) = para.clamp {
        Ellipsize::End(EllipsizeHeightLimit::Lines(n))
    } else if para.ellipsis {
        Ellipsize::End(EllipsizeHeightLimit::Lines(1))
    } else {
        Ellipsize::None
    };
    buffer.set_ellipsize(ellipsize);
    buffer.set_size(width, None);
    let spans = process_whitespace(&para.spans, para.white_space);
    let families: Vec<String> = spans.iter().map(|s| fonts.db_family(s.style.families.first().map(String::as_str).unwrap_or("Inter"))).collect();
    let line_heights: Vec<f32> = spans.iter().map(|s| fonts.line_height(&s.style).max(strut_lh)).collect();
    let rich: Vec<(&str, Attrs)> = spans
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let st = &s.style;
            let attrs = Attrs::new()
                .family(Family::Name(&families[i]))
                .weight(cosmic_text::Weight(st.weight))
                .style(if st.italic { cosmic_text::Style::Italic } else { cosmic_text::Style::Normal })
                .metrics(Metrics::new(st.size, line_heights[i]))
                .letter_spacing(if st.size > 0.0 { st.letter_spacing / st.size } else { 0.0 })
                .metadata(i);
            (s.text.as_str(), attrs)
        })
        .collect();
    let default_attrs = Attrs::new().family(Family::Name(families.first().map(String::as_str).unwrap_or("Inter")));
    let align = match para.align {
        "center" => Some(cosmic_text::Align::Center),
        "right" | "end" => Some(cosmic_text::Align::Right),
        "justify" => Some(cosmic_text::Align::Justified),
        _ => Some(cosmic_text::Align::Left),
    };
    if rich.is_empty() {
        buffer.set_text("", &default_attrs, Shaping::Advanced, align);
    } else {
        buffer.set_rich_text(rich, &default_attrs, Shaping::Advanced, align);
    }
    buffer.shape_until_scroll(&mut fonts.system, false);
    let mut w: f32 = 0.0;
    let mut h: f32 = 0.0;
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    if spans.is_empty() {
        h = 0.0;
    }
    Shaped { buffer, width: w, height: h }
}

/// The styles of the spans after whitespace processing, in metadata order.
pub fn span_styles(para: &Paragraph) -> Vec<SpanStyle> {
    process_whitespace(&para.spans, para.white_space).into_iter().map(|s| s.style).collect()
}
