//! Arranges a composition with taffy, and measures text with cosmic-text. The result is a
//! box for each instance and the lines of text with their positions. All positions are
//! absolute canvas coordinates.

use crate::project::{Child, Node};
use crate::props::{self, GridLine, RepeatCount, Style, TemplateItem, Track, L, V};
use crate::text::{self, Fonts, Paragraph, Shaped, Span, SpanStyle};
use crate::values::Len;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use taffy::prelude::*;
use taffy::{GridTemplateArea, GridTemplateAreas, GridTemplateComponent, GridTemplateRepetition};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct LBox {
    pub style: Arc<Style>,
    pub id: String,
    pub src: Option<PathBuf>,
    /// The border box, in canvas coordinates.
    pub rect: Rect,
    /// The border widths that the code draws: top, right, bottom, left.
    pub border: [f32; 4],
    pub padding: [f32; 4],
    pub children: Vec<LItem>,
    tid: NodeId,
}

impl LBox {
    pub fn padding_box(&self) -> Rect {
        let [t, r, b, l] = self.border;
        Rect { x: self.rect.x + l, y: self.rect.y + t, w: (self.rect.w - l - r).max(0.0), h: (self.rect.h - t - b).max(0.0) }
    }
    pub fn content_box(&self) -> Rect {
        let p = self.padding_box();
        let [t, r, b, l] = self.padding;
        Rect { x: p.x + l, y: p.y + t, w: (p.w - l - r).max(0.0), h: (p.h - t - b).max(0.0) }
    }
}

pub struct LText {
    pub rect: Rect,
    pub para: Paragraph,
    pub styles: Vec<SpanStyle>,
    pub shaped: Option<Shaped>,
    tid: NodeId,
}

pub enum LItem {
    Box(LBox),
    Text(LText),
}

pub struct Laid {
    pub width: f32,
    pub height: f32,
    pub items: Vec<LItem>,
}

enum Ctx {
    Text(usize),
    Image(f32, f32),
}

struct Builder<'a> {
    tree: TaffyTree<Ctx>,
    paras: Vec<Paragraph>,
    fonts: &'a mut Fonts,
    natural: HashMap<PathBuf, (f32, f32)>,
}

fn lpa(l: Len) -> LengthPercentageAuto {
    match l {
        Len::Px(v) => LengthPercentageAuto::length(v),
        Len::Pct(v) => LengthPercentageAuto::percent(v / 100.0),
        _ => LengthPercentageAuto::auto(),
    }
}

fn lp(l: Len) -> LengthPercentage {
    match l {
        Len::Pct(v) => LengthPercentage::percent(v / 100.0),
        Len::Px(v) => LengthPercentage::length(v),
        _ => LengthPercentage::length(0.0),
    }
}

fn dim(l: Len) -> Dimension {
    match l {
        Len::Px(v) => Dimension::length(v),
        Len::Pct(v) => Dimension::percent(v / 100.0),
        _ => Dimension::auto(),
    }
}

fn track_fn(t: &Track) -> TrackSizingFunction {
    match t {
        Track::Px(v) => TrackSizingFunction::from_length(*v),
        Track::Pct(v) => TrackSizingFunction::from_percent(*v / 100.0),
        Track::Fr(v) => TrackSizingFunction::from_fr(*v),
        Track::Auto => TrackSizingFunction::AUTO,
        Track::MinMax(a, b) => {
            let min = match a.as_ref() {
                Track::Px(v) => MinTrackSizingFunction::length(*v),
                Track::Pct(v) => MinTrackSizingFunction::percent(*v / 100.0),
                _ => MinTrackSizingFunction::auto(),
            };
            let max = match b.as_ref() {
                Track::Px(v) => MaxTrackSizingFunction::length(*v),
                Track::Pct(v) => MaxTrackSizingFunction::percent(*v / 100.0),
                Track::Fr(v) => MaxTrackSizingFunction::fr(*v),
                _ => MaxTrackSizingFunction::auto(),
            };
            taffy::MinMax { min, max }
        }
    }
}

fn template(items: &[TemplateItem]) -> (Vec<GridTemplateComponent<String>>, Vec<Vec<String>>) {
    let mut comps = Vec::new();
    let mut names: Vec<Vec<String>> = vec![Vec::new()];
    for it in items {
        match it {
            TemplateItem::Names(n) => names.last_mut().unwrap().extend(n.iter().cloned()),
            TemplateItem::Track(t) => {
                comps.push(GridTemplateComponent::Single(track_fn(t)));
                names.push(Vec::new());
            }
            TemplateItem::Repeat(c, ts) => {
                let count = match c {
                    RepeatCount::Count(n) => taffy::RepetitionCount::Count(*n),
                    RepeatCount::AutoFill => taffy::RepetitionCount::AutoFill,
                    RepeatCount::AutoFit => taffy::RepetitionCount::AutoFit,
                };
                let tracks: Vec<TrackSizingFunction> = ts.iter().map(track_fn).collect();
                let line_names = vec![Vec::new(); tracks.len() + 1];
                comps.push(GridTemplateComponent::Repeat(GridTemplateRepetition { count, tracks, line_names }));
                names.push(Vec::new());
            }
        }
    }
    let any = names.iter().any(|n| !n.is_empty());
    (comps, if any { names } else { Vec::new() })
}

fn placement(g: &GridLine) -> GridPlacement<String> {
    match g {
        GridLine::Auto => GridPlacement::Auto,
        GridLine::Line(n) => GridPlacement::from_line_index(*n),
        GridLine::Named(n) => GridPlacement::NamedLine(n.clone(), 0),
        GridLine::Span(n) => GridPlacement::Span(*n),
    }
}

fn areas(rows: &[Vec<String>]) -> GridTemplateAreas<String> {
    let mut found: Vec<GridTemplateArea<String>> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            if name.chars().all(|ch| ch == '.') {
                continue;
            }
            if let Some(a) = found.iter_mut().find(|a| a.name == *name) {
                a.row_start = a.row_start.min(r as u16 + 1);
                a.row_end = a.row_end.max(r as u16 + 2);
                a.column_start = a.column_start.min(c as u16 + 1);
                a.column_end = a.column_end.max(c as u16 + 2);
            } else {
                found.push(GridTemplateArea {
                    name: name.clone(),
                    row_start: r as u16 + 1,
                    row_end: r as u16 + 2,
                    column_start: c as u16 + 1,
                    column_end: c as u16 + 2,
                });
            }
        }
    }
    GridTemplateAreas { areas: found.into(), row_count: rows.len() as u16, column_count: rows.first().map(|r| r.len()).unwrap_or(0) as u16 }
}

fn align_items(k: &str) -> Option<AlignItems> {
    Some(match k {
        "stretch" => AlignItems::STRETCH,
        "flex-start" => AlignItems::FLEX_START,
        "flex-end" => AlignItems::FLEX_END,
        "center" => AlignItems::CENTER,
        "baseline" => AlignItems::BASELINE,
        "start" => AlignItems::START,
        "end" => AlignItems::END,
        _ => return None,
    })
}

fn align_content(k: &str) -> Option<AlignContent> {
    Some(match k {
        "stretch" => AlignContent::STRETCH,
        "flex-start" => AlignContent::FLEX_START,
        "flex-end" => AlignContent::FLEX_END,
        "center" => AlignContent::CENTER,
        "space-between" => AlignContent::SPACE_BETWEEN,
        "space-around" => AlignContent::SPACE_AROUND,
        "space-evenly" => AlignContent::SPACE_EVENLY,
        _ => return None,
    })
}

/// True when an element arranges its children as a block. Emul arranges blocks as flex
/// columns, which never collapse margins. The HTML output does the same.
fn is_blockish(display: &str) -> bool {
    display == "block" || display == "inline"
}

/// The taffy style of an element.
fn element_style(s: &Style, in_block: bool) -> taffy::Style {
    let mut t = taffy::Style::default();
    t.box_sizing = taffy::BoxSizing::BorderBox;
    let display = s.display();
    match display {
        "none" => t.display = Display::None,
        "flex" => t.display = Display::Flex,
        "grid" => t.display = Display::Grid,
        _ => {
            t.display = Display::Flex;
            t.flex_direction = FlexDirection::Column;
            t.flex_wrap = FlexWrap::NoWrap;
            t.align_items = AlignItems::STRETCH;
        }
    }
    let inline = display == "inline";
    t.position = if s.kw(L::Position) == "absolute" { Position::Absolute } else { Position::Relative };
    if s.is_positioned() {
        t.inset = taffy::Rect {
            top: lpa(s.len(L::Top)),
            right: lpa(s.len(L::Right)),
            bottom: lpa(s.len(L::Bottom)),
            left: lpa(s.len(L::Left)),
        };
    }
    if !inline {
        t.size = Size { width: dim(s.len(L::Width)), height: dim(s.len(L::Height)) };
        t.min_size = Size { width: lpa(s.len(L::MinWidth)), height: lpa(s.len(L::MinHeight)) };
        t.max_size = Size { width: lpa(s.len(L::MaxWidth)), height: lpa(s.len(L::MaxHeight)) };
        if let Some(V::Ratio(r)) = s.get(L::AspectRatio) {
            t.aspect_ratio = *r;
        }
        t.margin = taffy::Rect {
            top: lpa(s.len(L::MarginTop)),
            right: lpa(s.len(L::MarginRight)),
            bottom: lpa(s.len(L::MarginBottom)),
            left: lpa(s.len(L::MarginLeft)),
        };
        t.padding = taffy::Rect {
            top: lp(s.len(L::PaddingTop)),
            right: lp(s.len(L::PaddingRight)),
            bottom: lp(s.len(L::PaddingBottom)),
            left: lp(s.len(L::PaddingLeft)),
        };
        let bw = border_widths(s);
        t.border = taffy::Rect {
            top: LengthPercentage::length(bw[0]),
            right: LengthPercentage::length(bw[1]),
            bottom: LengthPercentage::length(bw[2]),
            left: LengthPercentage::length(bw[3]),
        };
    }
    if s.kw(L::Overflow) == "hidden" {
        t.overflow = taffy::Point { x: taffy::Overflow::Hidden, y: taffy::Overflow::Hidden };
        t.scrollbar_width = 0.0;
    }
    if display == "flex" || display == "grid" {
        t.align_items = align_items(s.kw(L::AlignItems)).unwrap_or(AlignItems::STRETCH);
        t.align_content = align_content(s.kw(L::AlignContent)).unwrap_or(AlignContent::STRETCH);
        t.justify_content = align_content(s.kw(L::JustifyContent)).unwrap_or(AlignContent::FLEX_START);
        t.gap = Size { width: lp(s.len(L::ColumnGap)), height: lp(s.len(L::RowGap)) };
    }
    if display == "flex" {
        t.flex_direction = match s.kw(L::FlexDirection) {
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => FlexDirection::Row,
        };
        t.flex_wrap = match s.kw(L::FlexWrap) {
            "wrap" => FlexWrap::Wrap,
            "wrap-reverse" => FlexWrap::WrapReverse,
            _ => FlexWrap::NoWrap,
        };
    }
    if display == "grid" {
        t.justify_items = align_items(s.kw(L::JustifyItems)).unwrap_or(AlignItems::STRETCH);
        if let Some(V::Template(Some(items))) = s.get(L::GridTemplateColumns) {
            let (c, n) = template(items);
            t.grid_template_columns = c;
            t.grid_template_column_names = n;
        }
        if let Some(V::Template(Some(items))) = s.get(L::GridTemplateRows) {
            let (c, n) = template(items);
            t.grid_template_rows = c;
            t.grid_template_row_names = n;
        }
        if let Some(V::AutoTracks(tr)) = s.get(L::GridAutoRows) {
            t.grid_auto_rows = tr.iter().map(track_fn).collect();
        }
        if let Some(V::Areas(Some(rows))) = s.get(L::GridTemplateAreas) {
            t.grid_template_areas = Some(areas(rows));
        }
    }
    // As an item.
    if in_block {
        // Children of a block do not grow, shrink or align themselves.
        t.flex_grow = 0.0;
        t.flex_shrink = 0.0;
        t.flex_basis = Dimension::auto();
        t.align_self = None;
        // A block child with an auto width and auto side margins still fills the line.
        // Then its margins get the space that remains, as in CSS block layout.
        let auto_margin = matches!(s.len(L::MarginLeft), Len::Auto) || matches!(s.len(L::MarginRight), Len::Auto);
        if auto_margin && matches!(s.len(L::Width), Len::Auto) && !inline {
            t.size.width = Dimension::percent(1.0);
        }
    } else {
        t.flex_grow = s.num(L::FlexGrow);
        t.flex_shrink = s.num(L::FlexShrink);
        t.flex_basis = match s.get(L::FlexBasis) {
            Some(V::Kw("content")) => Dimension::content(),
            _ => dim(s.len(L::FlexBasis)),
        };
        let a = s.kw(L::AlignSelf);
        t.align_self = if a == "auto" { None } else { align_items(a) };
        let j = s.kw(L::JustifySelf);
        t.justify_self = if j == "auto" { None } else { align_items(j) };
    }
    match s.get(L::GridColumn) {
        Some(V::Lines(a, b)) => t.grid_column = Line { start: placement(a), end: placement(b) },
        _ => {}
    }
    match s.get(L::GridRow) {
        Some(V::Lines(a, b)) => t.grid_row = Line { start: placement(a), end: placement(b) },
        _ => {}
    }
    match s.get(L::GridArea) {
        Some(V::Area(Some(name), _)) => {
            t.grid_row = Line { start: GridPlacement::NamedLine(name.clone(), 0), end: GridPlacement::NamedLine(name.clone(), 0) };
            t.grid_column = Line { start: GridPlacement::NamedLine(name.clone(), 0), end: GridPlacement::NamedLine(name.clone(), 0) };
        }
        Some(V::Area(None, l)) => {
            t.grid_row = Line { start: placement(&l[0]), end: placement(&l[2]) };
            t.grid_column = Line { start: placement(&l[1]), end: placement(&l[3]) };
        }
        _ => {}
    }
    t
}

pub fn border_widths(s: &Style) -> [f32; 4] {
    let mut out = [0.0; 4];
    for i in 0..4 {
        if s.kw(props::BORDER_STYLE[i]) != "none" {
            if let Len::Px(v) = s.len(props::BORDER_WIDTH[i]) {
                out[i] = v;
            }
        }
    }
    out
}

fn text_leaf_style(in_block: bool) -> taffy::Style {
    let mut t = taffy::Style::default();
    if in_block {
        t.flex_shrink = 0.0;
    }
    t
}

impl<'a> Builder<'a> {
    fn paragraph(&self, owner: &Style, spans: Vec<Span>) -> Paragraph {
        let ws = owner.kw(L::WhiteSpace);
        let clamp = owner.int(L::LineClamp).map(|n| n as usize);
        let ellipsis = owner.kw(L::TextOverflow) == "ellipsis" && owner.kw(L::Overflow) == "hidden" && matches!(ws, "nowrap" | "pre");
        Paragraph { spans, strut: SpanStyle::of(owner), align: owner.kw(L::TextAlign), white_space: ws, clamp, ellipsis }
    }

    /// Spans for an inline node: its own text in its own style, and its inline children.
    fn inline_spans(node: &Node, out: &mut Vec<Span>) {
        let st = SpanStyle::of(&node.style);
        let tt = node.style.kw(L::TextTransform);
        for c in &node.children {
            match c {
                Child::Text(t) => out.push(Span { text: text::transform(t, tt), style: st.clone() }),
                Child::Node(n) if n.style.display() != "none" => Self::inline_spans(n, out),
                _ => {}
            }
        }
    }

    fn text_node(&mut self, para: Paragraph, in_block: bool) -> (NodeId, LText) {
        let idx = self.paras.len();
        self.paras.push(para.clone());
        let tid = self.tree.new_leaf_with_context(text_leaf_style(in_block), Ctx::Text(idx)).unwrap();
        let styles = text::span_styles(&para);
        (tid, LText { rect: Rect::default(), para, styles, shaped: None, tid })
    }

    /// Builds the taffy subtree for `node`. `positioned` is the taffy node that absolutely
    /// positioned descendants attach to. Returns the box and the taffy children for
    /// the parent (absolutely positioned boxes attach to a different node).
    fn element(&mut self, node: &Node, in_block: bool, positioned: NodeId) -> LBox {
        let style = node.style.clone();
        let mut ts = element_style(&style, in_block);
        let is_img = style.tag() == "img";
        let tid = if is_img {
            let (w, h) = node.src.as_ref().and_then(|p| self.natural_size(p)).unwrap_or((0.0, 0.0));
            ts.item_is_replaced = true;
            // An image with a percentage size can shrink below its natural width in a flex
            // or grid container. Browsers do the same.
            ts.item_is_compressible_replaced = matches!(style.len(L::Width), Len::Pct(_)) || matches!(style.len(L::MaxWidth), Len::Pct(_));
            // Its automatic minimum width is then zero, as browsers compute it.
            if ts.item_is_compressible_replaced && matches!(style.len(L::MinWidth), Len::Auto) {
                ts.min_size.width = LengthPercentageAuto::length(0.0);
            }
            if ts.aspect_ratio.is_none() && w > 0.0 && h > 0.0 {
                ts.aspect_ratio = Some(w / h);
            }
            self.tree.new_leaf_with_context(ts, Ctx::Image(w, h)).unwrap()
        } else {
            self.tree.new_leaf(ts).unwrap()
        };
        let my_positioned = if style.is_positioned() { tid } else { positioned };
        let display = style.display();
        let block = is_blockish(display);
        let mut children = Vec::new();
        if !is_img && display != "none" {
            let mut run: Vec<Span> = Vec::new();
            let tt = style.kw(L::TextTransform);
            let own = SpanStyle::of(&style);
            let flush = |b: &mut Builder, run: &mut Vec<Span>, children: &mut Vec<LItem>| {
                if run.is_empty() {
                    return;
                }
                let para = b.paragraph(&style, std::mem::take(run));
                let (t, lt) = b.text_node(para, block);
                b.tree.add_child(tid, t).unwrap();
                children.push(LItem::Text(lt));
            };
            for c in &node.children {
                match c {
                    Child::Text(t) => run.push(Span { text: text::transform(t, tt), style: own.clone() }),
                    Child::Node(n) => {
                        let nd = n.style.display();
                        if nd == "inline" && block && n.style.kw(L::Position) != "absolute" {
                            Self::inline_spans(n, &mut run);
                            continue;
                        }
                        flush(self, &mut run, &mut children);
                        if nd == "inline" {
                            // An inline child of a flex or grid container becomes an item
                            // that holds its own text.
                            let mut spans = Vec::new();
                            Self::inline_spans(n, &mut spans);
                            let para = self.paragraph(&n.style, spans);
                            let item = self.element(n, block, my_positioned);
                            let (t, lt) = self.text_node(para, true);
                            self.tree.add_child(item.tid, t).unwrap();
                            let mut item = item;
                            item.children.push(LItem::Text(lt));
                            self.attach(tid, my_positioned, &item);
                            children.push(LItem::Box(item));
                            continue;
                        }
                        let child = self.element(n, block, my_positioned);
                        self.attach(tid, my_positioned, &child);
                        children.push(LItem::Box(child));
                    }
                }
            }
            flush(self, &mut run, &mut children);
        }
        LBox { style, id: node.id.clone(), src: node.src.clone(), rect: Rect::default(), border: [0.0; 4], padding: [0.0; 4], children, tid }
    }

    /// Adds a child box to its taffy parent: its own parent, or for absolutely positioned
    /// boxes, the nearest positioned ancestor.
    fn attach(&mut self, parent: NodeId, positioned: NodeId, child: &LBox) {
        let target = if child.style.kw(L::Position) == "absolute" { positioned } else { parent };
        self.tree.add_child(target, child.tid).unwrap();
    }

    fn natural_size(&mut self, path: &PathBuf) -> Option<(f32, f32)> {
        if let Some(s) = self.natural.get(path) {
            return Some(*s);
        }
        let s = natural_image_size(path)?;
        self.natural.insert(path.clone(), s);
        Some(s)
    }
}

/// The natural size of an image file, with the rules in the spec. Raster images
/// use their pixel size. SVGs use their width and height, view box, or 300 by 150.
pub fn natural_image_size(path: &PathBuf) -> Option<(f32, f32)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "svg" {
        let data = std::fs::read(path).ok()?;
        let text = String::from_utf8_lossy(&data);
        let tag_start = text.find("<svg")?;
        let tag_end = text[tag_start..].find('>')? + tag_start;
        let tag = &text[tag_start..tag_end];
        let attr = |n: &str| -> Option<String> {
            let k = format!(" {n}=\"");
            let i = tag.find(&k)? + k.len();
            let j = tag[i..].find('"')? + i;
            Some(tag[i..j].to_string())
        };
        let num = |s: Option<String>| s.and_then(|v| v.trim_end_matches("px").trim().parse::<f32>().ok());
        if let (Some(w), Some(h)) = (num(attr("width")), num(attr("height"))) {
            return Some((w, h));
        }
        if let Some(vb) = attr("viewBox") {
            let p: Vec<f32> = vb.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
            if p.len() == 4 {
                return Some((p[2], p[3]));
            }
        }
        return Some((300.0, 150.0));
    }
    let (w, h) = image::image_dimensions(path).ok()?;
    Some((w as f32, h as f32))
}

/// Arranges the root instances of a composition on a canvas `width` wide. The canvas is
/// `height` tall when `height` is not `None` (auto).
pub fn layout(roots: &[Node], width: f32, height: Option<f32>, fonts: &mut Fonts) -> Laid {
    let mut b = Builder { tree: TaffyTree::new(), paras: Vec::new(), fonts, natural: HashMap::new() };
    b.tree.disable_rounding();
    let mut root_style = taffy::Style::default();
    root_style.display = Display::Flex;
    root_style.flex_direction = FlexDirection::Column;
    root_style.align_items = AlignItems::STRETCH;
    root_style.size = Size { width: Dimension::length(width), height: height.map(Dimension::length).unwrap_or(Dimension::auto()) };
    root_style.position = Position::Relative;
    let root = b.tree.new_leaf(root_style).unwrap();
    let mut items = Vec::new();
    for n in roots {
        let child = b.element(n, true, root);
        b.attach(root, root, &child);
        items.push(LItem::Box(child));
    }
    let avail = Size { width: AvailableSpace::Definite(width), height: height.map(AvailableSpace::Definite).unwrap_or(AvailableSpace::MaxContent) };
    let Builder { mut tree, paras, fonts, .. } = b;
    let mut cache: HashMap<(usize, u32), (f32, f32)> = HashMap::new();
    tree.compute_layout_with_measure(root, avail, |inputs, _id, ctx, style| {
        let ctx: Option<&Ctx> = ctx.map(|c| &*c);
        taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
            let Some(ctx) = ctx else { return Size::ZERO };
            match ctx {
                Ctx::Image(w, h) => {
                    let (w, h) = (*w, *h);
                    match (known.width, known.height) {
                        (Some(kw), Some(kh)) => Size { width: kw, height: kh },
                        (Some(kw), None) => Size { width: kw, height: if w > 0.0 { kw * h / w } else { 0.0 } },
                        (None, Some(kh)) => Size { width: if h > 0.0 { kh * w / h } else { 0.0 }, height: kh },
                        (None, None) => Size { width: w, height: h },
                    }
                }
                Ctx::Text(i) => {
                    let wlimit = known.width.or(match available.width {
                        AvailableSpace::Definite(w) => Some(w),
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    });
                    let key = (*i, wlimit.map(|w| w.to_bits()).unwrap_or(u32::MAX));
                    let (w, h) = *cache.entry(key).or_insert_with(|| {
                        let s = text::shape(fonts, &paras[*i], wlimit);
                        (s.width, s.height)
                    });
                    Size { width: known.width.unwrap_or(w), height: known.height.unwrap_or(h) }
                }
            }
        })
    })
    .unwrap();

    // The absolute positions of every taffy node.
    let mut abs: HashMap<NodeId, (f32, f32)> = HashMap::new();
    fn walk(tree: &TaffyTree<Ctx>, id: NodeId, origin: (f32, f32), abs: &mut HashMap<NodeId, (f32, f32)>) {
        let l = tree.layout(id).unwrap();
        let p = (origin.0 + l.location.x, origin.1 + l.location.y);
        abs.insert(id, p);
        for c in tree.children(id).unwrap() {
            walk(tree, c, p, abs);
        }
    }
    walk(&tree, root, (0.0, 0.0), &mut abs);
    let root_layout = tree.layout(root).unwrap();
    let canvas_h = height.unwrap_or(root_layout.size.height.ceil().max(1.0));

    fn fill(tree: &TaffyTree<Ctx>, abs: &HashMap<NodeId, (f32, f32)>, item: &mut LItem, fonts: &mut Fonts) {
        match item {
            LItem::Box(b) => {
                let l = tree.layout(b.tid).unwrap();
                let (x, y) = abs[&b.tid];
                b.rect = Rect { x, y, w: l.size.width, h: l.size.height };
                b.border = [l.border.top, l.border.right, l.border.bottom, l.border.left];
                b.padding = [l.padding.top, l.padding.right, l.padding.bottom, l.padding.left];
                for c in &mut b.children {
                    fill(tree, abs, c, fonts);
                }
            }
            LItem::Text(t) => {
                let l = tree.layout(t.tid).unwrap();
                let (x, y) = abs[&t.tid];
                t.rect = Rect { x, y, w: l.size.width, h: l.size.height };
                // A small tolerance. Then text that fit when the code measured it still fits.
                t.shaped = Some(text::shape(fonts, &t.para, Some(l.size.width + 0.01)));
            }
        }
    }
    for it in &mut items {
        fill(&tree, &abs, it, fonts);
    }
    Laid { width, height: canvas_h, items }
}
