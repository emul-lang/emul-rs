//! Diagnostics: errors and warnings, reported with file, line, column and a code.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// A position in a source file. Lines and columns are 1-based; `len` is in characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

impl Span {
    pub fn new(line: usize, col: usize, len: usize) -> Self {
        Span { line, col, len: len.max(1) }
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub file: Option<PathBuf>,
    pub span: Option<Span>,
}

#[derive(Default, Debug, Clone)]
pub struct Diags {
    pub list: Vec<Diagnostic>,
}

impl Diags {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&mut self, code: &'static str, file: Option<&Path>, span: Option<Span>, message: impl Into<String>) {
        self.push(Severity::Error, code, file, span, message.into());
    }

    pub fn warning(&mut self, code: &'static str, file: Option<&Path>, span: Option<Span>, message: impl Into<String>) {
        self.push(Severity::Warning, code, file, span, message.into());
    }

    fn push(&mut self, severity: Severity, code: &'static str, file: Option<&Path>, span: Option<Span>, message: String) {
        let d = Diagnostic { severity, code, message, file: file.map(|p| p.to_path_buf()), span };
        // Avoid exact duplicates, which can arise when a file is reached through several imports.
        if !self.list.iter().any(|e| {
            e.code == d.code && e.file == d.file && e.span == d.span && e.message == d.message
        }) {
            self.list.push(d);
        }
    }

    pub fn extend(&mut self, other: Diags) {
        for d in other.list {
            self.push(d.severity, d.code, d.file.as_deref(), d.span, d.message);
        }
    }

    pub fn has_errors(&self) -> bool {
        self.list.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.list.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.list.iter().filter(|d| d.severity == Severity::Warning).count()
    }

    pub fn sorted(&self) -> Vec<Diagnostic> {
        let mut v = self.list.clone();
        v.sort_by(|a, b| {
            (a.file.clone(), a.span.map(|s| (s.line, s.col)), a.severity)
                .cmp(&(b.file.clone(), b.span.map(|s| (s.line, s.col)), b.severity))
        });
        v
    }

    /// Renders every diagnostic, with the offending line and a marker under the problem.
    pub fn render(&self, sources: &HashMap<PathBuf, String>, base: &Path) -> String {
        let mut out = String::new();
        for d in self.sorted() {
            out.push_str(&render_one(&d, sources, base));
        }
        out
    }
}

pub fn display_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).display().to_string()
}

pub fn render_one(d: &Diagnostic, sources: &HashMap<PathBuf, String>, base: &Path) -> String {
    let mut out = String::new();
    let sev = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let loc = match (&d.file, d.span) {
        (Some(f), Some(s)) => format!("{}:{}:{}: ", display_path(f, base), s.line, s.col),
        (Some(f), None) => format!("{}: ", display_path(f, base)),
        _ => String::new(),
    };
    let _ = writeln!(out, "{sev}: {loc}{} [{}]", d.message, d.code);
    if let (Some(f), Some(s)) = (&d.file, d.span) {
        if let Some(src) = sources.get(f) {
            if let Some(line) = src.lines().nth(s.line.saturating_sub(1)) {
                let num = s.line.to_string();
                let pad = " ".repeat(num.len());
                let shown: String = line.replace('\t', "    ");
                // Columns count characters; tabs were widened to four spaces for display.
                let prefix: String = line.chars().take(s.col.saturating_sub(1)).collect();
                let col = prefix.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
                let _ = writeln!(out, "  {num} | {shown}");
                let _ = writeln!(out, "  {pad} | {}{}", " ".repeat(col), "^".repeat(s.len.max(1)));
            }
        }
    }
    out
}
