//! The syntax tree of an `.emul` file. It keeps comments and blank lines. Then the
//! formatter can rewrite a file and keep all of its content.

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
    /// The comment text after `//`, without the spaces at its start.
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
    /// The name of this import in the file that imports it.
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

/// `export A, B`. It exports names that the file declared or imported before.
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
    Attribute(AttributeDecl),
    When(When),
}

/// `attribute size: small | large = large`, or `attribute size = small` to change the
/// default of an inherited attribute.
#[derive(Clone, Debug)]
pub struct AttributeDecl {
    pub name: String,
    pub name_span: Span,
    /// `None` when the line only changes the default.
    pub values: Option<Vec<(String, Span)>>,
    pub default: Option<(String, Span)>,
    pub span: Span,
}

/// `when <condition>:` with indented property lines.
#[derive(Clone, Debug)]
pub struct When {
    /// The condition as the author wrote it.
    pub raw: String,
    pub cond: Cond,
    pub span: Span,
    /// Blank lines, comments and properties.
    pub body: Vec<ComponentItem>,
}

#[derive(Clone, Debug)]
pub enum Cond {
    /// `attr == value` (eq true) or `attr != value`.
    Cmp { attr: (String, Span), eq: bool, value: (String, Span) },
    Not(Box<Cond>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
}

impl Cond {
    /// Calls `f` with every comparison in the condition.
    pub fn each_cmp(&self, f: &mut impl FnMut(&(String, Span), &(String, Span))) {
        match self {
            Cond::Cmp { attr, value, .. } => f(attr, value),
            Cond::Not(c) => c.each_cmp(f),
            Cond::And(a, b) | Cond::Or(a, b) => {
                a.each_cmp(f);
                b.each_cmp(f);
            }
        }
    }

    /// The value of the condition. `get` gives the value of an attribute.
    pub fn eval(&self, get: &impl Fn(&str) -> Option<String>) -> bool {
        match self {
            Cond::Cmp { attr, eq, value } => (get(&attr.0).as_deref() == Some(value.0.as_str())) == *eq,
            Cond::Not(c) => !c.eval(get),
            Cond::And(a, b) => a.eval(get) && b.eval(get),
            Cond::Or(a, b) => a.eval(get) || b.eval(get),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub name_span: Span,
    /// The value as the author wrote it, with escapes.
    pub raw: String,
    /// The value without escapes.
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
    /// As the author wrote it, with quotes and escapes.
    pub raw: String,
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub name_span: Span,
    /// As the author wrote it, with quotes and escapes.
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
