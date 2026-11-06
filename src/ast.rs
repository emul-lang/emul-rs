//! The syntax tree of an `.emul` file. It keeps comments and blank lines, so the
//! formatter can rewrite a file without losing anything.

use crate::diag::Span;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Blank,
    Comment(Comment),
    Import(Import),
    ReExport(ReExport),
    ExportNames(ExportNames),
    Component(Component),
    Composition(Composition),
}

#[derive(Clone, Debug)]
pub struct Comment {
    /// The comment text after `//`, with leading spaces removed.
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImportName {
    pub name: String,
    pub alias: Option<String>,
    pub span: Span,
}

impl ImportName {
    /// The name this import is known by in the importing file.
    pub fn local(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug)]
pub struct Import {
    pub path: String,
    pub path_span: Span,
    pub names: Vec<ImportName>,
    pub span: Span,
}

/// `from ./x export A, B` or `from ./x export *`.
#[derive(Clone, Debug)]
pub struct ReExport {
    pub path: String,
    pub path_span: Span,
    /// `None` means `*`.
    pub names: Option<Vec<ImportName>>,
    pub span: Span,
}

/// `export A, B`, exporting names declared or imported earlier.
#[derive(Clone, Debug)]
pub struct ExportNames {
    pub names: Vec<(String, Span)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Component {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    pub extends: Vec<(String, Span)>,
    pub body: Vec<ComponentItem>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ComponentItem {
    Blank,
    Comment(Comment),
    Property(Property),
}

#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub name_span: Span,
    /// The value as written, escapes included.
    pub raw: String,
    /// The value with escapes removed.
    pub value: String,
    pub value_span: Span,
}

#[derive(Clone, Debug)]
pub struct Composition {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    pub body: Vec<Entry>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Entry {
    Blank,
    Comment(Comment),
    Text(Text),
    Attribute(Attribute),
    Instance(Instance),
}

#[derive(Clone, Debug)]
pub struct Text {
    /// As written, quotes and escapes included.
    pub raw: String,
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub name_span: Span,
    /// As written, quotes and escapes included.
    pub raw: String,
    pub value: String,
    pub value_span: Span,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub name: String,
    pub name_span: Span,
    pub body: Vec<Entry>,
    pub span: Span,
}
