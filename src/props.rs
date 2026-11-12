//! All Emul properties. This module checks each value and sets the longhands of each
//! property. It also makes the resolved style. The layout, the paint step and the HTML
//! output read that style.

use crate::values::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Longhands: the individual settings. Each property divides into longhands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum L {
    Display,
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    GridTemplateColumns,
    GridTemplateRows,
    GridTemplateAreas,
    GridAutoRows,
    GridColumn,
    GridRow,
    GridArea,
    AlignItems,
    AlignSelf,
    AlignContent,
    JustifyContent,
    JustifyItems,
    JustifySelf,
    RowGap,
    ColumnGap,
    Position,
    Top,
    Right,
    Bottom,
    Left,
    ZIndex,
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    AspectRatio,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    RadiusTopLeft,
    RadiusTopRight,
    RadiusBottomRight,
    RadiusBottomLeft,
    Overflow,
    FontFamily,
    FontSize,
    FontWeight,
    FontStyle,
    LineHeight,
    WhiteSpace,
    TextOverflow,
    LineClamp,
    TextAlign,
    LetterSpacing,
    TextDecoration,
    Color,
    TextTransform,
    TextShadow,
    BackgroundColor,
    BackgroundImage,
    BackgroundSize,
    BackgroundPosition,
    BackgroundRepeat,
    Opacity,
    BoxShadow,
    Translate,
    Rotate,
    Scale,
    ObjectFit,
    ObjectPosition,
    HtmlTag,
}

impl L {
    pub fn css(self) -> &'static str {
        use L::*;
        match self {
            Display => "display",
            FlexDirection => "flex-direction",
            FlexWrap => "flex-wrap",
            FlexGrow => "flex-grow",
            FlexShrink => "flex-shrink",
            FlexBasis => "flex-basis",
            GridTemplateColumns => "grid-template-columns",
            GridTemplateRows => "grid-template-rows",
            GridTemplateAreas => "grid-template-areas",
            GridAutoRows => "grid-auto-rows",
            GridColumn => "grid-column",
            GridRow => "grid-row",
            GridArea => "grid-area",
            AlignItems => "align-items",
            AlignSelf => "align-self",
            AlignContent => "align-content",
            JustifyContent => "justify-content",
            JustifyItems => "justify-items",
            JustifySelf => "justify-self",
            RowGap => "row-gap",
            ColumnGap => "column-gap",
            Position => "position",
            Top => "top",
            Right => "right",
            Bottom => "bottom",
            Left => "left",
            ZIndex => "z-index",
            Width => "width",
            Height => "height",
            MinWidth => "min-width",
            MinHeight => "min-height",
            MaxWidth => "max-width",
            MaxHeight => "max-height",
            AspectRatio => "aspect-ratio",
            MarginTop => "margin-top",
            MarginRight => "margin-right",
            MarginBottom => "margin-bottom",
            MarginLeft => "margin-left",
            PaddingTop => "padding-top",
            PaddingRight => "padding-right",
            PaddingBottom => "padding-bottom",
            PaddingLeft => "padding-left",
            BorderTopWidth => "border-top-width",
            BorderRightWidth => "border-right-width",
            BorderBottomWidth => "border-bottom-width",
            BorderLeftWidth => "border-left-width",
            BorderTopStyle => "border-top-style",
            BorderRightStyle => "border-right-style",
            BorderBottomStyle => "border-bottom-style",
            BorderLeftStyle => "border-left-style",
            BorderTopColor => "border-top-color",
            BorderRightColor => "border-right-color",
            BorderBottomColor => "border-bottom-color",
            BorderLeftColor => "border-left-color",
            RadiusTopLeft => "border-top-left-radius",
            RadiusTopRight => "border-top-right-radius",
            RadiusBottomRight => "border-bottom-right-radius",
            RadiusBottomLeft => "border-bottom-left-radius",
            Overflow => "overflow",
            FontFamily => "font-family",
            FontSize => "font-size",
            FontWeight => "font-weight",
            FontStyle => "font-style",
            LineHeight => "line-height",
            WhiteSpace => "white-space",
            TextOverflow => "text-overflow",
            LineClamp => "line-clamp",
            TextAlign => "text-align",
            LetterSpacing => "letter-spacing",
            TextDecoration => "text-decoration",
            Color => "color",
            TextTransform => "text-transform",
            TextShadow => "text-shadow",
            BackgroundColor => "background-color",
            BackgroundImage => "background-image",
            BackgroundSize => "background-size",
            BackgroundPosition => "background-position",
            BackgroundRepeat => "background-repeat",
            Opacity => "opacity",
            BoxShadow => "box-shadow",
            Translate => "translate",
            Rotate => "rotate",
            Scale => "scale",
            ObjectFit => "object-fit",
            ObjectPosition => "object-position",
            HtmlTag => "",
        }
    }

    /// Box properties, which have no effect on inline components.
    pub fn is_box(self) -> bool {
        use L::*;
        matches!(
            self,
            Width | Height | MinWidth | MinHeight | MaxWidth | MaxHeight | AspectRatio
                | MarginTop | MarginRight | MarginBottom | MarginLeft
                | PaddingTop | PaddingRight | PaddingBottom | PaddingLeft
                | BorderTopWidth | BorderRightWidth | BorderBottomWidth | BorderLeftWidth
                | BorderTopStyle | BorderRightStyle | BorderBottomStyle | BorderLeftStyle
                | BorderTopColor | BorderRightColor | BorderBottomColor | BorderLeftColor
                | RadiusTopLeft | RadiusTopRight | RadiusBottomRight | RadiusBottomLeft
                | BackgroundColor | BackgroundImage | BackgroundSize | BackgroundPosition | BackgroundRepeat
                | BoxShadow
        )
    }
}

pub const SIDES_MARGIN: [L; 4] = [L::MarginTop, L::MarginRight, L::MarginBottom, L::MarginLeft];
pub const SIDES_PADDING: [L; 4] = [L::PaddingTop, L::PaddingRight, L::PaddingBottom, L::PaddingLeft];
pub const SIDES_INSET: [L; 4] = [L::Top, L::Right, L::Bottom, L::Left];
pub const BORDER_WIDTH: [L; 4] = [L::BorderTopWidth, L::BorderRightWidth, L::BorderBottomWidth, L::BorderLeftWidth];
pub const BORDER_STYLE: [L; 4] = [L::BorderTopStyle, L::BorderRightStyle, L::BorderBottomStyle, L::BorderLeftStyle];
pub const BORDER_COLOR: [L; 4] = [L::BorderTopColor, L::BorderRightColor, L::BorderBottomColor, L::BorderLeftColor];
pub const RADII: [L; 4] = [L::RadiusTopLeft, L::RadiusTopRight, L::RadiusBottomRight, L::RadiusBottomLeft];

#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    Px(f32),
    Pct(f32),
    Fr(f32),
    Auto,
    MinMax(Box<Track>, Box<Track>),
}

impl Track {
    pub fn css(&self) -> String {
        match self {
            Track::Px(v) => format!("{}px", fmt_num(*v as f64)),
            Track::Pct(v) => format!("{}%", fmt_num(*v as f64)),
            Track::Fr(v) => format!("{}fr", fmt_num(*v as f64)),
            Track::Auto => "auto".into(),
            Track::MinMax(a, b) => format!("minmax({}, {})", a.css(), b.css()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RepeatCount {
    Count(u16),
    AutoFill,
    AutoFit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TemplateItem {
    Track(Track),
    Repeat(RepeatCount, Vec<Track>),
    Names(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridLine {
    Auto,
    Line(i16),
    Named(String),
    Span(u16),
}

impl GridLine {
    pub fn css(&self) -> String {
        match self {
            GridLine::Auto => "auto".into(),
            GridLine::Line(n) => n.to_string(),
            GridLine::Named(n) => n.clone(),
            GridLine::Span(n) => format!("span {n}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub style: &'static str,
    pub color: Option<crate::values::Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BgSize {
    Auto,
    Cover,
    Contain,
    Explicit(Len, Len),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Px(f32),
    Pct(f32),
}

/// A typed longhand value.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Kw(&'static str),
    Num(f32),
    /// `None` is `auto` or `none`.
    Int(Option<i32>),
    Len(Len),
    Color(crate::values::Color),
    /// `None` is `none`, for backgroundColor.
    OptColor(Option<crate::values::Color>),
    Families(Vec<String>),
    Weight(u16),
    LineHeight(LineHeight),
    Template(Option<Vec<TemplateItem>>),
    AutoTracks(Vec<Track>),
    Areas(Option<Vec<Vec<String>>>),
    Lines(GridLine, GridLine),
    Area(Option<String>, [GridLine; 4]),
    Ratio(Option<f32>),
    Decoration(Option<Decoration>),
    Shadows(Vec<Shadow>),
    Images(Vec<Image>),
    BgSize(BgSize),
    Pos(PosAxis, PosAxis),
    Translate(Len, Len),
    Rotate(f32),
    Scale(f32, f32),
    Tag(String),
}

pub struct PErr {
    pub code: &'static str,
    pub msg: String,
}

impl From<crate::values::TokErr> for PErr {
    fn from(e: crate::values::TokErr) -> Self {
        PErr { code: e.code, msg: e.msg }
    }
}

impl From<String> for PErr {
    fn from(msg: String) -> Self {
        PErr { code: "invalid-value", msg }
    }
}

/// Context to parse values that refer to files.
pub struct Ctx<'a> {
    pub dir: &'a Path,
    pub root: &'a Path,
}

/// How specific a property is: shorthands of shorthands are 0, longhands are 2.
pub fn level(name: &str) -> Option<u8> {
    Some(match name {
        "border" | "margin" | "padding" | "inset" | "gap" | "flex" | "borderRadius" => 0,
        "borderWidth" | "borderStyle" | "borderColor" | "borderTop" | "borderRight" | "borderBottom" | "borderLeft" => 1,
        _ if longhand_of(name).is_some() => 2,
        _ => return None,
    })
}

/// The longhand that an ordinary (non-shorthand) property sets.
pub fn longhand_of(name: &str) -> Option<L> {
    use L::*;
    Some(match name {
        "display" => Display,
        "flexDirection" => FlexDirection,
        "flexWrap" => FlexWrap,
        "flexGrow" => FlexGrow,
        "flexShrink" => FlexShrink,
        "flexBasis" => FlexBasis,
        "gridTemplateColumns" => GridTemplateColumns,
        "gridTemplateRows" => GridTemplateRows,
        "gridTemplateAreas" => GridTemplateAreas,
        "gridAutoRows" => GridAutoRows,
        "gridColumn" => GridColumn,
        "gridRow" => GridRow,
        "gridArea" => GridArea,
        "alignItems" => AlignItems,
        "alignSelf" => AlignSelf,
        "alignContent" => AlignContent,
        "justifyContent" => JustifyContent,
        "justifyItems" => JustifyItems,
        "justifySelf" => JustifySelf,
        "rowGap" => RowGap,
        "columnGap" => ColumnGap,
        "position" => Position,
        "top" => Top,
        "right" => Right,
        "bottom" => Bottom,
        "left" => Left,
        "zIndex" => ZIndex,
        "width" => Width,
        "height" => Height,
        "minWidth" => MinWidth,
        "minHeight" => MinHeight,
        "maxWidth" => MaxWidth,
        "maxHeight" => MaxHeight,
        "aspectRatio" => AspectRatio,
        "marginTop" => MarginTop,
        "marginRight" => MarginRight,
        "marginBottom" => MarginBottom,
        "marginLeft" => MarginLeft,
        "paddingTop" => PaddingTop,
        "paddingRight" => PaddingRight,
        "paddingBottom" => PaddingBottom,
        "paddingLeft" => PaddingLeft,
        "overflow" => Overflow,
        "fontFamily" => FontFamily,
        "fontSize" => FontSize,
        "fontWeight" => FontWeight,
        "fontStyle" => FontStyle,
        "lineHeight" => LineHeight,
        "whiteSpace" => WhiteSpace,
        "textOverflow" => TextOverflow,
        "lineClamp" => LineClamp,
        "textAlign" => TextAlign,
        "letterSpacing" => LetterSpacing,
        "textDecoration" => TextDecoration,
        "color" => Color,
        "textTransform" => TextTransform,
        "textShadow" => TextShadow,
        "backgroundColor" => BackgroundColor,
        "backgroundImage" => BackgroundImage,
        "backgroundSize" => BackgroundSize,
        "backgroundPosition" => BackgroundPosition,
        "backgroundRepeat" => BackgroundRepeat,
        "opacity" => Opacity,
        "boxShadow" => BoxShadow,
        "translate" => Translate,
        "rotate" => Rotate,
        "scale" => Scale,
        "objectFit" => ObjectFit,
        "objectPosition" => ObjectPosition,
        "htmlTag" => HtmlTag,
        _ => return None,
    })
}

pub fn is_property(name: &str) -> bool {
    level(name).is_some()
}

pub const ALL_PROPERTY_NAMES: &[&str] = &[
    "display", "flex", "flexDirection", "flexWrap", "flexGrow", "flexShrink", "flexBasis",
    "gridTemplateColumns", "gridTemplateRows", "gridTemplateAreas", "gridAutoRows", "gridColumn",
    "gridRow", "gridArea", "alignItems", "alignSelf", "alignContent", "justifyContent",
    "justifyItems", "justifySelf", "gap", "rowGap", "columnGap", "position", "top", "right",
    "bottom", "left", "inset", "zIndex", "width", "height", "minWidth", "minHeight", "maxWidth",
    "maxHeight", "aspectRatio", "margin", "marginTop", "marginRight", "marginBottom", "marginLeft",
    "padding", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "border",
    "borderWidth", "borderStyle", "borderColor", "borderTop", "borderRight", "borderBottom",
    "borderLeft", "borderRadius", "overflow", "fontFamily", "fontSize", "fontWeight", "fontStyle",
    "lineHeight", "whiteSpace", "textOverflow", "lineClamp", "textAlign", "letterSpacing",
    "textDecoration", "color", "textTransform", "textShadow", "backgroundColor", "backgroundImage",
    "backgroundSize", "backgroundPosition", "backgroundRepeat", "opacity", "boxShadow",
    "translate", "rotate", "scale", "objectFit", "objectPosition", "htmlTag",
];

fn kw(toks: &[Tok], allowed: &[&'static str], what: &str) -> Result<&'static str, String> {
    match toks {
        [Tok::Ident(k)] => allowed
            .iter()
            .find(|a| **a == k.as_str())
            .copied()
            .ok_or_else(|| format!("`{k}` is not a {what} value; expected one of: {}", allowed.join(", "))),
        _ => Err(format!("{what} takes one of: {}", allowed.join(", "))),
    }
}

fn one(toks: &[Tok]) -> Result<&Tok, String> {
    match toks {
        [t] => Ok(t),
        [] => Err("a value is needed".into()),
        _ => Err("this property takes a single value".into()),
    }
}

fn number(toks: &[Tok], min: f32, what: &str) -> Result<f32, String> {
    match one(toks)? {
        Tok::Num(v, u) if u.is_empty() && *v as f32 >= min => Ok(*v as f32),
        _ => Err(format!("{what} is a number of at least {}", fmt_num(min as f64))),
    }
}

fn lens(toks: &[Tok], r: &LenRules, max: usize) -> Result<Vec<Len>, String> {
    if toks.is_empty() || toks.len() > max {
        return Err(format!("this property takes from 1 to {max} values"));
    }
    toks.iter().map(|t| parse_len(t, r)).collect()
}

/// Expands 1 to 4 values onto the four sides, as CSS does.
fn four<T: Clone>(v: &[T]) -> [T; 4] {
    match v.len() {
        1 => [v[0].clone(), v[0].clone(), v[0].clone(), v[0].clone()],
        2 => [v[0].clone(), v[1].clone(), v[0].clone(), v[1].clone()],
        3 => [v[0].clone(), v[1].clone(), v[2].clone(), v[1].clone()],
        _ => [v[0].clone(), v[1].clone(), v[2].clone(), v[3].clone()],
    }
}

pub const BORDER_STYLES: &[&str] = &["none", "solid", "dashed", "dotted", "double"];

fn border_parts(toks: &[Tok]) -> Result<(Len, &'static str, crate::values::Color), String> {
    let mut width = Len::Px(0.0);
    let mut style = "none";
    let mut color = crate::values::Color::black();
    let mut got_width = false;
    if let [Tok::Ident(k)] = toks {
        if k == "none" {
            return Ok((Len::Px(0.0), "none", color));
        }
    }
    for t in toks {
        match t {
            Tok::Num(..) => {
                width = parse_len(t, &LenRules { auto: false, none: false, pct: false, negative: false })?;
                got_width = true;
            }
            Tok::Ident(k) if BORDER_STYLES.contains(&k.as_str()) => {
                style = BORDER_STYLES.iter().find(|s| **s == k.as_str()).unwrap();
            }
            _ => color = parse_color(t)?,
        }
    }
    // A border with a style but no width is 1px, not the CSS `medium`,
    // because Emul has no width keywords.
    if !got_width && style != "none" {
        width = Len::Px(1.0);
    }
    Ok((width, style, color))
}

fn tracks(toks: &[Tok]) -> Result<Vec<TemplateItem>, String> {
    let mut out = Vec::new();
    for t in toks {
        match t {
            Tok::Names(n) => out.push(TemplateItem::Names(n.clone())),
            Tok::Func(f, args) if f == "repeat" => {
                let parts = split_commas(args);
                if parts.len() != 2 || parts[0].len() != 1 {
                    return Err("`repeat()` takes a count and a list of track sizes".into());
                }
                let count = match &parts[0][0] {
                    Tok::Num(v, u) if u.is_empty() && *v >= 1.0 => RepeatCount::Count(*v as u16),
                    Tok::Ident(k) if k == "auto-fill" => RepeatCount::AutoFill,
                    Tok::Ident(k) if k == "auto-fit" => RepeatCount::AutoFit,
                    _ => return Err("a repeat count is a whole number, auto-fill or auto-fit".into()),
                };
                let inner: Result<Vec<Track>, String> = parts[1].iter().map(track).collect();
                out.push(TemplateItem::Repeat(count, inner?));
            }
            _ => out.push(TemplateItem::Track(track(t)?)),
        }
    }
    if out.is_empty() {
        return Err("a track list needs at least one track".into());
    }
    Ok(out)
}

fn track(t: &Tok) -> Result<Track, String> {
    match t {
        Tok::Ident(k) if k == "auto" => Ok(Track::Auto),
        Tok::Num(v, u) if u == "fr" && *v >= 0.0 => Ok(Track::Fr(*v as f32)),
        Tok::Num(v, u) if u == "px" && *v >= 0.0 => Ok(Track::Px(*v as f32)),
        Tok::Num(v, u) if u == "%" && *v >= 0.0 => Ok(Track::Pct(*v as f32)),
        Tok::Num(v, u) if u.is_empty() && *v == 0.0 => Ok(Track::Px(0.0)),
        Tok::Func(f, args) if f == "minmax" => {
            let parts = split_commas(args);
            if parts.len() != 2 || parts[0].len() != 1 || parts[1].len() != 1 {
                return Err("`minmax()` takes a minimum and a maximum".into());
            }
            let a = track(&parts[0][0])?;
            let b = track(&parts[1][0])?;
            if matches!(a, Track::Fr(_)) {
                return Err("the minimum of `minmax()` can't be in fr".into());
            }
            Ok(Track::MinMax(Box::new(a), Box::new(b)))
        }
        _ => Err(format!("`{}` is not a track size; use px, %, fr, auto or minmax()", tokens_css(std::slice::from_ref(t)))),
    }
}

fn grid_line(toks: &[Tok]) -> Result<GridLine, String> {
    match toks {
        [Tok::Ident(k)] if k == "auto" => Ok(GridLine::Auto),
        [Tok::Num(v, u)] if u.is_empty() && v.fract() == 0.0 && *v != 0.0 => Ok(GridLine::Line(*v as i16)),
        [Tok::Ident(k), Tok::Num(v, u)] if k == "span" && u.is_empty() && *v >= 1.0 => Ok(GridLine::Span(*v as u16)),
        [Tok::Ident(k)] if k != "span" => Ok(GridLine::Named(k.clone())),
        _ => Err("a grid line is auto, a non-zero line number, a line name, or `span` and a count".into()),
    }
}

fn split_slash(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut out = vec![Vec::new()];
    for t in toks {
        if *t == Tok::Slash {
            out.push(Vec::new());
        } else {
            out.last_mut().unwrap().push(t.clone());
        }
    }
    out
}

fn image_file(path: &str, ctx: &Ctx) -> Result<PathBuf, PErr> {
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("//") {
        return Err(PErr { code: "missing-image", msg: format!("`{path}` is a remote URL; images must be local files") });
    }
    let p = if let Some(abs) = path.strip_prefix('/') { ctx.root.join(abs) } else { ctx.dir.join(path) };
    let p = normalize(&p);
    if !p.is_file() {
        return Err(PErr { code: "missing-image", msg: format!("the image `{path}` does not exist") });
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["png", "jpg", "jpeg", "webp", "svg"].contains(&ext.as_str()) {
        return Err(PErr { code: "unsupported-image", msg: format!("`{path}` is not a PNG, JPEG, WebP or SVG file") });
    }
    Ok(p)
}

pub fn resolve_image_path(path: &str, ctx: &Ctx) -> Result<PathBuf, PErr> {
    image_file(path, ctx)
}

/// Removes `.` and `..` components. It does not access the file system.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub const VOID_TAGS: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

/// Parses one property declaration into the longhands it sets.
pub fn parse_property(name: &str, value: &str, ctx: &Ctx) -> Result<Vec<(L, V)>, PErr> {
    use L::*;
    let toks = tokenize(value)?;
    let pos_any = LenRules { auto: true, none: false, pct: true, negative: true };
    let nonneg_pct = LenRules { auto: false, none: false, pct: true, negative: false };
    let size_auto = LenRules { auto: true, none: false, pct: true, negative: false };
    let size_none = LenRules { auto: false, none: true, pct: true, negative: false };
    let px_only = LenRules { auto: false, none: false, pct: false, negative: false };
    let single = |l: L, v: V| Ok(vec![(l, v)]);
    match name {
        "display" => single(Display, V::Kw(kw(&toks, &["block", "inline", "flex", "grid", "none"], "display")?)),
        "flexDirection" => single(FlexDirection, V::Kw(kw(&toks, &["row", "row-reverse", "column", "column-reverse"], "flexDirection")?)),
        "flexWrap" => single(FlexWrap, V::Kw(kw(&toks, &["nowrap", "wrap", "wrap-reverse"], "flexWrap")?)),
        "flexGrow" => single(FlexGrow, V::Num(number(&toks, 0.0, "flexGrow")?)),
        "flexShrink" => single(FlexShrink, V::Num(number(&toks, 0.0, "flexShrink")?)),
        "flexBasis" => {
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "content" {
                    return single(FlexBasis, V::Kw("content"));
                }
            }
            single(FlexBasis, V::Len(parse_len(one(&toks)?, &size_auto)?))
        }
        "flex" => {
            let (g, s, b) = match toks.as_slice() {
                [Tok::Ident(k)] if k == "none" => (0.0, 0.0, Len::Auto),
                [Tok::Ident(k)] if k == "auto" => (1.0, 1.0, Len::Auto),
                [Tok::Num(g, u)] if u.is_empty() => (*g as f32, 1.0, Len::Pct(0.0)),
                [Tok::Num(g, u), Tok::Num(s, u2)] if u.is_empty() && u2.is_empty() => (*g as f32, *s as f32, Len::Pct(0.0)),
                [Tok::Num(g, u), b] if u.is_empty() => (*g as f32, 1.0, parse_len(b, &size_auto)?),
                [Tok::Num(g, u), Tok::Num(s, u2), b] if u.is_empty() && u2.is_empty() => (*g as f32, *s as f32, parse_len(b, &size_auto)?),
                [b] => (1.0, 1.0, parse_len(b, &size_auto)?),
                _ => return Err("flex is none, auto, or a grow factor with an optional shrink factor and basis".to_string().into()),
            };
            if g < 0.0 || s < 0.0 {
                return Err("flex factors can't be negative".to_string().into());
            }
            Ok(vec![(FlexGrow, V::Num(g)), (FlexShrink, V::Num(s)), (FlexBasis, V::Len(b))])
        }
        "gridTemplateColumns" | "gridTemplateRows" => {
            let l = if name == "gridTemplateColumns" { GridTemplateColumns } else { GridTemplateRows };
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "none" {
                    return single(l, V::Template(None));
                }
            }
            single(l, V::Template(Some(tracks(&toks)?)))
        }
        "gridAutoRows" => {
            let t: Result<Vec<Track>, String> = toks.iter().map(track).collect();
            let t = t?;
            if t.is_empty() {
                return Err("gridAutoRows needs at least one track size".to_string().into());
            }
            single(GridAutoRows, V::AutoTracks(t))
        }
        "gridTemplateAreas" => {
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "none" {
                    return single(GridTemplateAreas, V::Areas(None));
                }
            }
            let mut rows = Vec::new();
            for t in &toks {
                match t {
                    Tok::Str(s) => rows.push(s.split_whitespace().map(String::from).collect::<Vec<_>>()),
                    _ => return Err("gridTemplateAreas is none, or one quoted string per row".to_string().into()),
                }
            }
            let cols = rows.first().map(|r| r.len()).unwrap_or(0);
            if rows.is_empty() || cols == 0 || rows.iter().any(|r| r.len() != cols) {
                return Err("every row of gridTemplateAreas must name the same number of cells".to_string().into());
            }
            single(GridTemplateAreas, V::Areas(Some(rows)))
        }
        "gridColumn" | "gridRow" => {
            let l = if name == "gridColumn" { GridColumn } else { GridRow };
            let parts = split_slash(&toks);
            let (a, b) = match parts.as_slice() {
                [a] => (grid_line(a)?, GridLine::Auto),
                [a, b] => (grid_line(a)?, grid_line(b)?),
                _ => return Err(format!("{name} is a start line and an optional end line, separated by a slash").into()),
            };
            single(l, V::Lines(a, b))
        }
        "gridArea" => {
            match toks.as_slice() {
                [Tok::Ident(k)] if k == "auto" => return single(GridArea, V::Area(None, [GridLine::Auto, GridLine::Auto, GridLine::Auto, GridLine::Auto])),
                [Tok::Ident(k)] => return single(GridArea, V::Area(Some(k.clone()), [GridLine::Auto, GridLine::Auto, GridLine::Auto, GridLine::Auto])),
                _ => {}
            }
            let parts = split_slash(&toks);
            if parts.len() != 4 {
                return Err("gridArea is an area name, or four lines separated by slashes".to_string().into());
            }
            let l: Result<Vec<GridLine>, String> = parts.iter().map(|p| grid_line(p)).collect();
            let l = l?;
            single(GridArea, V::Area(None, [l[0].clone(), l[1].clone(), l[2].clone(), l[3].clone()]))
        }
        "alignItems" => single(AlignItems, V::Kw(kw(&toks, &["stretch", "flex-start", "flex-end", "center", "baseline"], "alignItems")?)),
        "alignSelf" => single(AlignSelf, V::Kw(kw(&toks, &["auto", "stretch", "flex-start", "flex-end", "center", "baseline"], "alignSelf")?)),
        "alignContent" => single(AlignContent, V::Kw(kw(&toks, &["stretch", "flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly"], "alignContent")?)),
        "justifyContent" => single(JustifyContent, V::Kw(kw(&toks, &["flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly"], "justifyContent")?)),
        "justifyItems" => single(JustifyItems, V::Kw(kw(&toks, &["stretch", "start", "end", "center", "baseline"], "justifyItems")?)),
        "justifySelf" => single(JustifySelf, V::Kw(kw(&toks, &["auto", "stretch", "start", "end", "center", "baseline"], "justifySelf")?)),
        "gap" => {
            let v = lens(&toks, &nonneg_pct, 2)?;
            let col = v.get(1).copied().unwrap_or(v[0]);
            Ok(vec![(RowGap, V::Len(v[0])), (ColumnGap, V::Len(col))])
        }
        "rowGap" => single(RowGap, V::Len(parse_len(one(&toks)?, &nonneg_pct)?)),
        "columnGap" => single(ColumnGap, V::Len(parse_len(one(&toks)?, &nonneg_pct)?)),
        "position" => single(Position, V::Kw(kw(&toks, &["static", "relative", "absolute"], "position")?)),
        "top" | "right" | "bottom" | "left" => {
            let l = match name { "top" => Top, "right" => Right, "bottom" => Bottom, _ => Left };
            single(l, V::Len(parse_len(one(&toks)?, &pos_any)?))
        }
        "inset" => {
            let v = four(&lens(&toks, &pos_any, 4)?);
            Ok(SIDES_INSET.iter().zip(v).map(|(l, v)| (*l, V::Len(v))).collect())
        }
        "zIndex" => match one(&toks)? {
            Tok::Ident(k) if k == "auto" => single(ZIndex, V::Int(None)),
            Tok::Num(v, u) if u.is_empty() && v.fract() == 0.0 => single(ZIndex, V::Int(Some(*v as i32))),
            _ => Err("zIndex is auto or a whole number".to_string().into()),
        },
        "width" | "height" | "minWidth" | "minHeight" => {
            let l = match name { "width" => Width, "height" => Height, "minWidth" => MinWidth, _ => MinHeight };
            single(l, V::Len(parse_len(one(&toks)?, &size_auto)?))
        }
        "maxWidth" | "maxHeight" => {
            let l = if name == "maxWidth" { MaxWidth } else { MaxHeight };
            single(l, V::Len(parse_len(one(&toks)?, &size_none)?))
        }
        "aspectRatio" => match toks.as_slice() {
            [Tok::Ident(k)] if k == "auto" => single(AspectRatio, V::Ratio(None)),
            [Tok::Num(v, u)] if u.is_empty() && *v > 0.0 => single(AspectRatio, V::Ratio(Some(*v as f32))),
            [Tok::Num(a, u), Tok::Slash, Tok::Num(b, u2)] if u.is_empty() && u2.is_empty() && *a > 0.0 && *b > 0.0 => {
                single(AspectRatio, V::Ratio(Some((*a / *b) as f32)))
            }
            _ => Err("aspectRatio is auto, a number, or a width / height".to_string().into()),
        },
        "margin" => {
            let v = four(&lens(&toks, &pos_any, 4)?);
            Ok(SIDES_MARGIN.iter().zip(v).map(|(l, v)| (*l, V::Len(v))).collect())
        }
        "marginTop" | "marginRight" | "marginBottom" | "marginLeft" => {
            let l = longhand_of(name).unwrap();
            single(l, V::Len(parse_len(one(&toks)?, &pos_any)?))
        }
        "padding" => {
            let v = four(&lens(&toks, &nonneg_pct, 4)?);
            Ok(SIDES_PADDING.iter().zip(v).map(|(l, v)| (*l, V::Len(v))).collect())
        }
        "paddingTop" | "paddingRight" | "paddingBottom" | "paddingLeft" => {
            let l = longhand_of(name).unwrap();
            single(l, V::Len(parse_len(one(&toks)?, &nonneg_pct)?))
        }
        "border" => {
            let (w, s, c) = border_parts(&toks)?;
            let mut out = Vec::new();
            for i in 0..4 {
                out.push((BORDER_WIDTH[i], V::Len(w)));
                out.push((BORDER_STYLE[i], V::Kw(s)));
                out.push((BORDER_COLOR[i], V::Color(c.clone())));
            }
            Ok(out)
        }
        "borderTop" | "borderRight" | "borderBottom" | "borderLeft" => {
            let i = ["borderTop", "borderRight", "borderBottom", "borderLeft"].iter().position(|n| *n == name).unwrap();
            let (w, s, c) = border_parts(&toks)?;
            Ok(vec![(BORDER_WIDTH[i], V::Len(w)), (BORDER_STYLE[i], V::Kw(s)), (BORDER_COLOR[i], V::Color(c))])
        }
        "borderWidth" => {
            let v = four(&lens(&toks, &px_only, 4)?);
            Ok(BORDER_WIDTH.iter().zip(v).map(|(l, v)| (*l, V::Len(v))).collect())
        }
        "borderStyle" => {
            if toks.is_empty() || toks.len() > 4 {
                return Err("borderStyle takes from 1 to 4 values".to_string().into());
            }
            let s: Result<Vec<&'static str>, String> = toks.iter().map(|t| kw(std::slice::from_ref(t), BORDER_STYLES, "borderStyle")).collect();
            let v = four(&s?);
            Ok(BORDER_STYLE.iter().zip(v).map(|(l, v)| (*l, V::Kw(v))).collect())
        }
        "borderColor" => {
            if toks.is_empty() || toks.len() > 4 {
                return Err("borderColor takes from 1 to 4 colours".to_string().into());
            }
            let c: Result<Vec<crate::values::Color>, String> = toks.iter().map(parse_color).collect();
            let v = four(&c?);
            Ok(BORDER_COLOR.iter().zip(v).map(|(l, v)| (*l, V::Color(v))).collect())
        }
        "borderRadius" => {
            let v = four(&lens(&toks, &nonneg_pct, 4)?);
            Ok(RADII.iter().zip(v).map(|(l, v)| (*l, V::Len(v))).collect())
        }
        "overflow" => single(Overflow, V::Kw(kw(&toks, &["visible", "hidden"], "overflow")?)),
        "fontFamily" => {
            let mut fams = Vec::new();
            for part in split_commas(&toks) {
                let name = match part.as_slice() {
                    [Tok::Str(s)] => s.clone(),
                    words if !words.is_empty() && words.iter().all(|w| matches!(w, Tok::Ident(_))) => words
                        .iter()
                        .map(|w| if let Tok::Ident(s) = w { s.as_str() } else { "" })
                        .collect::<Vec<_>>()
                        .join(" "),
                    _ => return Err("fontFamily is a list of Google Fonts family names, separated by commas".to_string().into()),
                };
                if ["serif", "sans-serif", "monospace", "system-ui", "cursive", "fantasy"].contains(&name.as_str()) {
                    return Err(PErr {
                        code: "unknown-font-family",
                        msg: format!("`{name}` is a generic family; Emul only uses Google Fonts families, such as Inter"),
                    });
                }
                fams.push(name);
            }
            single(FontFamily, V::Families(fams))
        }
        "fontSize" => match one(&toks)? {
            t @ Tok::Num(_, u) if u == "px" || u.is_empty() => match parse_len(t, &px_only)? {
                Len::Px(v) if v > 0.0 => single(FontSize, V::Len(Len::Px(v))),
                _ => Err("fontSize must be more than 0px".to_string().into()),
            },
            _ => Err("fontSize is a size in px".to_string().into()),
        },
        "fontWeight" => match one(&toks)? {
            Tok::Ident(k) if k == "normal" => single(FontWeight, V::Weight(400)),
            Tok::Ident(k) if k == "bold" => single(FontWeight, V::Weight(700)),
            Tok::Num(v, u) if u.is_empty() && *v >= 1.0 && *v <= 1000.0 => single(FontWeight, V::Weight(*v as u16)),
            _ => Err("fontWeight is normal, bold, or a number from 1 to 1000".to_string().into()),
        },
        "fontStyle" => single(FontStyle, V::Kw(kw(&toks, &["normal", "italic", "oblique"], "fontStyle")?)),
        "lineHeight" => match one(&toks)? {
            Tok::Ident(k) if k == "normal" => single(LineHeight, V::LineHeight(crate::props::LineHeight::Normal)),
            Tok::Num(v, u) if u.is_empty() && *v >= 0.0 => single(LineHeight, V::LineHeight(crate::props::LineHeight::Number(*v as f32))),
            t => match parse_len(t, &nonneg_pct)? {
                Len::Px(v) => single(LineHeight, V::LineHeight(crate::props::LineHeight::Px(v))),
                Len::Pct(v) => single(LineHeight, V::LineHeight(crate::props::LineHeight::Pct(v))),
                _ => unreachable!(),
            },
        },
        "whiteSpace" => single(WhiteSpace, V::Kw(kw(&toks, &["normal", "nowrap", "pre", "pre-wrap", "pre-line"], "whiteSpace")?)),
        "textOverflow" => single(TextOverflow, V::Kw(kw(&toks, &["clip", "ellipsis"], "textOverflow")?)),
        "lineClamp" => match one(&toks)? {
            Tok::Ident(k) if k == "none" => single(LineClamp, V::Int(None)),
            Tok::Num(v, u) if u.is_empty() && v.fract() == 0.0 && *v >= 1.0 => single(LineClamp, V::Int(Some(*v as i32))),
            _ => Err("lineClamp is none or a whole number of lines".to_string().into()),
        },
        "textAlign" => single(TextAlign, V::Kw(kw(&toks, &["start", "end", "left", "right", "center", "justify"], "textAlign")?)),
        "letterSpacing" => match one(&toks)? {
            Tok::Ident(k) if k == "normal" => single(LetterSpacing, V::Len(Len::Px(0.0))),
            t => single(LetterSpacing, V::Len(parse_len(t, &LenRules { auto: false, none: false, pct: false, negative: true })?)),
        },
        "textDecoration" => {
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "none" {
                    return single(TextDecoration, V::Decoration(None));
                }
            }
            let mut d = Decoration { underline: false, overline: false, line_through: false, style: "solid", color: None };
            for t in &toks {
                match t {
                    Tok::Ident(k) if k == "underline" => d.underline = true,
                    Tok::Ident(k) if k == "overline" => d.overline = true,
                    Tok::Ident(k) if k == "line-through" => d.line_through = true,
                    Tok::Ident(k) if ["solid", "double", "dotted", "dashed", "wavy"].contains(&k.as_str()) => {
                        d.style = ["solid", "double", "dotted", "dashed", "wavy"].iter().find(|s| **s == k.as_str()).unwrap();
                    }
                    _ => d.color = Some(parse_color(t)?),
                }
            }
            if !(d.underline || d.overline || d.line_through) {
                return Err("textDecoration needs a line: underline, overline or line-through".to_string().into());
            }
            single(TextDecoration, V::Decoration(Some(d)))
        }
        "color" => single(Color, V::Color(parse_color(one(&toks)?)?)),
        "textTransform" => single(TextTransform, V::Kw(kw(&toks, &["none", "uppercase", "lowercase", "capitalize"], "textTransform")?)),
        "textShadow" | "boxShadow" => {
            let l = if name == "textShadow" { TextShadow } else { BoxShadow };
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "none" {
                    return single(l, V::Shadows(Vec::new()));
                }
            }
            single(l, V::Shadows(parse_shadows(&toks, name == "boxShadow")?))
        }
        "backgroundColor" => match one(&toks)? {
            Tok::Ident(k) if k == "none" => single(BackgroundColor, V::OptColor(None)),
            t => single(BackgroundColor, V::OptColor(Some(parse_color(t)?))),
        },
        "backgroundImage" => {
            if let [Tok::Ident(k)] = toks.as_slice() {
                if k == "none" {
                    return single(BackgroundImage, V::Images(Vec::new()));
                }
            }
            let mut imgs = Vec::new();
            for part in split_commas(&toks) {
                match part.as_slice() {
                    [Tok::Url(u)] => imgs.push(Image::Url(image_file(u, ctx)?)),
                    [Tok::Func(f, args)] if f == "linear-gradient" || f == "radial-gradient" => imgs.push(parse_gradient(f, args)?),
                    _ => return Err("backgroundImage is none, or a list of url(), linear-gradient() and radial-gradient()".to_string().into()),
                }
            }
            single(BackgroundImage, V::Images(imgs))
        }
        "backgroundSize" => match toks.as_slice() {
            [Tok::Ident(k)] if k == "cover" => single(BackgroundSize, V::BgSize(BgSize::Cover)),
            [Tok::Ident(k)] if k == "contain" => single(BackgroundSize, V::BgSize(BgSize::Contain)),
            [a] => single(BackgroundSize, V::BgSize(BgSize::Explicit(parse_len(a, &size_auto)?, Len::Auto))),
            [a, b] => single(BackgroundSize, V::BgSize(BgSize::Explicit(parse_len(a, &size_auto)?, parse_len(b, &size_auto)?))),
            _ => Err("backgroundSize is auto, cover, contain, or one or two sizes".to_string().into()),
        },
        "backgroundPosition" | "objectPosition" => {
            let l = if name == "backgroundPosition" { BackgroundPosition } else { ObjectPosition };
            let (x, y) = parse_position(&toks)?;
            single(l, V::Pos(x, y))
        }
        "backgroundRepeat" => single(BackgroundRepeat, V::Kw(kw(&toks, &["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"], "backgroundRepeat")?)),
        "opacity" => match one(&toks)? {
            Tok::Num(v, u) if u.is_empty() => single(Opacity, V::Num((*v as f32).clamp(0.0, 1.0))),
            Tok::Num(v, u) if u == "%" => single(Opacity, V::Num((*v as f32 / 100.0).clamp(0.0, 1.0))),
            _ => Err("opacity is a number from 0 to 1, or a percentage".to_string().into()),
        },
        "translate" => match toks.as_slice() {
            [Tok::Ident(k)] if k == "none" => single(Translate, V::Translate(Len::Px(0.0), Len::Px(0.0))),
            [x] => single(Translate, V::Translate(parse_len(x, &LenRules { auto: false, ..pos_any })?, Len::Px(0.0))),
            [x, y] => {
                let r = LenRules { auto: false, ..pos_any };
                single(Translate, V::Translate(parse_len(x, &r)?, parse_len(y, &r)?))
            }
            _ => Err("translate is none, or an x offset and an optional y offset".to_string().into()),
        },
        "rotate" => match one(&toks)? {
            Tok::Ident(k) if k == "none" => single(Rotate, V::Rotate(0.0)),
            Tok::Num(v, u) if u == "deg" => single(Rotate, V::Rotate(*v as f32)),
            Tok::Num(v, u) if u.is_empty() && *v == 0.0 => single(Rotate, V::Rotate(0.0)),
            _ => Err("rotate is none or an angle in deg".to_string().into()),
        },
        "scale" => {
            let f = |t: &Tok| -> Result<f32, String> {
                match t {
                    Tok::Num(v, u) if u.is_empty() => Ok(*v as f32),
                    Tok::Num(v, u) if u == "%" => Ok(*v as f32 / 100.0),
                    _ => Err("scale is none, or one or two numbers or percentages".into()),
                }
            };
            match toks.as_slice() {
                [Tok::Ident(k)] if k == "none" => single(Scale, V::Scale(1.0, 1.0)),
                [a] => {
                    let v = f(a)?;
                    single(Scale, V::Scale(v, v))
                }
                [a, b] => single(Scale, V::Scale(f(a)?, f(b)?)),
                _ => Err("scale is none, or one or two numbers or percentages".to_string().into()),
            }
        }
        "objectFit" => single(ObjectFit, V::Kw(kw(&toks, &["fill", "contain", "cover", "none", "scale-down"], "objectFit")?)),
        "htmlTag" => {
            let t = value.trim();
            if t.is_empty() || !t.chars().next().unwrap().is_ascii_lowercase() || !t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                return Err(PErr { code: "invalid-html-tag", msg: format!("`{t}` is not a lowercase HTML element name") });
            }
            if ["html", "head", "body", "script", "style"].contains(&t) {
                return Err(PErr { code: "invalid-html-tag", msg: format!("`{t}` can't be used as an htmlTag; the HTML output uses it itself") });
            }
            single(HtmlTag, V::Tag(t.to_string()))
        }
        _ => Err(PErr { code: "unknown-property", msg: format!("`{name}` is not a property") }),
    }
}

/// The full style of a component, after inheritance and the shorthand rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub set: BTreeMap<L, V>,
}

impl Style {
    pub fn get(&self, l: L) -> Option<&V> {
        self.set.get(&l)
    }
    pub fn kw(&self, l: L) -> &'static str {
        match self.set.get(&l) {
            Some(V::Kw(k)) => k,
            _ => default_kw(l),
        }
    }
    pub fn len(&self, l: L) -> Len {
        match self.set.get(&l) {
            Some(V::Len(v)) => *v,
            _ => default_len(l),
        }
    }
    pub fn num(&self, l: L) -> f32 {
        match self.set.get(&l) {
            Some(V::Num(v)) => *v,
            _ => match l {
                L::FlexShrink | L::Opacity => 1.0,
                _ => 0.0,
            },
        }
    }
    pub fn int(&self, l: L) -> Option<i32> {
        match self.set.get(&l) {
            Some(V::Int(v)) => *v,
            _ => None,
        }
    }
    pub fn display(&self) -> &'static str {
        self.kw(L::Display)
    }
    pub fn color(&self) -> crate::values::Color {
        match self.set.get(&L::Color) {
            Some(V::Color(c)) => c.clone(),
            _ => crate::values::Color::black(),
        }
    }
    pub fn border_color(&self, i: usize) -> crate::values::Color {
        match self.set.get(&BORDER_COLOR[i]) {
            Some(V::Color(c)) => c.clone(),
            _ => crate::values::Color::black(),
        }
    }
    pub fn families(&self) -> Vec<String> {
        match self.set.get(&L::FontFamily) {
            Some(V::Families(f)) => f.clone(),
            _ => vec!["Inter".to_string()],
        }
    }
    pub fn font_size(&self) -> f32 {
        match self.set.get(&L::FontSize) {
            Some(V::Len(Len::Px(v))) => *v,
            _ => 16.0,
        }
    }
    pub fn weight(&self) -> u16 {
        match self.set.get(&L::FontWeight) {
            Some(V::Weight(w)) => *w,
            _ => 400,
        }
    }
    pub fn italic(&self) -> bool {
        self.kw(L::FontStyle) != "normal"
    }
    pub fn line_height(&self) -> LineHeight {
        match self.set.get(&L::LineHeight) {
            Some(V::LineHeight(l)) => l.clone(),
            _ => LineHeight::Normal,
        }
    }
    pub fn tag(&self) -> String {
        match self.set.get(&L::HtmlTag) {
            Some(V::Tag(t)) => t.clone(),
            _ => "div".into(),
        }
    }
    pub fn is_positioned(&self) -> bool {
        self.kw(L::Position) != "static"
    }
    pub fn shadows(&self, l: L) -> Vec<Shadow> {
        match self.set.get(&l) {
            Some(V::Shadows(s)) => s.clone(),
            _ => Vec::new(),
        }
    }
    pub fn decoration(&self) -> Option<Decoration> {
        match self.set.get(&L::TextDecoration) {
            Some(V::Decoration(d)) => d.clone(),
            _ => None,
        }
    }
}

pub fn default_kw(l: L) -> &'static str {
    use L::*;
    match l {
        Display => "block",
        FlexDirection => "row",
        FlexWrap => "nowrap",
        AlignItems => "stretch",
        AlignSelf => "auto",
        AlignContent => "stretch",
        JustifyContent => "flex-start",
        JustifyItems => "stretch",
        JustifySelf => "auto",
        Position => "static",
        Overflow => "visible",
        FontStyle => "normal",
        WhiteSpace => "normal",
        TextOverflow => "clip",
        TextAlign => "start",
        TextTransform => "none",
        BackgroundRepeat => "repeat",
        ObjectFit => "fill",
        BorderTopStyle | BorderRightStyle | BorderBottomStyle | BorderLeftStyle => "none",
        _ => "",
    }
}

pub fn default_len(l: L) -> Len {
    use L::*;
    match l {
        FlexBasis | Width | Height | MinWidth | MinHeight | Top | Right | Bottom | Left => Len::Auto,
        MaxWidth | MaxHeight => Len::None,
        _ => Len::Px(0.0),
    }
}

/// CSS text for a longhand value, for the HTML output. `own_color` replaces
/// colours that use the default, which is the own colour of the component.
pub fn css_value(l: L, v: &V, own_color: &crate::values::Color, asset: &dyn Fn(&Path) -> String) -> String {
    match v {
        V::Kw(k) => k.to_string(),
        V::Num(n) => fmt_num(*n as f64),
        V::Int(None) => if l == L::LineClamp { "none".into() } else { "auto".into() },
        V::Int(Some(i)) => i.to_string(),
        V::Len(len) => len.css(),
        V::Color(c) => c.css.clone(),
        V::OptColor(None) => "transparent".into(),
        V::OptColor(Some(c)) => c.css.clone(),
        V::Families(f) => f.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", "),
        V::Weight(w) => w.to_string(),
        V::LineHeight(lh) => match lh {
            LineHeight::Normal => "normal".into(),
            LineHeight::Number(n) => fmt_num(*n as f64),
            LineHeight::Px(p) => format!("{}px", fmt_num(*p as f64)),
            LineHeight::Pct(p) => format!("{}%", fmt_num(*p as f64)),
        },
        V::Template(None) => "none".into(),
        V::Template(Some(items)) => items
            .iter()
            .map(|i| match i {
                TemplateItem::Track(t) => t.css(),
                TemplateItem::Names(n) => format!("[{}]", n.join(" ")),
                TemplateItem::Repeat(c, t) => format!(
                    "repeat({}, {})",
                    match c {
                        RepeatCount::Count(n) => n.to_string(),
                        RepeatCount::AutoFill => "auto-fill".into(),
                        RepeatCount::AutoFit => "auto-fit".into(),
                    },
                    t.iter().map(|t| t.css()).collect::<Vec<_>>().join(" ")
                ),
            })
            .collect::<Vec<_>>()
            .join(" "),
        V::AutoTracks(t) => t.iter().map(|t| t.css()).collect::<Vec<_>>().join(" "),
        V::Areas(None) => "none".into(),
        V::Areas(Some(rows)) => rows.iter().map(|r| format!("\"{}\"", r.join(" "))).collect::<Vec<_>>().join(" "),
        V::Lines(a, b) => {
            if *b == GridLine::Auto { a.css() } else { format!("{} / {}", a.css(), b.css()) }
        }
        V::Area(Some(n), _) => n.clone(),
        V::Area(None, l) => l.iter().map(|g| g.css()).collect::<Vec<_>>().join(" / "),
        V::Ratio(None) => "auto".into(),
        V::Ratio(Some(r)) => fmt_num(*r as f64),
        V::Decoration(None) => "none".into(),
        V::Decoration(Some(d)) => {
            let mut parts = Vec::new();
            if d.underline {
                parts.push("underline".to_string());
            }
            if d.overline {
                parts.push("overline".to_string());
            }
            if d.line_through {
                parts.push("line-through".to_string());
            }
            parts.push(d.style.to_string());
            parts.push(d.color.as_ref().unwrap_or(own_color).css.clone());
            parts.join(" ")
        }
        V::Shadows(s) if s.is_empty() => "none".into(),
        V::Shadows(s) => shadows_css(s, own_color),
        V::Images(i) if i.is_empty() => "none".into(),
        V::Images(imgs) => imgs
            .iter()
            .map(|i| match i {
                Image::Url(p) => format!("url(\"{}\")", asset(p)),
                Image::Linear { angle, stops } => {
                    let dir = match angle {
                        LinearDir::Angle(a) => format!("{}deg", fmt_num(*a as f64)),
                        LinearDir::Corner(x, y) => format!(
                            "to {} {}",
                            if *y < 0.0 { "top" } else { "bottom" },
                            if *x < 0.0 { "left" } else { "right" }
                        ),
                    };
                    format!("linear-gradient({dir} in srgb, {})", stops_css(stops))
                }
                Image::Radial { circle, at, stops } => format!(
                    "radial-gradient({} at {} {} in srgb, {})",
                    if *circle { "circle" } else { "ellipse" },
                    at.0.css(),
                    at.1.css(),
                    stops_css(stops)
                ),
            })
            .collect::<Vec<_>>()
            .join(", "),
        V::BgSize(b) => match b {
            BgSize::Auto => "auto".into(),
            BgSize::Cover => "cover".into(),
            BgSize::Contain => "contain".into(),
            BgSize::Explicit(a, b) => format!("{} {}", a.css(), b.css()),
        },
        V::Pos(x, y) => format!("{} {}", x.css(), y.css()),
        V::Translate(x, y) => format!("{} {}", x.css(), y.css()),
        V::Rotate(r) => format!("{}deg", fmt_num(*r as f64)),
        V::Scale(a, b) => format!("{} {}", fmt_num(*a as f64), fmt_num(*b as f64)),
        V::Tag(t) => t.clone(),
    }
}
