//! The source formatter: rewrites an `.emul` file into its one canonical layout, as
//! set out in `spec/Formatting.pi`. Formatting never changes what a file means, and
//! formatting an already formatted file changes nothing.

use crate::ast::*;
use crate::diag::Diags;
use crate::escape;
use crate::parser;
use std::path::Path;

const INDENT: &str = "    ";
const WIDTH: usize = 80;

/// Returns the canonically formatted form of `src`. A file with syntax errors is not
/// formatted; its diagnostics are returned instead.
pub fn format_source(path: &Path, src: &str) -> Result<String, Diags> {
    let parsed = parser::parse(path, src);
    if parsed.diags.has_errors() {
        return Err(parsed.diags);
    }
    let f = Formatter { src, line_starts: line_starts(src) };
    Ok(f.file(&parsed.file.items))
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut v = vec![0];
    v.extend(src.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1));
    v
}

/// A top-level declaration with the comments written above it. In `lead`, `None`
/// stands for a blank line the author left among those comments.
struct Unit<'a> {
    lead: Vec<Option<&'a Comment>>,
    item: Option<&'a Item>,
    /// Whether a blank line came before this unit in the source.
    blank_before: bool,
}

struct Formatter<'a> {
    src: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> Formatter<'a> {
    fn file(&self, items: &'a [Item]) -> String {
        let mut units = group_units(items);

        // Comments at the very top, separated from what follows by a blank line, are a
        // file header: they stay first, above the imports.
        let mut header: Vec<Option<&Comment>> = Vec::new();
        if let Some(first) = units.first_mut() {
            if first.item.is_none() || matches!(first.lead.last(), Some(None)) {
                header = std::mem::take(&mut first.lead);
                while matches!(header.last(), Some(None)) {
                    header.pop();
                }
            }
        }
        units.retain(|u| u.item.is_some() || !u.lead.is_empty());

        let mut imports: Vec<&Unit> = units.iter().filter(|u| matches!(u.item, Some(Item::Import(_)))).collect();
        imports.sort_by_key(|u| match u.item {
            Some(Item::Import(im)) => strip_ext(&im.path).to_string(),
            _ => unreachable!(),
        });
        let rest: Vec<&Unit> = units.iter().filter(|u| !matches!(u.item, Some(Item::Import(_)))).collect();

        let mut out: Vec<String> = Vec::new();
        lead_lines(&header, "", &mut out);
        if !out.is_empty() && !imports.is_empty() {
            out.push(String::new());
        }
        for u in &imports {
            lead_lines(&u.lead, "", &mut out);
            if let Some(item) = u.item {
                out.extend(self.item(item));
            }
        }
        let mut prev: Option<&Unit> = None;
        for u in rest {
            if !out.is_empty() {
                // Runs of re-exports and `export` lines keep the author's grouping; every
                // other declaration is set apart by one blank line.
                let together = prev.is_some_and(|p| is_export_line(p.item))
                    && is_export_line(u.item)
                    && u.lead.is_empty()
                    && !u.blank_before;
                if !together {
                    out.push(String::new());
                }
            }
            lead_lines(&u.lead, "", &mut out);
            if let Some(item) = u.item {
                out.extend(self.item(item));
            }
            prev = Some(u);
        }
        finish(out)
    }

    fn item(&self, item: &Item) -> Vec<String> {
        match item {
            Item::Import(im) => from_lines(&im.path, "import", Some(&im.names)),
            Item::ReExport(r) => from_lines(&r.path, "export", r.names.as_deref()),
            Item::ExportNames(e) => {
                let names: Vec<&str> = e.names.iter().map(|(n, _)| n.as_str()).collect();
                vec![format!("export {}", names.join(", "))]
            }
            Item::Component(c) => component_lines(c),
            Item::Composition(c) => self.composition_lines(c),
            Item::Blank | Item::Comment(_) => Vec::new(),
        }
    }

    fn composition_lines(&self, c: &Composition) -> Vec<String> {
        let head = format!("{}composition {}", if c.exported { "export " } else { "" }, c.name);
        // The parser drops a comment written after the closing brace, so it is
        // recovered from the source and kept there.
        let tail = match self.trailing_comment(c) {
            Some(t) => format!(" {}", comment_text(&t)),
            None => String::new(),
        };
        let body = normalize_blanks(&c.body);
        if body.is_empty() {
            return vec![format!("{head} {{}}{tail}")];
        }
        let mut lines = vec![format!("{head} {{")];
        body_lines(&body, 1, &mut lines);
        lines.push(format!("}}{tail}"));
        lines
    }

    /// Byte offset of a 1-based line and character column.
    fn offset(&self, line: usize, col: usize) -> usize {
        let start = self.line_starts[line - 1];
        start + self.src[start..].char_indices().nth(col - 1).map(|(i, _)| i).unwrap_or(0)
    }

    fn trailing_comment(&self, c: &Composition) -> Option<String> {
        let src = self.src;
        let bytes = src.as_bytes();
        let mut k = self.offset(c.name_span.line, c.name_span.col) + c.name.len();
        while k < src.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        if bytes.get(k) != Some(&b'{') {
            return None;
        }
        let end = scan::body_end(src, k + 1)?;
        let tail = src[end..scan::line_end(src, end)].trim();
        tail.strip_prefix("//").map(|t| t.trim().to_string())
    }
}

fn group_units(items: &[Item]) -> Vec<Unit<'_>> {
    let mut units = Vec::new();
    let mut lead: Vec<Option<&Comment>> = Vec::new();
    let mut blank_pending = false;
    let mut blank_before = false;
    for item in items {
        match item {
            Item::Blank => {
                if lead.is_empty() {
                    blank_pending = true;
                } else if !matches!(lead.last(), Some(None)) {
                    lead.push(None);
                }
            }
            Item::Comment(c) => {
                if lead.is_empty() {
                    blank_before = blank_pending;
                    blank_pending = false;
                }
                lead.push(Some(c));
            }
            other => {
                if lead.is_empty() {
                    blank_before = blank_pending;
                }
                blank_pending = false;
                units.push(Unit { lead: std::mem::take(&mut lead), item: Some(other), blank_before });
            }
        }
    }
    while matches!(lead.last(), Some(None)) {
        lead.pop();
    }
    if !lead.is_empty() {
        units.push(Unit { lead, item: None, blank_before });
    }
    units
}

fn is_export_line(item: Option<&Item>) -> bool {
    matches!(item, Some(Item::ReExport(_) | Item::ExportNames(_)))
}

fn lead_lines(lead: &[Option<&Comment>], indent: &str, out: &mut Vec<String>) {
    for c in lead {
        match c {
            Some(c) => out.push(format!("{indent}{}", comment_text(&c.text))),
            None => out.push(String::new()),
        }
    }
}

fn comment_text(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() { "//".to_string() } else { format!("// {text}") }
}

fn finish(lines: Vec<String>) -> String {
    let mut s = String::new();
    let mut last_blank = true;
    for l in lines {
        let l = l.trim_end();
        if l.is_empty() {
            if last_blank {
                continue;
            }
            last_blank = true;
        } else {
            last_blank = false;
        }
        s.push_str(l);
        s.push('\n');
    }
    while s.ends_with("\n\n") {
        s.pop();
    }
    if s == "\n" {
        s.clear();
    }
    s
}

fn strip_ext(path: &str) -> &str {
    match path.strip_suffix(".emul") {
        Some(p) if !p.is_empty() && !p.ends_with('/') => p,
        _ => path,
    }
}

/// `from PATH import|export names`, split one name per line when it has more than
/// two names or would pass the line width. `None` names means `*`.
fn from_lines(path: &str, kw: &str, names: Option<&[ImportName]>) -> Vec<String> {
    let path = strip_ext(path);
    let Some(names) = names else {
        return vec![format!("from {path} {kw} *")];
    };
    let mut names: Vec<String> = {
        let mut v: Vec<&ImportName> = names.iter().collect();
        v.sort_by(|a, b| (&a.name, &a.alias).cmp(&(&b.name, &b.alias)));
        v.iter()
            .map(|n| match &n.alias {
                Some(a) => format!("{} {a}", n.name),
                None => n.name.clone(),
            })
            .collect()
    };
    let one = format!("from {path} {kw} {}", names.join(", "));
    if names.len() <= 2 && one.chars().count() <= WIDTH {
        return vec![one];
    }
    let mut lines = vec![format!("from {path} {kw}")];
    lines.extend(names.drain(..).map(|n| format!("{INDENT}{n}")));
    lines
}

fn component_lines(c: &Component) -> Vec<String> {
    let mut head = format!("{}component {}", if c.exported { "export " } else { "" }, c.name);
    if !c.extends.is_empty() {
        let names: Vec<&str> = c.extends.iter().map(|(n, _)| n.as_str()).collect();
        head.push_str(" extends ");
        head.push_str(&names.join(", "));
    }
    head.push(':');
    let mut lines = vec![head];
    let mut last_blank = true;
    for item in &c.body {
        match item {
            ComponentItem::Blank => {
                if !last_blank {
                    lines.push(String::new());
                }
                last_blank = true;
                continue;
            }
            ComponentItem::Comment(cm) => lines.push(format!("{INDENT}{}", comment_text(&cm.text))),
            ComponentItem::Property(p) => lines.push(format!("{INDENT}{}: {}", p.name, format_value(&p.raw))),
        }
        last_blank = false;
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Drops leading, trailing and repeated blank entries. Returns an empty list for a body
/// with nothing but blank lines in it.
fn normalize_blanks(entries: &[Entry]) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = Vec::new();
    for e in entries {
        if matches!(e, Entry::Blank) && (v.is_empty() || matches!(v.last(), Some(Entry::Blank))) {
            continue;
        }
        v.push(e);
    }
    while matches!(v.last(), Some(Entry::Blank)) {
        v.pop();
    }
    v
}

fn body_lines(entries: &[&Entry], depth: usize, out: &mut Vec<String>) {
    let ind = INDENT.repeat(depth);
    for e in entries {
        match e {
            Entry::Blank => out.push(String::new()),
            Entry::Comment(c) => out.push(format!("{ind}{}", comment_text(&c.text))),
            Entry::Text(t) => out.push(format!("{ind}{}", t.raw)),
            Entry::Attribute(a) => out.push(format!("{ind}{}", attribute(a))),
            Entry::Instance(i) => instance_lines(i, depth, out),
        }
    }
}

/// Attribute values are written exactly as given (trimmed). They are HTML attribute
/// values such as paths and URLs, so the value spacing rules would change them.
fn attribute(a: &Attribute) -> String {
    format!("{}: {}", a.name, a.raw.trim())
}

fn instance_lines(inst: &Instance, depth: usize, out: &mut Vec<String>) {
    let ind = INDENT.repeat(depth);
    let body = normalize_blanks(&inst.body);
    if body.is_empty() {
        out.push(format!("{ind}{} {{}}", inst.name));
        return;
    }
    if let Some(one) = one_line(&inst.name, &body) {
        if ind.len() + one.chars().count() <= WIDTH {
            out.push(format!("{ind}{one}"));
            return;
        }
    }
    out.push(format!("{ind}{} {{", inst.name));
    body_lines(&body, depth + 1, out);
    out.push(format!("{ind}}}"));
}

/// The one-line form of a body, if it may have one: no children and no comments.
fn one_line(name: &str, body: &[&Entry]) -> Option<String> {
    let mut parts = Vec::new();
    for e in body {
        match e {
            Entry::Blank => {}
            Entry::Text(t) => parts.push(t.raw.clone()),
            Entry::Attribute(a) => {
                // A value ending in a backslash followed by ` }` would open an escape.
                if a.raw.trim_end().ends_with('\\') {
                    return None;
                }
                parts.push(attribute(a));
            }
            Entry::Comment(_) | Entry::Instance(_) => return None,
        }
    }
    Some(format!("{name} {{ {} }}", parts.join(", ")))
}

// ---------------------------------------------------------------------------------
// Values

#[derive(Debug)]
enum Vt {
    /// Anything that is not spacing or punctuation: words, numbers, quoted text,
    /// escapes, `url(...)` and `[...]`, all kept as written.
    Word(String),
    Space,
    Comma,
    Slash(usize),
    Open,
    Close,
}

/// Formats a property value: single spaces, `, ` after commas, ` / ` around slashes,
/// no space before or just inside a function's parentheses, lowercase hex colours.
/// Quoted text, escapes, `url(...)` and `[...]` are kept byte for byte.
pub(crate) fn format_value(raw: &str) -> String {
    let raw = raw.trim();
    let Some(toks) = value_tokens(raw) else {
        return raw.to_string();
    };
    let out = emit_value(&toks);
    // Safety net: the escapes must come out exactly as they went in.
    if wrappers(&out) != wrappers(raw) || escape::unescape(&out).is_err() {
        return raw.to_string();
    }
    out
}

fn wrappers(s: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if let Ok(Some((end, _))) = escape::scan_wrapper(s, i) {
            v.push(&s[i..end]);
            i = end;
        } else {
            i += s[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    v
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Whether a word ends in a name that can be a function's, such as `rgba`.
fn ends_in_name(w: &str) -> bool {
    let seg: String = w.chars().rev().take_while(|&c| is_ident_char(c)).collect();
    seg.chars().last().is_some_and(char::is_alphabetic)
}

fn ends_in_url(w: &str) -> bool {
    let seg: String = w.chars().rev().take_while(|&c| is_ident_char(c)).collect::<String>().chars().rev().collect();
    seg.eq_ignore_ascii_case("url")
}

/// Returns `None` when the value holds a backslash that is not part of an escape;
/// such a value is left exactly as written.
fn value_tokens(raw: &str) -> Option<Vec<Vt>> {
    let bytes = raw.as_bytes();
    let mut toks = Vec::new();
    let mut word = String::new();
    fn flush(word: &mut String, toks: &mut Vec<Vt>) {
        if !word.is_empty() {
            toks.push(Vt::Word(std::mem::take(word)));
        }
    }
    let mut i = 0;
    while i < raw.len() {
        let ch = raw[i..].chars().next().unwrap();
        match ch {
            '\\' => match escape::scan_wrapper(raw, i) {
                Ok(Some((end, _))) => {
                    word.push_str(&raw[i..end]);
                    i = end;
                }
                _ => return None,
            },
            '"' | '\'' => {
                let mut j = i + 1;
                loop {
                    if j >= raw.len() {
                        word.push_str(&raw[i..]);
                        i = raw.len();
                        break;
                    }
                    if bytes[j] == b'\\' {
                        if let Ok(Some((end, _))) = escape::scan_wrapper(raw, j) {
                            j = end;
                            continue;
                        }
                    }
                    let c = raw[j..].chars().next().unwrap();
                    if c == ch {
                        word.push_str(&raw[i..=j]);
                        i = j + 1;
                        break;
                    }
                    j += c.len_utf8();
                }
            }
            ' ' | '\t' => {
                flush(&mut word, &mut toks);
                toks.push(Vt::Space);
                i += 1;
            }
            ',' => {
                flush(&mut word, &mut toks);
                toks.push(Vt::Comma);
                i += 1;
            }
            '/' => {
                flush(&mut word, &mut toks);
                let n = raw[i..].bytes().take_while(|&b| b == b'/').count();
                toks.push(Vt::Slash(n));
                i += n;
            }
            '(' => {
                // `url(` keeps its contents as written, up to the first `)`.
                if word.is_empty() && matches!(toks.last(), Some(Vt::Space)) {
                    if let Some(Vt::Word(w)) = toks.len().checked_sub(2).and_then(|k| toks.get(k)) {
                        if ends_in_url(w) {
                            toks.pop();
                            if let Some(Vt::Word(w)) = toks.pop() {
                                word = w;
                            }
                        }
                    }
                }
                if !word.is_empty() && ends_in_url(&word) {
                    let end = raw[i..].find(')').map_or(raw.len(), |e| i + e + 1);
                    word.push_str(&raw[i..end]);
                    i = end;
                } else {
                    flush(&mut word, &mut toks);
                    toks.push(Vt::Open);
                    i += 1;
                }
            }
            ')' => {
                flush(&mut word, &mut toks);
                toks.push(Vt::Close);
                i += 1;
            }
            '[' => {
                let end = raw[i..].find(']').map_or(raw.len(), |e| i + e + 1);
                word.push_str(&raw[i..end]);
                i = end;
            }
            '#' => {
                let j = i + 1 + raw[i + 1..].bytes().take_while(u8::is_ascii_alphanumeric).count();
                let hex = &raw[i + 1..j];
                word.push('#');
                if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    word.push_str(&hex.to_ascii_lowercase());
                } else {
                    word.push_str(hex);
                }
                i = j;
            }
            _ => {
                word.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    flush(&mut word, &mut toks);
    Some(toks)
}

fn emit_value(toks: &[Vt]) -> String {
    // Each token other than a space, and whether a space came before it.
    let mut items: Vec<(&Vt, bool)> = Vec::new();
    let mut spaced = false;
    for t in toks {
        if matches!(t, Vt::Space) {
            spaced = true;
        } else {
            items.push((t, spaced));
            spaced = false;
        }
    }
    let mut out = String::new();
    for (idx, &(t, sp)) in items.iter().enumerate() {
        let prev = idx.checked_sub(1).map(|p| items[p].0);
        let space = match (prev, t) {
            (None, _) => false,
            (_, Vt::Comma) => false,
            (_, Vt::Close) => false,
            (Some(Vt::Open), _) => false,
            (Some(Vt::Comma), _) => true,
            (_, Vt::Slash(_)) | (Some(Vt::Slash(_)), _) => true,
            (Some(Vt::Word(w)), Vt::Open) if ends_in_name(w) => false,
            _ => sp,
        };
        if space {
            out.push(' ');
        }
        match t {
            Vt::Word(w) => out.push_str(w),
            Vt::Comma => out.push(','),
            Vt::Slash(n) => out.push_str(&"/".repeat(*n)),
            Vt::Open => out.push('('),
            Vt::Close => out.push(')'),
            Vt::Space => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------------
// Finding where a composition body ends, mirroring the parser, so a comment after the
// closing brace can be recovered.

mod scan {
    use crate::escape;

    pub fn line_end(src: &str, k: usize) -> usize {
        src[k..].find('\n').map_or(src.len(), |e| k + e)
    }

    /// From just inside an opening brace, returns the offset just past its closing brace.
    pub fn body_end(src: &str, mut k: usize) -> Option<usize> {
        let bytes = src.as_bytes();
        let mut at_line_start = false;
        loop {
            let c = src.get(k..)?.chars().next()?;
            match c {
                '\n' => {
                    at_line_start = true;
                    k += 1;
                }
                ' ' | '\t' | '\r' => k += 1,
                ',' => {
                    at_line_start = false;
                    k += 1;
                }
                '}' => return Some(k + 1),
                '/' if at_line_start && src[k..].starts_with("//") => k = line_end(src, k),
                '"' => {
                    at_line_start = false;
                    k = quoted_end(src, k)?;
                }
                c if c.is_ascii_alphabetic() => {
                    at_line_start = false;
                    while k < src.len() && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'-') {
                        k += 1;
                    }
                    let mut m = k;
                    while m < src.len() && (bytes[m] == b' ' || bytes[m] == b'\t') {
                        m += 1;
                    }
                    k = match bytes.get(m) {
                        Some(b'{') => body_end(src, m + 1)?,
                        Some(b':') => attr_end(src, m + 1),
                        _ => m,
                    };
                }
                _ => {
                    at_line_start = false;
                    k += c.len_utf8();
                }
            }
        }
    }

    fn quoted_end(src: &str, k: usize) -> Option<usize> {
        let mut m = k + 1;
        loop {
            if m >= src.len() || src.as_bytes()[m] == b'\n' {
                return None;
            }
            match escape::scan_wrapper(src, m) {
                Ok(Some((end, _))) => {
                    m = end;
                    continue;
                }
                Err(()) => return None,
                Ok(None) => {}
            }
            let ch = src[m..].chars().next()?;
            if ch == '"' {
                return Some(m + 1);
            }
            m += ch.len_utf8();
        }
    }

    fn attr_end(src: &str, mut k: usize) -> usize {
        let bytes = src.as_bytes();
        while k < src.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        if bytes.get(k) == Some(&b'"') {
            return quoted_end(src, k).unwrap_or_else(|| line_end(src, k));
        }
        let mut m = k;
        let mut depth = 0i32;
        while m < src.len() {
            match escape::scan_wrapper(src, m) {
                Ok(Some((end, _))) if !src[m..end].contains('\n') => {
                    m = end;
                    continue;
                }
                Ok(Some(_)) | Err(()) => {
                    m += 1;
                    continue;
                }
                Ok(None) => {}
            }
            let ch = src[m..].chars().next().unwrap();
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' | '\n' | '}' if depth <= 0 => break,
                _ => {}
            }
            m += ch.len_utf8();
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        match format_source(Path::new("t.emul"), src) {
            Ok(s) => s,
            Err(d) => panic!("unexpected errors: {d:?}"),
        }
    }

    /// Formats, then checks the result is stable under a second pass.
    fn check(src: &str, expected: &str) {
        let once = fmt(src);
        assert_eq!(once, expected, "\n--- got ---\n{once}\n--- expected ---\n{expected}");
        let twice = fmt(&once);
        assert_eq!(twice, once, "not idempotent:\n{twice}");
    }

    const SPEC_BEFORE: &str = "from ./Buttons.emul import Secondary,Primary\n\
component   Card  extends Base,Shadowed :\n  padding:24px    32px\n  backgroundColor : #FFFFFF\n\
//main card\ncomposition Home{Card{\n    \"Welcome\"   ,Primary{\"Start\"}}}\n";

    const SPEC_AFTER: &str = "from ./Buttons import Primary, Secondary

component Card extends Base, Shadowed:
    padding: 24px 32px
    backgroundColor: #ffffff

// main card
composition Home {
    Card {
        \"Welcome\"
        Primary { \"Start\" }
    }
}
";

    #[test]
    fn spec_example() {
        assert_eq!(fmt(SPEC_BEFORE), SPEC_AFTER);
    }

    #[test]
    fn spec_example_idempotent() {
        assert_eq!(fmt(SPEC_AFTER), SPEC_AFTER);
        assert_eq!(fmt(&fmt(SPEC_BEFORE)), SPEC_AFTER);
    }

    #[test]
    fn syntax_errors_are_returned() {
        let r = format_source(Path::new("t.emul"), "component lower:\n    color red\n");
        assert!(r.is_err_and(|d| d.has_errors()));
    }

    #[test]
    fn imports_sorted_and_split() {
        let src = "from ./b.emul import Zed, Alpha Al, Mid\n\
from ./a import One\n\
from ./c import Two,One\n\
component X:\n    color: #000\n";
        let expected = "from ./a import One
from ./b import
    Alpha Al
    Mid
    Zed
from ./c import One, Two

component X:
    color: #000
";
        check(src, expected);
    }

    #[test]
    fn long_import_split() {
        let src = "from ./some/very/long/path/to/a/module/that/goes/on/and/on import LongNameOne, LongNameTwo\n";
        let expected = "from ./some/very/long/path/to/a/module/that/goes/on/and/on import\n    LongNameOne\n    LongNameTwo\n";
        check(src, expected);
    }

    #[test]
    fn imports_hoisted_above_declarations() {
        let src = "component A:\n    color: #000\nfrom ./x import B\n";
        check(src, "from ./x import B\n\ncomponent A:\n    color: #000\n");
    }

    #[test]
    fn long_body_goes_multiline() {
        let src = "composition Home {\n    Card { \"A fairly long piece of text here\", title: Some title, alt: \"Another value\" }\n    Empty {   }\n}\n";
        let expected = "composition Home {
    Card {
        \"A fairly long piece of text here\"
        title: Some title
        alt: \"Another value\"
    }
    Empty {}
}
";
        check(src, expected);
    }

    #[test]
    fn short_body_joins_one_line() {
        let src = "composition Home {\n    Card {\n        \"Hi\"\n        alt: x\n    }\n}\n";
        check(src, "composition Home {\n    Card { \"Hi\", alt: x }\n}\n");
    }

    #[test]
    fn empty_composition() {
        check("export composition Blank {\n\n}\n", "export composition Blank {}\n");
    }

    #[test]
    fn escapes_preserved() {
        let src = "component Q:\n    fontFamily:   \"Helvetica  Neue\",sans-serif\n    content: a\\ , \\ b\n\n\
composition Pricing {\n    Quote { \"She said \\ \" \\ worth   it \\ \" \\ and\" }\n    Price { title: Plans\\ , \\ pricing,src: ./images/a.png }\n}\n";
        let expected = "component Q:
    fontFamily: \"Helvetica  Neue\", sans-serif
    content: a\\ , \\ b

composition Pricing {
    Quote { \"She said \\ \" \\ worth   it \\ \" \\ and\" }
    Price { title: Plans\\ , \\ pricing, src: ./images/a.png }
}
";
        check(src, expected);
    }

    #[test]
    fn values() {
        assert_eq!(format_value("rgba ( 0,0,0,.5 )"), "rgba(0, 0, 0, .5)");
        assert_eq!(format_value("16/9"), "16 / 9");
        assert_eq!(format_value("1   /   3"), "1 / 3");
        assert_eq!(format_value("0 4px 12px RGBA(0 , 0, 0, 0.08) ,  #ABCDEF"), "0 4px 12px RGBA(0, 0, 0, 0.08), #abcdef");
        assert_eq!(format_value("url(./Images/A B.png)"), "url(./Images/A B.png)");
        assert_eq!(format_value("url(https://example.com/HERO.png) , #FFF"), "url(https://example.com/HERO.png), #fff");
        assert_eq!(format_value("oklch(0.5 0.1 200/0.5)"), "oklch(0.5 0.1 200 / 0.5)");
        assert_eq!(format_value("repeat(2,1fr) [Main  Side]"), "repeat(2, 1fr) [Main  Side]");
        assert_eq!(format_value("#GGG"), "#GGG");
        // A backslash that is not an escape leaves the value alone.
        assert_eq!(format_value("a\\b ,c"), "a\\b ,c");
        for v in ["rgba(0, 0, 0, .5)", "16 / 9", "\"A  B\", serif", "a\\ , \\ b"] {
            assert_eq!(format_value(v), v);
        }
    }

    #[test]
    fn comments() {
        let src = "//  Header comment\n\n\
from ./z import Z\n\
//about a\n\
from ./a import A\n\
//   about card\n\n\
component Card:\n\
//inside\n    color: #000\n\n\n    //   indented\n    padding: 4px\n\
composition Home {\n    //first\n    Card { \"x\" }\n\n\n    Card {\n        //note\n        \"y\"\n    }\n} //after\n\
// trailing\n";
        let expected = "// Header comment

// about a
from ./a import A
from ./z import Z

// about card

component Card:
    // inside
    color: #000

    // indented
    padding: 4px

composition Home {
    // first
    Card { \"x\" }

    Card {
        // note
        \"y\"
    }
} // after

// trailing
";
        check(src, expected);
    }

    #[test]
    fn comment_attached_without_blank() {
        let src = "component A:\n    color: #000\n// about B\ncomponent B:\n    color: #fff\n";
        check(src, "component A:\n    color: #000\n\n// about B\ncomponent B:\n    color: #fff\n");
    }

    #[test]
    fn exports_and_reexports_stay_in_place() {
        let src = "from ./Button.emul import Button\nexport Button\nfrom ./Card export Card\nfrom ./Forms.emul export *\n\n\n\nfrom ./Badge export Badge Tag\n";
        let expected = "from ./Button import Button

export Button
from ./Card export Card
from ./Forms export *

from ./Badge export Badge Tag
";
        check(src, expected);
    }

    #[test]
    fn crlf_tabs_and_trailing_space() {
        let src = "component A:\r\n\tcolor:  #FFF   \r\n\r\n\r\n\r\ncomposition B {\r\n\tA { }   \r\n}\r\n\r\n";
        check(src, "component A:\n    color: #fff\n\ncomposition B {\n    A {}\n}\n");
    }

    #[test]
    fn empty_file() {
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("\n\n\n"), "");
    }
}
