//! Parses `.emul` source into the syntax tree. Components are indentation-based and
//! line-oriented; compositions use braces and may span any number of lines.

use crate::ast::*;
use crate::diag::{Diags, Span};
use crate::escape;
use std::path::{Path, PathBuf};

pub struct ParseResult {
    pub file: SourceFile,
    pub diags: Diags,
}

pub fn parse(path: &Path, src: &str) -> ParseResult {
    let mut p = Parser::new(path, src);
    let items = p.parse_file();
    ParseResult { file: SourceFile { path: path.to_path_buf(), items }, diags: p.diags }
}

struct Parser<'a> {
    path: PathBuf,
    src: &'a str,
    /// Byte offset of the start of every line.
    line_starts: Vec<usize>,
    diags: Diags,
    /// The indentation used for component properties, once seen.
    indent_unit: Option<String>,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

pub fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}

impl<'a> Parser<'a> {
    fn new(path: &Path, src: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Parser { path: path.to_path_buf(), src, line_starts, diags: Diags::new(), indent_unit: None }
    }

    fn line_count(&self) -> usize {
        // A trailing newline does not start another line.
        if self.src.ends_with('\n') { self.line_starts.len() - 1 } else { self.line_starts.len() }
    }

    /// The text of a line (0-based index), without its line ending.
    fn line(&self, i: usize) -> &'a str {
        let start = self.line_starts[i];
        let end = self.line_starts.get(i + 1).copied().unwrap_or(self.src.len());
        self.src[start..end].trim_end_matches(['\n', '\r'])
    }

    /// Converts a byte offset to a (line, column) pair, both 1-based, columns in characters.
    fn pos(&self, offset: usize) -> (usize, usize) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let col = self.src[self.line_starts[line]..offset].chars().count() + 1;
        (line + 1, col)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let (line, col) = self.pos(start);
        let len = self.src[start..end.max(start)].chars().take_while(|&c| c != '\n').count();
        Span::new(line, col, len)
    }

    fn err(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        let path = self.path.clone();
        self.diags.error(code, Some(&path), Some(span), msg);
    }

    fn parse_file(&mut self) -> Vec<Item> {
        let mut items = Vec::new();
        let n = self.line_count();
        let mut i = 0;
        while i < n {
            let line = self.line(i);
            let trimmed = line.trim();
            let lstart = self.line_starts[i];
            let indent_len = line.len() - line.trim_start().len();
            if trimmed.is_empty() {
                items.push(Item::Blank);
                i += 1;
                continue;
            }
            if trimmed.starts_with("//") {
                items.push(Item::Comment(self.comment(lstart + indent_len, trimmed)));
                i += 1;
                continue;
            }
            if indent_len > 0 {
                self.err(
                    "unexpected-indentation",
                    self.span(lstart, lstart + line.len()),
                    "this line is indented, but no component is open",
                );
                i += 1;
                continue;
            }
            let (first, rest) = split_word(trimmed);
            match first {
                "from" => {
                    let (item, next) = self.parse_from(i);
                    if let Some(item) = item {
                        items.push(item);
                    }
                    i = next;
                }
                "export" => {
                    let (second, after) = split_word(rest);
                    match second {
                        "component" => {
                            let (c, next) = self.parse_component(i, true, after);
                            if let Some(c) = c {
                                items.push(Item::Component(c));
                            }
                            i = next;
                        }
                        "composition" => {
                            let (c, next) = self.parse_composition(i, true);
                            if let Some(c) = c {
                                items.push(Item::Composition(c));
                            }
                            i = next;
                        }
                        _ => {
                            if let Some(e) = self.parse_export_names(i, rest) {
                                items.push(Item::ExportNames(e));
                            }
                            i += 1;
                        }
                    }
                }
                "component" => {
                    let (c, next) = self.parse_component(i, false, rest);
                    if let Some(c) = c {
                        items.push(Item::Component(c));
                    }
                    i = next;
                }
                "composition" => {
                    let (c, next) = self.parse_composition(i, false);
                    if let Some(c) = c {
                        items.push(Item::Composition(c));
                    }
                    i = next;
                }
                _ => {
                    let end = lstart + first.len();
                    self.err(
                        "unknown-keyword",
                        self.span(lstart, end),
                        format!("expected `component`, `composition`, `export` or `from`, found `{first}`"),
                    );
                    i += 1;
                }
            }
        }
        items
    }

    fn comment(&self, start: usize, trimmed: &str) -> Comment {
        let text = trimmed[2..].trim().to_string();
        Comment { text, span: self.span(start, start + trimmed.len()) }
    }

    /// `from PATH import names` or `from PATH export names|*`. Names may continue on
    /// indented lines when the first line ends after `import` or `export`.
    fn parse_from(&mut self, i: usize) -> (Option<Item>, usize) {
        let line = self.line(i);
        let lstart = self.line_starts[i];
        let after_from = line[4..].trim_start();
        let path_off = lstart + (line.len() - after_from.len());
        let (path, rest) = split_word(after_from);
        if path.is_empty() {
            self.err("missing-file", self.span(lstart, lstart + line.len()), "`from` needs a path");
            return (None, i + 1);
        }
        let path_span = self.span(path_off, path_off + path.len());
        let (kw, names_text) = split_word(rest);
        if kw != "import" && kw != "export" {
            self.err(
                "unknown-keyword",
                self.span(lstart, lstart + line.len()),
                "expected `import` or `export` after the path",
            );
            return (None, i + 1);
        }
        let mut next = i + 1;
        let mut pieces: Vec<(String, usize)> = Vec::new();
        let names_off = lstart + line.len() - names_text.len();
        if !names_text.trim().is_empty() {
            pieces.push((names_text.to_string(), names_off));
        } else {
            // Names on following indented lines.
            while next < self.line_count() {
                let l = self.line(next);
                if l.trim().is_empty() || l.len() == l.trim_start().len() {
                    break;
                }
                let off = self.line_starts[next] + (l.len() - l.trim_start().len());
                pieces.push((l.trim().to_string(), off));
                next += 1;
            }
        }
        let span = self.span(lstart, lstart + line.len());
        if kw == "export" && pieces.len() == 1 && pieces[0].0.trim() == "*" {
            return (Some(Item::ReExport(ReExport { path: path.to_string(), path_span, names: None, span })), next);
        }
        let mut names = Vec::new();
        for (text, off) in pieces {
            for (part, poff) in split_commas(&text, off) {
                let words: Vec<&str> = part.split_whitespace().collect();
                if words.is_empty() {
                    continue;
                }
                let word_span = self.span(poff, poff + part.len());
                if words.len() > 2 || !words.iter().all(|w| w.chars().all(is_ident_char)) {
                    self.err("unknown-keyword", word_span, format!("`{part}` is not a name, or a name and its alias"));
                    continue;
                }
                names.push(ImportName {
                    name: words[0].to_string(),
                    alias: words.get(1).map(|s| s.to_string()),
                    span: word_span,
                });
            }
        }
        if names.is_empty() {
            self.err("unknown-keyword", span, format!("`{kw}` needs at least one name"));
            return (None, next);
        }
        let item = if kw == "import" {
            Item::Import(Import { path: path.to_string(), path_span, names, span })
        } else {
            Item::ReExport(ReExport { path: path.to_string(), path_span, names: Some(names), span })
        };
        (Some(item), next)
    }

    fn parse_export_names(&mut self, i: usize, rest: &str) -> Option<ExportNames> {
        let line = self.line(i);
        let lstart = self.line_starts[i];
        let off = lstart + line.len() - rest.len();
        let mut names = Vec::new();
        for (part, poff) in split_commas(rest, off) {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            let sp = self.span(poff, poff + part.len());
            if !name.chars().all(is_ident_char) {
                self.err("unknown-keyword", sp, format!("`{name}` is not a name that can be exported"));
                continue;
            }
            names.push((name.to_string(), sp));
        }
        if names.is_empty() {
            self.err("unknown-keyword", self.span(lstart, lstart + line.len()), "`export` needs a declaration or a name");
            return None;
        }
        Some(ExportNames { names, span: self.span(lstart, lstart + line.len()) })
    }

    fn check_name(&mut self, name: &str, span: Span, what: &str) {
        if !is_pascal_case(name) {
            self.err(
                "invalid-name",
                span,
                format!("{what} name `{name}` must be PascalCase: a capital letter, then letters and digits"),
            );
        }
    }

    /// `[export] component Name [extends A, B]:` followed by indented property lines.
    fn parse_component(&mut self, i: usize, exported: bool, rest: &str) -> (Option<Component>, usize) {
        let line = self.line(i);
        let lstart = self.line_starts[i];
        let header_span = self.span(lstart, lstart + line.len());
        let rest_off = lstart + line.len() - rest.len();
        let rest_trim = rest.trim_end();
        let Some(colon) = rest_trim.rfind(':').filter(|&c| c == rest_trim.len() - 1) else {
            self.err("missing-colon", header_span, "a component declaration must end with a colon");
            // Skip its body so it doesn't produce a flood of indentation errors.
            return (None, self.skip_indented(i + 1));
        };
        let decl = &rest_trim[..colon];
        let (name, after_name) = split_word(decl);
        let name = name.trim_end_matches(':');
        let name_span = self.span(rest_off, rest_off + name.len());
        if name.is_empty() {
            self.err("invalid-name", header_span, "a component needs a name");
            return (None, self.skip_indented(i + 1));
        }
        self.check_name(name, name_span, "component");
        let mut extends = Vec::new();
        let after = after_name.trim();
        if !after.is_empty() {
            let (kw, list) = split_word(after);
            if kw != "extends" {
                let off = rest_off + (decl.len() - after_name.len()) + (after_name.len() - after_name.trim_start().len());
                self.err("unknown-keyword", self.span(off, off + kw.len()), format!("expected `extends` or `:`, found `{kw}`"));
            } else {
                let list_off = lstart + line.len() - rest.len() + decl.len() - list.len();
                for (part, poff) in split_commas(list, list_off) {
                    let n = part.trim();
                    if n.is_empty() {
                        continue;
                    }
                    let sp = self.span(poff, poff + part.len());
                    if !n.chars().all(is_ident_char) {
                        self.err("invalid-name", sp, format!("`{n}` is not a component name"));
                        continue;
                    }
                    extends.push((n.to_string(), sp));
                }
                if extends.is_empty() {
                    self.err("invalid-name", header_span, "`extends` needs at least one component");
                }
            }
        }

        let mut body = Vec::new();
        let mut j = i + 1;
        let n = self.line_count();
        while j < n {
            let l = self.line(j);
            let t = l.trim();
            let indent = &l[..l.len() - l.trim_start().len()];
            if t.is_empty() || (t.starts_with("//") && indent.is_empty()) {
                // A blank line or unindented comment stays in the component only if the
                // next significant line is still indented.
                if self.next_significant_is_indented(j + 1) {
                    if t.is_empty() {
                        body.push(ComponentItem::Blank);
                    } else {
                        body.push(ComponentItem::Comment(self.comment(self.line_starts[j], t)));
                    }
                    j += 1;
                    continue;
                }
                break;
            }
            if indent.is_empty() {
                break;
            }
            let lst = self.line_starts[j];
            self.check_indent(indent, self.span(lst, lst + indent.len()));
            let off = lst + indent.len();
            if t.starts_with("//") {
                body.push(ComponentItem::Comment(self.comment(off, t)));
                j += 1;
                continue;
            }
            if let Some(p) = self.parse_property(t, off) {
                body.push(ComponentItem::Property(p));
            }
            j += 1;
        }
        // Trailing blank lines belong to the file, not the component.
        let mut trailing = 0;
        while matches!(body.last(), Some(ComponentItem::Blank)) {
            body.pop();
            trailing += 1;
        }
        let c = Component { exported, name: name.to_string(), name_span, extends, body, span: header_span };
        (Some(c), j - trailing)
    }

    fn next_significant_is_indented(&self, mut j: usize) -> bool {
        while j < self.line_count() {
            let l = self.line(j);
            let t = l.trim();
            if t.is_empty() {
                j += 1;
                continue;
            }
            let indented = l.len() != l.trim_start().len();
            if t.starts_with("//") && !indented {
                j += 1;
                continue;
            }
            return indented;
        }
        false
    }

    fn skip_indented(&self, mut j: usize) -> usize {
        while j < self.line_count() {
            let l = self.line(j);
            if !l.trim().is_empty() && l.len() == l.trim_start().len() {
                break;
            }
            j += 1;
        }
        j
    }

    fn check_indent(&mut self, indent: &str, span: Span) {
        let mixed = indent.contains(' ') && indent.contains('\t');
        match &self.indent_unit {
            None if !mixed => self.indent_unit = Some(indent.to_string()),
            Some(unit) if !mixed && unit == indent => {}
            _ => {
                let expected = match &self.indent_unit {
                    Some(u) if u.starts_with('\t') => format!("{} tab(s)", u.len()),
                    Some(u) => format!("{} space(s)", u.len()),
                    None => "tabs or spaces, not both".into(),
                };
                self.err("mixed-indentation", span, format!("inconsistent indentation; this file uses {expected}"));
            }
        }
    }

    fn parse_property(&mut self, t: &str, off: usize) -> Option<Property> {
        let line_span = self.span(off, off + t.len());
        let first = t.chars().next().unwrap();
        if first == '"' || t.contains('{') && t.split(['{', ':']).next().is_some_and(|w| is_pascal_case(w.trim())) {
            self.err(
                "content-in-component",
                line_span,
                "components hold properties only; text and instances belong in compositions",
            );
            return None;
        }
        let Some(colon) = t.find(':') else {
            self.err("missing-colon", line_span, "a property line needs a colon between its name and value");
            return None;
        };
        let name = t[..colon].trim_end();
        if name.is_empty() || !name.chars().next().is_some_and(is_ident_start) || !name.chars().all(is_ident_char) {
            self.err("content-in-component", line_span, format!("`{name}` is not a property name"));
            return None;
        }
        let name_span = self.span(off, off + name.len());
        let raw_full = &t[colon + 1..];
        let raw = raw_full.trim();
        let voff = off + colon + 1 + (raw_full.len() - raw_full.trim_start().len());
        let value_span = self.span(voff, voff + raw.len());
        if raw.is_empty() {
            self.err("empty-value", name_span, format!("`{name}` has no value after its colon"));
            return None;
        }
        let value = match escape::unescape(raw) {
            Ok(v) => v,
            Err(at) => {
                self.err("unclosed-escape", self.span(voff + at, voff + at + 1), "this escape is never closed");
                return None;
            }
        };
        Some(Property { name: name.to_string(), name_span, raw: raw.to_string(), value, value_span })
    }

    /// `[export] composition Name {` ... `}`.
    fn parse_composition(&mut self, i: usize, exported: bool) -> (Option<Composition>, usize) {
        let line = self.line(i);
        let lstart = self.line_starts[i];
        let header_span = self.span(lstart, lstart + line.len());
        let kw_at = line.find("composition").unwrap();
        let mut k = lstart + kw_at + "composition".len();
        let bytes = self.src.as_bytes();
        while k < self.src.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        let name_start = k;
        while k < self.src.len() && (bytes[k] as char).is_ascii_alphanumeric() {
            k += 1;
        }
        let name = self.src[name_start..k].to_string();
        let name_span = self.span(name_start, k);
        if name.is_empty() {
            self.err("invalid-name", header_span, "a composition needs a name");
            return (None, i + 1);
        }
        self.check_name(&name, name_span, "composition");
        while k < self.src.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        if bytes.get(k) != Some(&b'{') {
            self.err("missing-body", name_span, format!("composition `{name}` needs a body in braces"));
            return (None, i + 1);
        }
        let (body, end) = self.parse_body(k + 1, true);
        // `end` is just past the closing brace, or the end of the file if it never closed.
        let Some(end) = end else {
            self.err("unclosed-brace", self.span(k, k + 1), format!("the body of `{name}` is never closed"));
            return (None, self.line_count());
        };
        let (end_line, _) = self.pos(end.saturating_sub(1));
        // Nothing may follow the closing brace on its line.
        let line_end = self.line_starts.get(end_line).copied().unwrap_or(self.src.len());
        let tail = self.src[end..line_end].trim();
        if !tail.is_empty() && !tail.starts_with("//") {
            let at = end + self.src[end..].find(tail).unwrap_or(0);
            self.err("unexpected-brace", self.span(at, at + tail.len()), "unexpected text after the end of the composition");
        }
        let c = Composition { exported, name, name_span, body, span: header_span };
        (Some(c), end_line)
    }

    /// Parses the entries of a body starting at byte `start`, just inside its opening
    /// brace. Returns the entries and the offset just past the matching closing brace.
    fn parse_body(&mut self, start: usize, _root: bool) -> (Vec<Entry>, Option<usize>) {
        let src = self.src;
        let bytes = src.as_bytes();
        let mut entries = Vec::new();
        let mut k = start;
        // Whether only whitespace has been seen since the last line break, for comments.
        let mut at_line_start = false;
        let mut newlines_in_gap = 0;
        loop {
            if k >= src.len() {
                return (entries, None);
            }
            let c = src[k..].chars().next().unwrap();
            match c {
                '\n' => {
                    newlines_in_gap += 1;
                    if newlines_in_gap == 2 && !entries.is_empty() && !matches!(entries.last(), Some(Entry::Blank)) {
                        entries.push(Entry::Blank);
                    }
                    at_line_start = true;
                    k += 1;
                }
                ' ' | '\t' | '\r' => k += 1,
                ',' => {
                    newlines_in_gap = 0;
                    at_line_start = false;
                    k += 1;
                }
                '}' => {
                    while matches!(entries.last(), Some(Entry::Blank)) {
                        entries.pop();
                    }
                    return (entries, Some(k + 1));
                }
                '/' if at_line_start && src[k..].starts_with("//") => {
                    let end = src[k..].find('\n').map(|e| k + e).unwrap_or(src.len());
                    let t = src[k..end].trim_end();
                    entries.push(Entry::Comment(self.comment(k, t)));
                    newlines_in_gap = 0;
                    k = end;
                }
                '"' => {
                    newlines_in_gap = 0;
                    at_line_start = false;
                    match self.scan_quoted(k) {
                        Some((end, value)) => {
                            entries.push(Entry::Text(Text {
                                raw: src[k..end].to_string(),
                                value,
                                span: self.span(k, end),
                            }));
                            k = end;
                        }
                        None => {
                            k = self.line_end(k);
                        }
                    }
                }
                c if c.is_ascii_alphabetic() => {
                    newlines_in_gap = 0;
                    at_line_start = false;
                    let name_start = k;
                    while k < src.len() && {
                        let b = bytes[k] as char;
                        b.is_ascii_alphanumeric() || b == '-'
                    } {
                        k += 1;
                    }
                    let name = src[name_start..k].to_string();
                    let name_span = self.span(name_start, k);
                    let mut m = k;
                    while m < src.len() && (bytes[m] == b' ' || bytes[m] == b'\t') {
                        m += 1;
                    }
                    match bytes.get(m) {
                        Some(b'{') => {
                            if !is_pascal_case(&name) {
                                self.err("invalid-name", name_span, format!("`{name}` is not a component name; component names are PascalCase"));
                            }
                            let (body, end) = self.parse_body(m + 1, false);
                            let Some(end) = end else {
                                self.err("unclosed-brace", self.span(m, m + 1), format!("the body of `{name}` is never closed"));
                                return (entries, None);
                            };
                            entries.push(Entry::Instance(Instance {
                                name,
                                name_span,
                                body,
                                span: self.span(name_start, m + 1),
                            }));
                            k = end;
                        }
                        Some(b':') => {
                            let (end, raw, value, vstart) = self.scan_attr_value(m + 1);
                            let value_span = self.span(vstart, vstart + raw.len());
                            entries.push(Entry::Attribute(Attribute { name, name_span, raw, value, value_span }));
                            k = end;
                        }
                        _ => {
                            if is_pascal_case(&name) {
                                self.err(
                                    "missing-body",
                                    name_span,
                                    format!("instance `{name}` needs a body in braces, even an empty one"),
                                );
                            } else {
                                self.err(
                                    "unexpected-character",
                                    name_span,
                                    format!("`{name}` is not an instance, text or attribute"),
                                );
                            }
                            k = m;
                        }
                    }
                }
                _ => {
                    newlines_in_gap = 0;
                    at_line_start = false;
                    self.err(
                        "unexpected-character",
                        self.span(k, k + c.len_utf8()),
                        format!("unexpected `{c}` in a composition body"),
                    );
                    k += c.len_utf8();
                }
            }
        }
    }

    fn line_end(&self, k: usize) -> usize {
        self.src[k..].find('\n').map(|e| k + e).unwrap_or(self.src.len())
    }

    /// Scans quoted text starting at the opening quote at `k`. Returns the offset just
    /// past the closing quote and the unescaped content.
    fn scan_quoted(&mut self, k: usize) -> Option<(usize, String)> {
        let src = self.src;
        let mut m = k + 1;
        let mut value = String::new();
        loop {
            if m >= src.len() || src.as_bytes()[m] == b'\n' {
                self.err("unclosed-quote", self.span(k, m), "this text is not closed before the end of the line");
                return None;
            }
            match escape::scan_wrapper(src, m) {
                Ok(Some((end, content))) => {
                    if src[m..end].contains('\n') {
                        self.err("unclosed-escape", self.span(m, m + 1), "this escape is never closed on its line");
                        return None;
                    }
                    value.push_str(&content);
                    m = end;
                    continue;
                }
                Err(()) => {
                    self.err("unclosed-escape", self.span(m, m + 1), "this escape is never closed");
                    return None;
                }
                Ok(None) => {}
            }
            let ch = src[m..].chars().next().unwrap();
            if ch == '"' {
                return Some((m + 1, value));
            }
            value.push(ch);
            m += ch.len_utf8();
        }
    }

    /// Scans an attribute value starting just after its colon. Returns the end offset,
    /// the raw text, the unescaped value and where the value starts.
    fn scan_attr_value(&mut self, mut k: usize) -> (usize, String, String, usize) {
        let src = self.src;
        let bytes = src.as_bytes();
        while k < src.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
            k += 1;
        }
        let vstart = k;
        if bytes.get(k) == Some(&b'"') {
            if let Some((end, value)) = self.scan_quoted(k) {
                return (end, src[k..end].to_string(), value, vstart);
            }
            let end = self.line_end(k);
            return (end, src[k..end].to_string(), String::new(), vstart);
        }
        let mut m = k;
        let mut depth = 0i32;
        let mut value = String::new();
        while m < src.len() {
            match escape::scan_wrapper(src, m) {
                Ok(Some((end, content))) if !src[m..end].contains('\n') => {
                    value.push_str(&content);
                    m = end;
                    continue;
                }
                Ok(Some(_)) | Err(()) => {
                    self.err("unclosed-escape", self.span(m, m + 1), "this escape is never closed");
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
            value.push(ch);
            m += ch.len_utf8();
        }
        let raw = src[k..m].trim_end().to_string();
        (m, raw, value.trim().to_string(), vstart)
    }
}

/// Splits off the first whitespace-separated word.
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Splits on commas, returning each piece (untrimmed of inner content but with
/// surrounding spaces removed) and its byte offset.
fn split_commas(s: &str, base: usize) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices().chain(std::iter::once((s.len(), ','))) {
        if c == ',' {
            let piece = &s[start..i];
            let lead = piece.len() - piece.trim_start().len();
            out.push((piece.trim().to_string(), base + start + lead));
            start = i + 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> ParseResult {
        parse(Path::new("t.emul"), src)
    }

    #[test]
    fn component_and_composition() {
        let r = p("component Card extends Base, Other:\n    padding: 8px 12px\n    color: #fff\n\ncomposition Home {\n    Card { \"Hi, there\", title: a\\ , \\ b }\n}\n");
        assert!(!r.diags.has_errors(), "{:?}", r.diags);
        let comps: Vec<_> = r.file.items.iter().filter_map(|i| if let Item::Component(c) = i { Some(c) } else { None }).collect();
        assert_eq!(comps[0].extends.len(), 2);
        let Some(Item::Composition(c)) = r.file.items.iter().find(|i| matches!(i, Item::Composition(_))) else { panic!() };
        let Entry::Instance(inst) = &c.body[0] else { panic!() };
        assert_eq!(inst.body.len(), 2);
        let Entry::Attribute(a) = &inst.body[1] else { panic!() };
        assert_eq!(a.value, "a, b");
    }

    #[test]
    fn errors() {
        let r = p("component lower:\n    color red\ncomposition X {\n    Card\n");
        let codes: Vec<_> = r.diags.list.iter().map(|d| d.code).collect();
        assert!(codes.contains(&"invalid-name"));
        assert!(codes.contains(&"missing-colon"));
        assert!(codes.contains(&"unclosed-brace"));
    }
}
