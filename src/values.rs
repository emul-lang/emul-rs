//! Tokenizing values, and the typed values shared by many properties: lengths,
//! colours, gradients, shadows and positions.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// A number and its unit, which may be empty or `%`.
    Num(f64, String),
    Ident(String),
    Hash(String),
    Str(String),
    Func(String, Vec<Tok>),
    Url(String),
    Comma,
    Slash,
    /// `[a b]`, line names in grid templates.
    Names(Vec<String>),
}

pub fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let (toks, end) = tok_until(&chars, &mut i, None)?;
    let _ = end;
    Ok(toks)
}

fn tok_until(c: &[char], i: &mut usize, close: Option<char>) -> Result<(Vec<Tok>, bool), String> {
    let mut out = Vec::new();
    while *i < c.len() {
        let ch = c[*i];
        if Some(ch) == close {
            *i += 1;
            return Ok((out, true));
        }
        match ch {
            ' ' | '\t' => *i += 1,
            ',' => {
                out.push(Tok::Comma);
                *i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                *i += 1;
            }
            '"' | '\'' => {
                let q = ch;
                *i += 1;
                let mut s = String::new();
                while *i < c.len() && c[*i] != q {
                    s.push(c[*i]);
                    *i += 1;
                }
                if *i >= c.len() {
                    return Err("this quoted text is never closed".into());
                }
                *i += 1;
                out.push(Tok::Str(s));
            }
            '#' => {
                *i += 1;
                let mut s = String::new();
                while *i < c.len() && c[*i].is_ascii_alphanumeric() {
                    s.push(c[*i]);
                    *i += 1;
                }
                out.push(Tok::Hash(s));
            }
            '[' => {
                *i += 1;
                let mut s = String::new();
                while *i < c.len() && c[*i] != ']' {
                    s.push(c[*i]);
                    *i += 1;
                }
                if *i >= c.len() {
                    return Err("this `[` is never closed".into());
                }
                *i += 1;
                out.push(Tok::Names(s.split_whitespace().map(String::from).collect()));
            }
            c0 if c0.is_ascii_digit() || c0 == '.' || ((c0 == '-' || c0 == '+') && c.get(*i + 1).is_some_and(|n| n.is_ascii_digit() || *n == '.')) => {
                let start = *i;
                *i += 1;
                while *i < c.len() && (c[*i].is_ascii_digit() || c[*i] == '.') {
                    *i += 1;
                }
                // Exponents are not part of Emul values; stop at the unit.
                let num: String = c[start..*i].iter().collect();
                let v: f64 = num.parse().map_err(|_| format!("`{num}` is not a number"))?;
                let mut unit = String::new();
                if *i < c.len() && c[*i] == '%' {
                    unit.push('%');
                    *i += 1;
                } else {
                    while *i < c.len() && c[*i].is_ascii_alphabetic() {
                        unit.push(c[*i]);
                        *i += 1;
                    }
                }
                out.push(Tok::Num(v, unit));
            }
            c0 if c0.is_alphabetic() || c0 == '-' || c0 == '_' => {
                let start = *i;
                while *i < c.len() && (c[*i].is_alphanumeric() || c[*i] == '-' || c[*i] == '_') {
                    *i += 1;
                }
                let name: String = c[start..*i].iter().collect();
                if *i < c.len() && c[*i] == '(' {
                    *i += 1;
                    if name.eq_ignore_ascii_case("url") {
                        let mut s = String::new();
                        while *i < c.len() && c[*i] != ')' {
                            s.push(c[*i]);
                            *i += 1;
                        }
                        if *i >= c.len() {
                            return Err("`url(` is never closed".into());
                        }
                        *i += 1;
                        let s = s.trim().trim_matches(['"', '\'']).to_string();
                        out.push(Tok::Url(s));
                    } else {
                        let (inner, closed) = tok_until(c, i, Some(')'))?;
                        if !closed {
                            return Err(format!("`{name}(` is never closed"));
                        }
                        out.push(Tok::Func(name.to_ascii_lowercase(), inner));
                    }
                } else {
                    out.push(Tok::Ident(name));
                }
            }
            other => return Err(format!("unexpected `{other}`")),
        }
    }
    if close.is_some() {
        return Ok((out, false));
    }
    Ok((out, true))
}

/// Splits tokens at top-level commas.
pub fn split_commas(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut out = vec![Vec::new()];
    for t in toks {
        if *t == Tok::Comma {
            out.push(Vec::new());
        } else {
            out.last_mut().unwrap().push(t.clone());
        }
    }
    out
}

pub fn fmt_num(v: f64) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Serializes tokens back to CSS text, normalizing spaces.
pub fn tokens_css(toks: &[Tok]) -> String {
    let mut s = String::new();
    for (i, t) in toks.iter().enumerate() {
        let piece = match t {
            Tok::Num(v, u) => format!("{}{}", fmt_num(*v), u),
            Tok::Ident(x) => x.clone(),
            Tok::Hash(h) => format!("#{h}"),
            Tok::Str(x) => format!("\"{}\"", x.replace('"', "\\\"")),
            Tok::Func(n, inner) => format!("{}({})", n, tokens_css(inner)),
            Tok::Url(u) => format!("url(\"{u}\")"),
            Tok::Comma => ",".into(),
            Tok::Slash => "/".into(),
            Tok::Names(n) => format!("[{}]", n.join(" ")),
        };
        if i > 0 && *t != Tok::Comma {
            s.push(' ');
        }
        s.push_str(&piece);
    }
    s
}

// ---------------------------------------------------------------- lengths

/// A length or percentage, possibly `auto` or `none`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Auto,
    None,
    Px(f32),
    Pct(f32),
}

impl Len {
    pub fn css(&self) -> String {
        match self {
            Len::Auto => "auto".into(),
            Len::None => "none".into(),
            Len::Px(v) => format!("{}px", fmt_num(*v as f64)),
            Len::Pct(v) => format!("{}%", fmt_num(*v as f64)),
        }
    }
    pub fn resolve(&self, basis: f32) -> Option<f32> {
        match self {
            Len::Px(v) => Some(*v),
            Len::Pct(p) => Some(p / 100.0 * basis),
            _ => None,
        }
    }
}

pub struct LenRules {
    pub auto: bool,
    pub none: bool,
    pub pct: bool,
    pub negative: bool,
}

pub const LEN_ANY: LenRules = LenRules { auto: true, none: false, pct: true, negative: true };

pub fn parse_len(t: &Tok, r: &LenRules) -> Result<Len, String> {
    match t {
        Tok::Ident(k) if k == "auto" && r.auto => Ok(Len::Auto),
        Tok::Ident(k) if k == "none" && r.none => Ok(Len::None),
        Tok::Num(v, u) => {
            if *v < 0.0 && !r.negative {
                return Err("negative values are not allowed here".into());
            }
            match u.as_str() {
                "px" => Ok(Len::Px(*v as f32)),
                "%" if r.pct => Ok(Len::Pct(*v as f32)),
                "%" => Err("a percentage is not accepted here; use px".into()),
                "" if *v == 0.0 => Ok(Len::Px(0.0)),
                "" => Err(format!("`{}` needs a unit, px{}", fmt_num(*v), if r.pct { " or %" } else { "" })),
                other => Err(format!("`{other}` is not an Emul unit; sizes use px{}", if r.pct { " or %" } else { "" })),
            }
        }
        other => Err(format!("`{}` is not a size", tokens_css(std::slice::from_ref(other)))),
    }
}

// ---------------------------------------------------------------- colours

#[derive(Clone, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    /// The colour as written, for the HTML output.
    pub css: String,
}

impl Color {
    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        let mut c = Color { r, g, b, a, css: String::new() };
        c.css = c.hex();
        c
    }
    pub fn black() -> Color {
        Color::rgba(0.0, 0.0, 0.0, 1.0)
    }
    /// `#rrggbb`, ignoring alpha.
    pub fn hex6(&self) -> String {
        let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", to(self.r), to(self.g), to(self.b))
    }
    pub fn hex(&self) -> String {
        if self.a >= 1.0 {
            self.hex6()
        } else {
            format!("{}{:02x}", self.hex6(), (self.a.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
    }
    pub fn with_alpha_mul(&self, m: f32) -> Color {
        let mut c = self.clone();
        c.a *= m;
        c
    }
}

fn hexval(h: &str) -> Option<u8> {
    u8::from_str_radix(h, 16).ok()
}

pub fn parse_color(t: &Tok) -> Result<Color, String> {
    let css = tokens_css(std::slice::from_ref(t));
    let mut c = match t {
        Tok::Hash(h) => {
            let h = h.to_ascii_lowercase();
            let digits: Vec<String> = match h.len() {
                3 | 4 => h.chars().map(|c| format!("{c}{c}")).collect(),
                6 | 8 => (0..h.len() / 2).map(|i| h[i * 2..i * 2 + 2].to_string()).collect(),
                _ => return Err(format!("`#{h}` needs 3, 4, 6 or 8 hex digits")),
            };
            let v: Option<Vec<u8>> = digits.iter().map(|d| hexval(d)).collect();
            let v = v.ok_or_else(|| format!("`#{h}` is not a hex colour"))?;
            let f = |x: u8| x as f32 / 255.0;
            Color { r: f(v[0]), g: f(v[1]), b: f(v[2]), a: v.get(3).map(|&x| f(x)).unwrap_or(1.0), css: String::new() }
        }
        Tok::Func(name, args) => {
            let parts = color_args(args)?;
            match name.as_str() {
                "rgb" | "rgba" => {
                    if parts.len() < 3 || parts.len() > 4 {
                        return Err(format!("`{name}()` takes three channels and an optional alpha"));
                    }
                    let ch = |t: &Tok| -> Result<f32, String> {
                        match t {
                            Tok::Num(v, u) if u.is_empty() => Ok((*v as f32 / 255.0).clamp(0.0, 1.0)),
                            Tok::Num(v, u) if u == "%" => Ok((*v as f32 / 100.0).clamp(0.0, 1.0)),
                            _ => Err("rgba channels are numbers from 0 to 255, or percentages".into()),
                        }
                    };
                    let a = parts.get(3).map(alpha).transpose()?.unwrap_or(1.0);
                    Color { r: ch(&parts[0])?, g: ch(&parts[1])?, b: ch(&parts[2])?, a, css: String::new() }
                }
                "hsl" | "hsla" => {
                    if parts.len() < 3 || parts.len() > 4 {
                        return Err(format!("`{name}()` takes hue, saturation, lightness and an optional alpha"));
                    }
                    let h = angle_or_number(&parts[0])?;
                    let pct = |t: &Tok| -> Result<f32, String> {
                        match t {
                            Tok::Num(v, u) if u == "%" || u.is_empty() => Ok((*v as f32 / 100.0).clamp(0.0, 1.0)),
                            _ => Err("hsl saturation and lightness are percentages".into()),
                        }
                    };
                    let (r, g, b) = hsl_to_rgb(h, pct(&parts[1])?, pct(&parts[2])?);
                    let a = parts.get(3).map(alpha).transpose()?.unwrap_or(1.0);
                    Color { r, g, b, a, css: String::new() }
                }
                "oklch" => {
                    if parts.len() < 3 || parts.len() > 4 {
                        return Err("`oklch()` takes lightness, chroma, hue and an optional alpha".into());
                    }
                    let l = match &parts[0] {
                        Tok::Num(v, u) if u == "%" => *v as f32 / 100.0,
                        Tok::Num(v, u) if u.is_empty() => *v as f32,
                        _ => return Err("oklch lightness is a number or a percentage".into()),
                    };
                    let ch = match &parts[1] {
                        Tok::Num(v, u) if u == "%" => *v as f32 / 100.0 * 0.4,
                        Tok::Num(v, u) if u.is_empty() => *v as f32,
                        _ => return Err("oklch chroma is a number or a percentage".into()),
                    };
                    let h = angle_or_number(&parts[2])?;
                    let (r, g, b) = oklch_to_srgb(l.clamp(0.0, 1.0), ch.max(0.0), h);
                    let a = parts.get(3).map(alpha).transpose()?.unwrap_or(1.0);
                    Color { r, g, b, a, css: String::new() }
                }
                other => return Err(format!("`{other}()` is not a colour; use hex, rgba(), hsl() or oklch()")),
            }
        }
        Tok::Ident(n) => {
            return Err(format!("`{n}` is not accepted as a colour; use hex, rgba(), hsl() or oklch()"));
        }
        _ => return Err(format!("`{css}` is not a colour")),
    };
    c.css = css;
    Ok(c)
}

/// Colour function arguments in either comma or space form, with alpha after `/`.
fn color_args(args: &[Tok]) -> Result<Vec<Tok>, String> {
    if args.contains(&Tok::Comma) {
        let parts = split_commas(args);
        let mut out = Vec::new();
        for p in parts {
            if p.len() != 1 {
                return Err("each colour channel is a single number".into());
            }
            out.push(p[0].clone());
        }
        Ok(out)
    } else {
        Ok(args.iter().filter(|t| **t != Tok::Slash).cloned().collect())
    }
}

fn alpha(t: &Tok) -> Result<f32, String> {
    match t {
        Tok::Num(v, u) if u.is_empty() => Ok((*v as f32).clamp(0.0, 1.0)),
        Tok::Num(v, u) if u == "%" => Ok((*v as f32 / 100.0).clamp(0.0, 1.0)),
        _ => Err("alpha is a number from 0 to 1, or a percentage".into()),
    }
}

fn angle_or_number(t: &Tok) -> Result<f32, String> {
    match t {
        Tok::Num(v, u) if u.is_empty() || u == "deg" => Ok(*v as f32),
        _ => Err("a hue is a number of degrees".into()),
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 360.0;
    let f = |n: f32| {
        let k = (n + h * 12.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
}

fn oklch_to_srgb(l: f32, c: f32, h: f32) -> (f32, f32, f32) {
    let hr = h.to_radians();
    let a = c * hr.cos();
    let b = c * hr.sin();
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let enc = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
    };
    (enc(r), enc(g), enc(bl))
}

// ---------------------------------------------------------------- shadows

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub inset: bool,
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    /// `None` means the component's own `color`.
    pub color: Option<Color>,
}

pub fn parse_shadows(toks: &[Tok], allow_spread: bool) -> Result<Vec<Shadow>, String> {
    let mut out = Vec::new();
    for part in split_commas(toks) {
        let mut inset = false;
        let mut color = None;
        let mut lens = Vec::new();
        for t in &part {
            match t {
                Tok::Ident(k) if k == "inset" && allow_spread => inset = true,
                Tok::Num(..) => match parse_len(t, &LenRules { auto: false, none: false, pct: false, negative: true })? {
                    Len::Px(v) => lens.push(v),
                    _ => unreachable!(),
                },
                _ => color = Some(parse_color(t)?),
            }
        }
        let max = if allow_spread { 4 } else { 3 };
        if lens.len() < 2 || lens.len() > max {
            return Err(format!("a shadow needs an x and y offset, then an optional blur{}", if allow_spread { " and spread" } else { "" }));
        }
        if lens.get(2).is_some_and(|b| *b < 0.0) {
            return Err("a shadow's blur can't be negative".into());
        }
        out.push(Shadow {
            inset,
            x: lens[0],
            y: lens[1],
            blur: lens.get(2).copied().unwrap_or(0.0),
            spread: lens.get(3).copied().unwrap_or(0.0),
            color,
        });
    }
    Ok(out)
}

pub fn shadows_css(v: &[Shadow], own_color: &Color) -> String {
    v.iter()
        .map(|s| {
            let mut p = String::new();
            if s.inset {
                p.push_str("inset ");
            }
            let _ = write!(p, "{}px {}px {}px", fmt_num(s.x as f64), fmt_num(s.y as f64), fmt_num(s.blur as f64));
            if s.spread != 0.0 {
                let _ = write!(p, " {}px", fmt_num(s.spread as f64));
            }
            let c = s.color.as_ref().unwrap_or(own_color);
            let _ = write!(p, " {}", c.css);
            p
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------- positions

/// One axis of a position, as a fraction of the free space plus an offset in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosAxis {
    pub pct: f32,
    pub px: f32,
}

impl PosAxis {
    pub fn css(&self) -> String {
        if self.px == 0.0 {
            format!("{}%", fmt_num(self.pct as f64))
        } else if self.pct == 0.0 {
            format!("{}px", fmt_num(self.px as f64))
        } else {
            format!("calc({}% + {}px)", fmt_num(self.pct as f64), fmt_num(self.px as f64))
        }
    }
    /// Places something `size` long inside `container`.
    pub fn place(&self, container: f32, size: f32) -> f32 {
        (container - size) * self.pct / 100.0 + self.px
    }
}

pub fn parse_position(toks: &[Tok]) -> Result<(PosAxis, PosAxis), String> {
    let axis = |t: &Tok| -> Result<(Option<char>, PosAxis), String> {
        match t {
            Tok::Ident(k) => match k.as_str() {
                "left" => Ok((Some('x'), PosAxis { pct: 0.0, px: 0.0 })),
                "right" => Ok((Some('x'), PosAxis { pct: 100.0, px: 0.0 })),
                "top" => Ok((Some('y'), PosAxis { pct: 0.0, px: 0.0 })),
                "bottom" => Ok((Some('y'), PosAxis { pct: 100.0, px: 0.0 })),
                "center" => Ok((None, PosAxis { pct: 50.0, px: 0.0 })),
                _ => Err(format!("`{k}` is not a position")),
            },
            Tok::Num(..) => match parse_len(t, &LenRules { auto: false, none: false, pct: true, negative: true })? {
                Len::Px(v) => Ok((None, PosAxis { pct: 0.0, px: v })),
                Len::Pct(v) => Ok((None, PosAxis { pct: v, px: 0.0 })),
                _ => unreachable!(),
            },
            _ => Err("a position is a keyword, a length or a percentage".into()),
        }
    };
    let center = PosAxis { pct: 50.0, px: 0.0 };
    match toks {
        [a] => {
            let (k, v) = axis(a)?;
            Ok(if k == Some('y') { (center, v) } else { (v, center) })
        }
        [a, b] => {
            let (ka, va) = axis(a)?;
            let (kb, vb) = axis(b)?;
            if ka == Some('y') || kb == Some('x') { Ok((vb, va)) } else { Ok((va, vb)) }
        }
        _ => Err("a position has one or two parts".into()),
    }
}

// ---------------------------------------------------------------- images

#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub color: Color,
    pub pos: Option<Len>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    Url(std::path::PathBuf),
    /// Angle in degrees, CSS convention (0 points up, clockwise).
    Linear { angle: LinearDir, stops: Vec<Stop> },
    Radial { circle: bool, at: (PosAxis, PosAxis), stops: Vec<Stop> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinearDir {
    Angle(f32),
    /// A `to` corner, which depends on the box's proportions: (x sign, y sign).
    Corner(f32, f32),
}

pub fn parse_stops(parts: &[Vec<Tok>]) -> Result<Vec<Stop>, String> {
    let mut stops = Vec::new();
    for p in parts {
        match p.as_slice() {
            [c] => stops.push(Stop { color: parse_color(c)?, pos: None }),
            [c, pos] => stops.push(Stop {
                color: parse_color(c)?,
                pos: Some(parse_len(pos, &LenRules { auto: false, none: false, pct: true, negative: true })?),
            }),
            _ => return Err("a colour stop is a colour and an optional position".into()),
        }
    }
    if stops.len() < 2 {
        return Err("a gradient needs at least two colour stops".into());
    }
    Ok(stops)
}

pub fn parse_gradient(name: &str, args: &[Tok]) -> Result<Image, String> {
    let parts = split_commas(args);
    match name {
        "linear-gradient" => {
            let (dir, rest) = match parts[0].first() {
                Some(Tok::Num(v, u)) if u == "deg" && parts[0].len() == 1 => (LinearDir::Angle(*v as f32), &parts[1..]),
                Some(Tok::Ident(k)) if k == "to" => {
                    let mut x = 0.0;
                    let mut y = 0.0;
                    for t in &parts[0][1..] {
                        match t {
                            Tok::Ident(s) if s == "left" => x = -1.0,
                            Tok::Ident(s) if s == "right" => x = 1.0,
                            Tok::Ident(s) if s == "top" => y = -1.0,
                            Tok::Ident(s) if s == "bottom" => y = 1.0,
                            _ => return Err("`to` is followed by top, bottom, left or right".into()),
                        }
                    }
                    let dir = if x != 0.0 && y != 0.0 {
                        LinearDir::Corner(x, y)
                    } else if x != 0.0 {
                        LinearDir::Angle(if x > 0.0 { 90.0 } else { 270.0 })
                    } else {
                        LinearDir::Angle(if y > 0.0 { 180.0 } else { 0.0 })
                    };
                    (dir, &parts[1..])
                }
                _ => (LinearDir::Angle(180.0), &parts[..]),
            };
            Ok(Image::Linear { angle: dir, stops: parse_stops(rest)? })
        }
        "radial-gradient" => {
            let first = &parts[0];
            let is_shape = first.iter().any(|t| matches!(t, Tok::Ident(k) if k == "circle" || k == "ellipse" || k == "at"));
            let (mut circle, mut at) = (false, (PosAxis { pct: 50.0, px: 0.0 }, PosAxis { pct: 50.0, px: 0.0 }));
            let rest = if is_shape {
                let at_i = first.iter().position(|t| matches!(t, Tok::Ident(k) if k == "at"));
                let shape = &first[..at_i.unwrap_or(first.len())];
                for t in shape {
                    match t {
                        Tok::Ident(k) if k == "circle" => circle = true,
                        Tok::Ident(k) if k == "ellipse" => circle = false,
                        _ => return Err("a radial gradient's shape is `circle` or `ellipse`".into()),
                    }
                }
                if let Some(i) = at_i {
                    at = parse_position(&first[i + 1..])?;
                }
                &parts[1..]
            } else {
                &parts[..]
            };
            Ok(Image::Radial { circle, at, stops: parse_stops(rest)? })
        }
        _ => Err(format!("`{name}()` is not an image")),
    }
}

pub fn stops_css(stops: &[Stop]) -> String {
    stops
        .iter()
        .map(|s| match &s.pos {
            Some(p) => format!("{} {}", s.color.css, p.css()),
            None => s.color.css.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolves stop positions along a gradient line `len` px long, as offsets from 0 to 1,
/// filling in missing positions as CSS does.
pub fn resolve_stops(stops: &[Stop], len: f32) -> Vec<(f32, Color)> {
    let n = stops.len();
    let mut pos: Vec<Option<f32>> = stops
        .iter()
        .map(|s| s.pos.map(|p| match p {
            Len::Px(v) => if len > 0.0 { v / len } else { 0.0 },
            Len::Pct(v) => v / 100.0,
            _ => 0.0,
        }))
        .collect();
    if pos[0].is_none() {
        pos[0] = Some(0.0);
    }
    if pos[n - 1].is_none() {
        pos[n - 1] = Some(1.0);
    }
    // Positions never go backwards.
    let mut maxp = 0.0f32;
    for p in pos.iter_mut().flatten() {
        if *p < maxp {
            *p = maxp;
        }
        maxp = *p;
    }
    let mut i = 0;
    while i < n {
        if pos[i].is_none() {
            let start = i - 1;
            let mut end = i;
            while pos[end].is_none() {
                end += 1;
            }
            let (a, b) = (pos[start].unwrap(), pos[end].unwrap());
            for k in start + 1..end {
                pos[k] = Some(a + (b - a) * (k - start) as f32 / (end - start) as f32);
            }
            i = end;
        }
        i += 1;
    }
    stops.iter().zip(pos).map(|(s, p)| (p.unwrap(), s.color.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        let c = parse_color(&tokenize("#ff000080").unwrap()[0]).unwrap();
        assert!((c.a - 0.5).abs() < 0.01 && c.r == 1.0);
        let c = parse_color(&tokenize("rgba(0, 128, 255, 0.5)").unwrap()[0]).unwrap();
        assert_eq!(c.hex(), "#0080ff80");
        let c = parse_color(&tokenize("rgb(0 128 255 / 50%)").unwrap()[0]).unwrap();
        assert_eq!(c.hex(), "#0080ff80");
        let c = parse_color(&tokenize("hsl(0, 100%, 50%)").unwrap()[0]).unwrap();
        assert_eq!(c.hex6(), "#ff0000");
        let c = parse_color(&tokenize("oklch(0.628 0.2577 29.23)").unwrap()[0]).unwrap();
        assert_eq!(c.hex6(), "#ff0000");
        assert!(parse_color(&tokenize("red").unwrap()[0]).is_err());
    }

    #[test]
    fn tokens() {
        let t = tokenize("0 4px 12px rgba(0, 0, 0, 0.08)").unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!(tokens_css(&t), "0 4px 12px rgba(0, 0, 0, 0.08)");
    }
}
