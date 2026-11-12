//! Loads a project. It parses each file that the entry reaches. It resolves imports,
//! exports and component inheritance. It builds the instance tree of each composition
//! that the build renders.

use crate::ast::*;
use crate::diag::{Diags, Span};
use crate::parser;
use crate::props::{self, Ctx, Style, L, V};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type FileId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclRef {
    Component(FileId, usize),
    Composition(FileId, usize),
}

pub struct LoadedFile {
    pub path: PathBuf,
    pub ast: SourceFile,
    pub components: Vec<Component>,
    pub compositions: Vec<Composition>,
    /// The names that this file knows: local declarations and imports.
    pub scope: HashMap<String, Binding>,
    /// The names that this file exports.
    pub exports: HashMap<String, DeclRef>,
}

#[derive(Clone, Debug)]
pub enum Binding {
    Local(DeclRef),
    Imported { decl: Option<DeclRef>, span: Span, used: std::cell::Cell<bool> },
}

#[derive(Clone, Debug)]
pub enum Child {
    Text(String),
    Node(Node),
}

#[derive(Clone, Debug)]
pub struct Node {
    /// The class name in the HTML. It is the declared name of the component, made unique.
    pub class: String,
    pub style: Arc<Style>,
    pub attrs: Vec<(String, String)>,
    /// The resolved image file of an `img` instance.
    pub src: Option<PathBuf>,
    pub children: Vec<Child>,
    /// A readable id for SVG output, such as `MyButton-3`.
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct RenderedComposition {
    pub name: String,
    pub root: Vec<Node>,
}

pub struct Project {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub files: Vec<LoadedFile>,
    pub sources: HashMap<PathBuf, String>,
    pub diags: Diags,
    pub compositions: Vec<RenderedComposition>,
}

struct Loader {
    root: PathBuf,
    files: Vec<LoadedFile>,
    by_path: HashMap<PathBuf, FileId>,
    sources: HashMap<PathBuf, String>,
    diags: Diags,
}

/// Resolves an import path relative to `from_dir`. A path that starts with `/` is
/// relative to the root.
pub fn resolve_module(spec: &str, from_dir: &Path, root: &Path) -> Option<PathBuf> {
    let base = if let Some(abs) = spec.strip_prefix('/') { root.join(abs) } else { from_dir.join(spec) };
    let base = props::normalize(&base);
    let candidates = [
        base.clone(),
        base.with_extension("emul"),
        PathBuf::from(format!("{}.emul", base.display())),
        base.join("index.emul"),
    ];
    candidates.into_iter().find(|c| c.is_file() && c.extension().is_some_and(|e| e == "emul"))
}

impl Loader {
    fn load(&mut self, path: &Path) -> Option<FileId> {
        let path = props::normalize(path);
        if let Some(&id) = self.by_path.get(&path) {
            return Some(id);
        }
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.diags.error("missing-file", Some(&path), None, format!("can't read the file: {e}"));
                return None;
            }
        };
        let parsed = parser::parse(&path, &src);
        self.diags.extend(parsed.diags);
        self.sources.insert(path.clone(), src);
        let mut components = Vec::new();
        let mut compositions = Vec::new();
        for item in &parsed.file.items {
            match item {
                Item::Component(c) => components.push(c.clone()),
                Item::Composition(c) => compositions.push(c.clone()),
                _ => {}
            }
        }
        let id = self.files.len();
        self.by_path.insert(path.clone(), id);
        self.files.push(LoadedFile {
            path: path.clone(),
            ast: parsed.file,
            components,
            compositions,
            scope: HashMap::new(),
            exports: HashMap::new(),
        });
        // Load every imported file. A circular import then finds its file already loaded.
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let specs: Vec<(String, Span)> = self.files[id]
            .ast
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Import(im) => Some((im.path.clone(), im.path_span)),
                Item::ReExport(r) => Some((r.path.clone(), r.path_span)),
                _ => None,
            })
            .collect();
        for (spec, span) in specs {
            match resolve_module(&spec, &dir, &self.root) {
                Some(p) => {
                    self.load(&p);
                }
                None => self.diags.error("missing-file", Some(&path), Some(span), format!("`{spec}` is not a file or module")),
            }
        }
        Some(id)
    }

    fn file_for(&self, spec: &str, from: FileId) -> Option<FileId> {
        let dir = self.files[from].path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let p = resolve_module(spec, &dir, &self.root)?;
        self.by_path.get(&props::normalize(&p)).copied()
    }

    /// Completes the scope and exports of every file. Exports can depend on the exports
    /// of other files, so this step repeats until nothing changes.
    fn link(&mut self) {
        // Local declarations.
        for fid in 0..self.files.len() {
            let mut seen: HashMap<String, Span> = HashMap::new();
            let file = &self.files[fid];
            let path = file.path.clone();
            let mut scope = HashMap::new();
            let mut exports = HashMap::new();
            let decls: Vec<(String, Span, bool, DeclRef)> = file
                .components
                .iter()
                .enumerate()
                .map(|(i, c)| (c.name.clone(), c.name_span, c.exported, DeclRef::Component(fid, i)))
                .chain(
                    file.compositions
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (c.name.clone(), c.name_span, c.exported, DeclRef::Composition(fid, i))),
                )
                .collect();
            for (name, span, exported, r) in decls {
                if seen.contains_key(&name) {
                    self.diags.error("duplicate-name", Some(&path), Some(span), format!("`{name}` is already declared in this file"));
                    continue;
                }
                seen.insert(name.clone(), span);
                scope.insert(name.clone(), Binding::Local(r));
                if exported {
                    exports.insert(name, r);
                }
            }
            self.files[fid].scope = scope;
            self.files[fid].exports = exports;
        }
        // Imports and re-exports. This step repeats until nothing changes.
        let mut changed = true;
        let mut rounds = 0;
        while changed && rounds < 64 {
            changed = false;
            rounds += 1;
            for fid in 0..self.files.len() {
                let items = self.files[fid].ast.items.clone();
                for item in &items {
                    match item {
                        Item::ReExport(r) => {
                            let Some(src) = self.file_for(&r.path, fid) else { continue };
                            let src_exports = self.files[src].exports.clone();
                            match &r.names {
                                None => {
                                    for (n, d) in src_exports {
                                        if !self.files[fid].exports.contains_key(&n) {
                                            self.files[fid].exports.insert(n, d);
                                            changed = true;
                                        }
                                    }
                                }
                                Some(names) => {
                                    for n in names {
                                        if let Some(d) = src_exports.get(&n.name) {
                                            let local = n.local().to_string();
                                            if !self.files[fid].exports.contains_key(&local) {
                                                self.files[fid].exports.insert(local, *d);
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Item::ExportNames(e) => {
                            for (n, _) in &e.names {
                                if self.files[fid].exports.contains_key(n) {
                                    continue;
                                }
                                let d = match self.files[fid].scope.get(n) {
                                    Some(Binding::Local(d)) => Some(*d),
                                    _ => self.import_target(fid, n),
                                };
                                if let Some(d) = d {
                                    self.files[fid].exports.insert(n.clone(), d);
                                    changed = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        // Bind the imports and report the problems.
        for fid in 0..self.files.len() {
            let path = self.files[fid].path.clone();
            let items = self.files[fid].ast.items.clone();
            for item in &items {
                match item {
                    Item::Import(im) => {
                        let src = self.file_for(&im.path, fid);
                        for n in &im.names {
                            let decl = src.and_then(|s| self.files[s].exports.get(&n.name).copied());
                            if let Some(s) = src {
                                if decl.is_none() {
                                    let declared = self.files[s].scope.contains_key(&n.name);
                                    self.diags.error(
                                        "not-exported",
                                        Some(&path),
                                        Some(n.span),
                                        if declared {
                                            format!("`{}` is not exported from `{}`", n.name, im.path)
                                        } else {
                                            format!("`{}` does not declare `{}`", im.path, n.name)
                                        },
                                    );
                                }
                            }
                            let local = n.local().to_string();
                            if self.files[fid].scope.contains_key(&local) {
                                self.diags.error("duplicate-name", Some(&path), Some(n.span), format!("`{local}` is already declared or imported in this file"));
                                continue;
                            }
                            self.files[fid]
                                .scope
                                .insert(local, Binding::Imported { decl, span: n.span, used: std::cell::Cell::new(false) });
                        }
                    }
                    Item::ReExport(r) => {
                        let Some(src) = self.file_for(&r.path, fid) else { continue };
                        if let Some(names) = &r.names {
                            for n in names {
                                if !self.files[src].exports.contains_key(&n.name) {
                                    self.diags.error("export-unknown", Some(&path), Some(n.span), format!("`{}` does not export `{}`", r.path, n.name));
                                }
                            }
                        }
                    }
                    Item::ExportNames(e) => {
                        for (n, sp) in &e.names {
                            let known = self.files[fid].scope.contains_key(n) || self.import_target(fid, n).is_some();
                            if !known {
                                self.diags.error("export-unknown", Some(&path), Some(*sp), format!("`{n}` is not declared or imported in this file"));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// The item that an import of `name` in file `fid` points to. This reads the import
    /// items directly.
    fn import_target(&self, fid: FileId, name: &str) -> Option<DeclRef> {
        for item in &self.files[fid].ast.items {
            if let Item::Import(im) = item {
                for n in &im.names {
                    if n.local() == name {
                        let src = self.file_for(&im.path, fid)?;
                        return self.files[src].exports.get(&n.name).copied();
                    }
                }
            }
        }
        None
    }
}

/// Finds a name in the scope of a file, and marks imports as used.
fn lookup(files: &[LoadedFile], fid: FileId, name: &str) -> Option<Option<DeclRef>> {
    match files[fid].scope.get(name)? {
        Binding::Local(d) => Some(Some(*d)),
        Binding::Imported { decl, used, .. } => {
            used.set(true);
            Some(*decl)
        }
    }
}

/// A declared attribute of a component, after inheritance.
#[derive(Clone, Debug)]
pub struct AttrInfo {
    pub name: String,
    pub values: Vec<String>,
    pub default: String,
}

/// A `when` block of a component or of a component that it extends.
#[derive(Clone, Debug)]
struct Block {
    cond: Cond,
    decls: Vec<(String, Vec<(L, V)>)>,
}

type Decls = Vec<(String, Vec<(L, V)>)>;
/// Declarations in source order, with the span of each property name.
type OwnDecls = Vec<(String, Vec<(L, V)>, Span)>;

struct Resolver<'a> {
    root: &'a Path,
    files: &'a [LoadedFile],
    diags: &'a mut Diags,
    /// The style of each component for each list of attribute values, in the order of
    /// its attributes.
    styles: HashMap<(FileId, usize, Vec<String>), Arc<Style>>,
    /// The declarations as the author wrote them, merged through inheritance: (property,
    /// value, file of the declaration, longhands). The list keeps the merge order. The
    /// tie-break between two shorthands with the same specificity uses this order.
    merged: HashMap<(FileId, usize), Decls>,
    /// The attributes of each component, after inheritance, in declaration order.
    attrs: HashMap<(FileId, usize), Vec<AttrInfo>>,
    /// The `when` blocks of each component along its chain. Bases come first, so a later
    /// block overrides an earlier one.
    blocks: HashMap<(FileId, usize), Vec<Block>>,
    visiting: HashSet<(FileId, usize)>,
    used: HashSet<DeclRef>,
}

/// The shorthand rule: for each longhand, the most specific declaration wins. If two
/// declarations are equal, the later one in merge order wins.
fn resolve_decls(decls: &Decls) -> BTreeMap<L, V> {
    let mut best: BTreeMap<L, (u8, usize, V)> = BTreeMap::new();
    for (order, (name, lh)) in decls.iter().enumerate() {
        let lvl = props::level(name).unwrap_or(2);
        for (l, v) in lh {
            let better = match best.get(l) {
                None => true,
                Some((bl, bo, _)) => lvl > *bl || (lvl == *bl && order >= *bo),
            };
            if better {
                best.insert(*l, (lvl, order, v.clone()));
            }
        }
    }
    best.into_iter().map(|(l, (_, _, v))| (l, v)).collect()
}

/// True when some values of the attributes make both conditions true.
fn can_both_hold(a: &Cond, b: &Cond, attrs: &[AttrInfo]) -> bool {
    let mut names: Vec<String> = Vec::new();
    for c in [a, b] {
        c.each_cmp(&mut |attr, _| {
            if !names.contains(&attr.0) {
                names.push(attr.0.clone());
            }
        });
    }
    let lists: Vec<&[String]> = names.iter().map(|n| attrs.iter().find(|x| x.name == *n).map(|x| x.values.as_slice()).unwrap_or(&[])).collect();
    if lists.iter().any(|l| l.is_empty()) {
        return false;
    }
    let mut idx = vec![0usize; names.len()];
    loop {
        let get = |n: &str| names.iter().position(|x| x == n).map(|i| lists[i][idx[i]].clone());
        if a.eval(&get) && b.eval(&get) {
            return true;
        }
        // The next combination.
        let mut k = 0;
        loop {
            if k == idx.len() {
                return false;
            }
            idx[k] += 1;
            if idx[k] < lists[k].len() {
                break;
            }
            idx[k] = 0;
            k += 1;
        }
    }
}

impl<'a> Resolver<'a> {
    /// Parses property lines. It reports duplicates, unknown properties, bad values and a
    /// shorthand set with a property that it covers. Returns the declarations in source
    /// order with their spans.
    fn own_props(&mut self, items: &[ComponentItem], path: &Path, in_when: bool) -> OwnDecls {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let ctx = Ctx { dir: &dir, root: self.root };
        let mut out: OwnDecls = Vec::new();
        let mut own: HashSet<String> = HashSet::new();
        for item in items {
            let ComponentItem::Property(p) = item else { continue };
            if !own.insert(p.name.clone()) {
                self.diags.error("duplicate-property", Some(path), Some(p.name_span), format!("`{}` is set more than once", p.name));
                continue;
            }
            if !props::is_property(&p.name) {
                let hint = suggest(&p.name);
                self.diags.error("unknown-property", Some(path), Some(p.name_span), format!("`{}` is not a property{hint}", p.name));
                continue;
            }
            if in_when && p.name == "htmlTag" {
                self.diags.error("content-in-when", Some(path), Some(p.name_span), "htmlTag can't be set in a `when` block; an element's tag never depends on an attribute");
                continue;
            }
            match props::parse_property(&p.name, &p.value, &ctx) {
                Ok(lh) => out.push((p.name.clone(), lh, p.name_span)),
                Err(e) => self.diags.error(e.code, Some(path), Some(p.value_span), format!("{}: {}", p.name, e.msg)),
            }
        }
        // A shorthand and a property that it covers, both set here.
        for (n, ls, sp) in &out {
            let lvl = props::level(n).unwrap();
            for (n2, ls2, _) in &out {
                let lvl2 = props::level(n2).unwrap();
                let ls: Vec<L> = ls.iter().map(|(l, _)| *l).collect();
                if lvl < lvl2 && ls2.iter().all(|(l, _)| ls.contains(l)) {
                    self.diags.warning("shorthand-overlap", Some(path), Some(*sp), format!("`{n}` and `{n2}` are both set; `{n2}` wins for what it covers"));
                }
            }
        }
        out
    }

    fn merged_decls(&mut self, fid: FileId, ci: usize) -> Decls {
        if let Some(m) = self.merged.get(&(fid, ci)) {
            return m.clone();
        }
        let comp = &self.files[fid].components[ci];
        let path = self.files[fid].path.clone();
        if !self.visiting.insert((fid, ci)) {
            self.diags.error("extends-cycle", Some(&path), Some(comp.name_span), format!("`{}` extends itself", comp.name));
            return Vec::new();
        }
        let mut merged: Decls = Vec::new();
        let mut attrs: Vec<AttrInfo> = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut seen_bases = HashSet::new();
        for (base, sp) in &comp.extends {
            if !seen_bases.insert(base.clone()) {
                self.diags.warning("duplicate-extends", Some(&path), Some(*sp), format!("`{base}` is listed more than once after `extends`"));
            }
            match lookup(self.files, fid, base) {
                Some(Some(DeclRef::Component(bf, bi))) => {
                    self.used.insert(DeclRef::Component(bf, bi));
                    let decls = self.merged_decls(bf, bi);
                    for d in decls {
                        merged.retain(|(n, _)| *n != d.0);
                        merged.push(d);
                    }
                    for a in self.attrs.get(&(bf, bi)).cloned().unwrap_or_default() {
                        match attrs.iter_mut().find(|x| x.name == a.name) {
                            Some(x) if x.values != a.values => {
                                self.diags.error(
                                    "redeclared-attribute",
                                    Some(&path),
                                    Some(*sp),
                                    format!("`{}` is declared with different values by two components that `{}` extends", a.name, comp.name),
                                );
                            }
                            Some(x) => x.default = a.default,
                            None => attrs.push(a),
                        }
                    }
                    blocks.extend(self.blocks.get(&(bf, bi)).cloned().unwrap_or_default());
                }
                Some(Some(DeclRef::Composition(..))) => {
                    self.diags.error("extends-composition", Some(&path), Some(*sp), format!("`{base}` is a composition; components can only extend components"));
                }
                Some(None) => {} // The code reported this import before.
                None => {
                    self.diags.error("unknown-component", Some(&path), Some(*sp), format!("`{base}` is not a component declared in or imported into this file"));
                }
            }
        }

        // The attributes that the component declares. The order of lines has no meaning,
        // so they are read before the `when` blocks.
        let inherited: HashSet<String> = attrs.iter().map(|a| a.name.clone()).collect();
        let mut own_attrs: HashSet<String> = HashSet::new();
        for item in &comp.body {
            let ComponentItem::Attribute(a) = item else { continue };
            if !own_attrs.insert(a.name.clone()) {
                self.diags.error("duplicate-attribute-declaration", Some(&path), Some(a.name_span), format!("`{}` is declared more than once", a.name));
                continue;
            }
            let default = a.default.as_ref().map(|d| d.0.clone()).unwrap_or_default();
            match &a.values {
                Some(values) => {
                    if props::is_property(&a.name) || matches!(a.name.as_str(), "class" | "style" | "src") {
                        self.diags.error(
                            "invalid-attribute-declaration",
                            Some(&path),
                            Some(a.name_span),
                            format!("`{}` can't be an attribute name; it is a component property or an attribute Emul uses itself", a.name),
                        );
                        continue;
                    }
                    if inherited.contains(&a.name) {
                        self.diags.error(
                            "redeclared-attribute",
                            Some(&path),
                            Some(a.name_span),
                            format!("`{}` is inherited, so its values can't change; write `attribute {} = <value>` to change only its default", a.name, a.name),
                        );
                        continue;
                    }
                    attrs.push(AttrInfo { name: a.name.clone(), values: values.iter().map(|(v, _)| v.clone()).collect(), default });
                }
                None => match attrs.iter_mut().find(|x| x.name == a.name) {
                    None => {
                        self.diags.error("unknown-attribute", Some(&path), Some(a.name_span), format!("`{}` is not an attribute that `{}` inherits, so its default can't be changed", a.name, comp.name));
                    }
                    Some(x) if !x.values.contains(&default) => {
                        let span = a.default.as_ref().map(|d| d.1).unwrap_or(a.name_span);
                        let msg = format!("`{default}` is not a value of `{}`; its values are {}", a.name, x.values.join(", "));
                        self.diags.error("unknown-attribute-value", Some(&path), Some(span), msg);
                    }
                    Some(x) => x.default = default,
                },
            }
        }

        // The own properties of the component.
        for (name, lh, _) in self.own_props(&comp.body, &path, false) {
            merged.retain(|(n, _)| *n != name);
            merged.push((name, lh));
        }

        // The own `when` blocks.
        let mut own_blocks: Vec<(&When, Decls, HashSet<L>)> = Vec::new();
        for item in &comp.body {
            let ComponentItem::When(w) = item else { continue };
            let mut ok = true;
            w.cond.each_cmp(&mut |attr, value| match attrs.iter().find(|a| a.name == attr.0) {
                None => {
                    self.diags.error("unknown-attribute", Some(&path), Some(attr.1), format!("`{}` is not an attribute of `{}`", attr.0, comp.name));
                    ok = false;
                }
                Some(a) if !a.values.contains(&value.0) => {
                    self.diags.error("unknown-attribute-value", Some(&path), Some(value.1), format!("`{}` is not a value of `{}`; its values are {}", value.0, attr.0, a.values.join(", ")));
                    ok = false;
                }
                _ => {}
            });
            let decls: Decls = self.own_props(&w.body, &path, true).into_iter().map(|(n, lh, _)| (n, lh)).collect();
            if !ok {
                continue;
            }
            let longhands: HashSet<L> = decls.iter().flat_map(|(_, lh)| lh.iter().map(|(l, _)| *l)).collect();
            own_blocks.push((w, decls, longhands));
        }
        // Two blocks of this component that can both hold and set the same property.
        for (i, (wa, _, la)) in own_blocks.iter().enumerate() {
            for (wb, _, lb) in &own_blocks[i + 1..] {
                let shared: Vec<&L> = la.intersection(lb).collect();
                if shared.is_empty() || !can_both_hold(&wa.cond, &wb.cond, &attrs) {
                    continue;
                }
                let mut names: Vec<String> = shared.iter().map(|l| css_to_camel(l.css())).collect();
                names.sort();
                self.diags.error(
                    "conflicting-conditions",
                    Some(&path),
                    Some(wb.span),
                    format!(
                        "this block and `when {}` (line {}) can both be true and both set {}; narrow one of the conditions",
                        wa.raw,
                        wa.span.line,
                        names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
                    ),
                );
            }
        }
        blocks.extend(own_blocks.into_iter().map(|(w, decls, _)| Block { cond: w.cond.clone(), decls }));

        self.visiting.remove(&(fid, ci));
        self.merged.insert((fid, ci), merged.clone());
        self.attrs.insert((fid, ci), attrs);
        self.blocks.insert((fid, ci), blocks);
        merged
    }

    /// The attributes of a component, after inheritance.
    fn attrs(&mut self, fid: FileId, ci: usize) -> Vec<AttrInfo> {
        self.merged_decls(fid, ci);
        self.attrs.get(&(fid, ci)).cloned().unwrap_or_default()
    }

    /// The style of a component with every attribute at its default.
    fn style(&mut self, fid: FileId, ci: usize) -> Arc<Style> {
        let values = self.attrs(fid, ci).into_iter().map(|a| a.default).collect::<Vec<_>>();
        self.style_with(fid, ci, &values)
    }

    /// The style of a component for the given attribute values, in the order of its
    /// attributes. The base comes first, then each `when` block whose condition is true.
    fn style_with(&mut self, fid: FileId, ci: usize, values: &[String]) -> Arc<Style> {
        let key = (fid, ci, values.to_vec());
        if let Some(s) = self.styles.get(&key) {
            return s.clone();
        }
        let decls = self.merged_decls(fid, ci);
        let mut set = resolve_decls(&decls);
        let attrs = self.attrs.get(&(fid, ci)).cloned().unwrap_or_default();
        let get = |n: &str| attrs.iter().position(|a| a.name == n).and_then(|i| values.get(i).cloned());
        for b in self.blocks.get(&(fid, ci)).cloned().unwrap_or_default() {
            if b.cond.eval(&get) {
                set.extend(resolve_decls(&b.decls));
            }
        }
        let style = Arc::new(Style { set: set.into_iter().collect() });
        self.styles.insert(key, style.clone());
        style
    }
}

fn suggest(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let best = props::ALL_PROPERTY_NAMES
        .iter()
        .map(|p| (strsim(&lower, &p.to_ascii_lowercase()), p))
        .min_by_key(|(d, _)| *d);
    match best {
        Some((d, p)) if d <= 2 => format!("; did you mean `{p}`?"),
        _ => String::new(),
    }
}

/// The Levenshtein distance.
fn strsim(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

pub fn load(entry: &Path, root: &Path, only: Option<&[String]>) -> Project {
    let mut loader = Loader { root: root.to_path_buf(), files: Vec::new(), by_path: HashMap::new(), sources: HashMap::new(), diags: Diags::new() };
    let entry = props::normalize(entry);
    if !entry.is_file() {
        loader.diags.error("missing-entry", Some(&entry), None, "the entry file does not exist");
        return Project { root: root.to_path_buf(), entry, files: Vec::new(), sources: HashMap::new(), diags: loader.diags, compositions: Vec::new() };
    }
    let entry_id = loader.load(&entry).unwrap_or(0);
    loader.link();
    let Loader { files, sources, mut diags, .. } = loader;

    let mut res = Resolver { root, files: &files, diags: &mut diags, styles: HashMap::new(), merged: HashMap::new(), attrs: HashMap::new(), blocks: HashMap::new(), visiting: HashSet::new(), used: HashSet::new() };
    // Resolve every component. Then the compiler reports the errors of each file,
    // also for unused components.
    for fid in 0..files.len() {
        for ci in 0..files[fid].components.len() {
            res.style(fid, ci);
        }
    }

    // The build renders the compositions that the entry exports.
    let mut rendered: Vec<(String, FileId, usize)> = Vec::new();
    if let Some(entry_file) = files.get(entry_id) {
        let mut names: Vec<(&String, &DeclRef)> = entry_file.exports.iter().collect();
        names.sort_by_key(|(n, _)| n.as_str());
        for (name, d) in names {
            if let DeclRef::Composition(f, i) = d {
                rendered.push((name.clone(), *f, *i));
            }
        }
    }
    let rendered_set: HashSet<(FileId, usize)> = rendered.iter().map(|(_, f, i)| (*f, *i)).collect();

    // Build the trees, and check every composition in every file.
    let mut class_names: HashMap<(FileId, usize), String> = HashMap::new();
    let mut taken: HashMap<String, usize> = HashMap::new();
    let mut compositions = Vec::new();
    let mut counter = 0usize;
    let mut instanced: HashSet<(FileId, usize)> = HashSet::new();
    let mut not_rendered: Vec<(FileId, usize)> = Vec::new();
    for fid in 0..files.len() {
        for (ci, comp) in files[fid].compositions.iter().enumerate() {
            let mut b = TreeBuilder {
                res: &mut res,
                class_names: &mut class_names,
                taken: &mut taken,
                counter: &mut counter,
                stack: Vec::new(),
                instanced: &mut instanced,
            };
            let root = b.root(fid, ci);
            if comp.body.iter().all(|e| matches!(e, Entry::Blank | Entry::Comment(_))) {
                b.res.diags.warning("empty-composition", Some(&files[fid].path), Some(comp.name_span), format!("`{}` has nothing in it", comp.name));
            }
            if rendered_set.contains(&(fid, ci)) {
                for (name, f, i) in &rendered {
                    if *f == fid && *i == ci {
                        compositions.push(RenderedComposition { name: name.clone(), root: root.clone() });
                    }
                }
            } else if fid != entry_id && comp.exported {
                not_rendered.push((fid, ci));
            }
        }
    }
    for (fid, ci) in not_rendered {
        if !instanced.contains(&(fid, ci)) {
            let comp = &files[fid].compositions[ci];
            res.diags.warning(
                "not-rendered",
                Some(&files[fid].path),
                Some(comp.name_span),
                format!("`{}` is exported here but not from the entry file, and no composition instances it, so it is never used", comp.name),
            );
        }
    }
    let used = res.used.clone();
    drop(res);

    // Unused declarations and imports.
    for (fid, f) in files.iter().enumerate() {
        for (ci, c) in f.components.iter().enumerate() {
            if !c.exported && !used.contains(&DeclRef::Component(fid, ci)) && !f.exports.values().any(|d| *d == DeclRef::Component(fid, ci)) {
                diags.warning("unused-declaration", Some(&f.path), Some(c.name_span), format!("`{}` is never used", c.name));
            }
        }
        for (ci, c) in f.compositions.iter().enumerate() {
            let r = DeclRef::Composition(fid, ci);
            if !c.exported && !f.exports.values().any(|d| *d == r) && !instanced.contains(&(fid, ci)) {
                diags.warning("unused-declaration", Some(&f.path), Some(c.name_span), format!("composition `{}` is not exported and no composition instances it, so it is never used", c.name));
            }
        }
        for (name, b) in &f.scope {
            if let Binding::Imported { used, span, .. } = b {
                let exported = f.exports.contains_key(name);
                if !used.get() && !exported {
                    diags.warning("unused-import", Some(&f.path), Some(*span), format!("`{name}` is imported but never used"));
                }
            }
        }
    }
    if only.is_none() && compositions.is_empty() && !files.is_empty() {
        diags.warning("nothing-to-render", Some(&entry), None, "the entry file exports no compositions, so nothing is rendered");
    }
    if let Some(only) = only {
        compositions.retain(|c| only.contains(&c.name));
    }
    Project { root: root.to_path_buf(), entry, files, sources, diags, compositions }
}

struct TreeBuilder<'r, 'a> {
    res: &'r mut Resolver<'a>,
    class_names: &'r mut HashMap<(FileId, usize), String>,
    taken: &'r mut HashMap<String, usize>,
    counter: &'r mut usize,
    /// The compositions that this step builds now, outermost first. This list finds
    /// compositions that contain themselves.
    stack: Vec<(FileId, usize)>,
    /// Every composition that has an instance in another composition.
    instanced: &'r mut HashSet<(FileId, usize)>,
}

impl<'r, 'a> TreeBuilder<'r, 'a> {
    fn root(&mut self, fid: FileId, ci: usize) -> Vec<Node> {
        self.expand(fid, ci, None)
    }

    /// Builds the root instances of a composition. `parent` arranges them. `parent` is None
    /// at the root of the canvas.
    fn expand(&mut self, fid: FileId, ci: usize, parent: Option<&Style>) -> Vec<Node> {
        let files = self.res.files;
        let comp = &files[fid].compositions[ci];
        let path = files[fid].path.clone();
        self.stack.push((fid, ci));
        let mut out = Vec::new();
        for e in &comp.body {
            match e {
                Entry::Instance(i) => out.extend(self.instance(fid, i, parent)),
                Entry::Text(t) => self.res.diags.error("content-in-root", Some(&path), Some(t.span), "the root of a composition holds instances only, not text"),
                Entry::Attribute(a) => self.res.diags.error("content-in-root", Some(&path), Some(a.name_span), "the root of a composition holds instances only, not attributes"),
                _ => {}
            }
        }
        self.stack.pop();
        out
    }

    /// Builds an instance of a composition. The instance draws the root instances of that
    /// composition in its place.
    fn composition_instance(&mut self, fid: FileId, inst: &Instance, cf: FileId, ci: usize, parent: Option<&Style>) -> Vec<Node> {
        let path = self.res.files[fid].path.clone();
        self.instanced.insert((cf, ci));
        for e in &inst.body {
            let span = match e {
                Entry::Text(t) => t.span,
                Entry::Attribute(a) => a.name_span,
                Entry::Instance(i) => i.name_span,
                _ => continue,
            };
            self.res.diags.error(
                "content-in-composition-instance",
                Some(&path),
                Some(span),
                format!("`{}` is a composition, so its instance can't hold text, attributes or children", inst.name),
            );
        }
        if self.stack.contains(&(cf, ci)) {
            let chain: Vec<String> = self.stack.iter().skip_while(|c| **c != (cf, ci)).map(|(f, i)| self.res.files[*f].compositions[*i].name.clone()).collect();
            self.res.diags.error(
                "composition-cycle",
                Some(&path),
                Some(inst.name_span),
                format!("`{}` contains itself: {} -> {}", inst.name, chain.join(" -> "), self.res.files[cf].compositions[ci].name),
            );
            return Vec::new();
        }
        self.expand(cf, ci, parent)
    }

    fn class_for(&mut self, fid: FileId, ci: usize) -> String {
        if let Some(c) = self.class_names.get(&(fid, ci)) {
            return c.clone();
        }
        let name = self.res.files[fid].components[ci].name.clone();
        let n = self.taken.entry(name.clone()).or_insert(0);
        *n += 1;
        let class = if *n == 1 { name } else { format!("{name}-{n}") };
        self.class_names.insert((fid, ci), class.clone());
        class
    }

    fn instance(&mut self, fid: FileId, inst: &Instance, parent: Option<&Style>) -> Vec<Node> {
        let path = self.res.files[fid].path.clone();
        let (cf, ci) = match lookup(self.res.files, fid, &inst.name) {
            Some(Some(DeclRef::Component(cf, ci))) => (cf, ci),
            Some(Some(DeclRef::Composition(cf, ci))) => return self.composition_instance(fid, inst, cf, ci, parent),
            Some(None) => return Vec::new(),
            None => {
                self.res.diags.error(
                    "unknown-component",
                    Some(&path),
                    Some(inst.name_span),
                    format!("`{}` is not a component or composition declared in or imported into this file", inst.name),
                );
                return Vec::new();
            }
        };
        self.res.used.insert(DeclRef::Component(cf, ci));
        // The values of the declared attributes. They choose the `when` blocks.
        let declared = self.res.attrs(cf, ci);
        let mut values: Vec<String> = declared.iter().map(|a| a.default.clone()).collect();
        let mut set_declared: HashSet<String> = HashSet::new();
        for e in &inst.body {
            let Entry::Attribute(a) = e else { continue };
            let Some(k) = declared.iter().position(|d| d.name == a.name) else { continue };
            if !set_declared.insert(a.name.clone()) {
                self.res.diags.error("duplicate-attribute", Some(&path), Some(a.name_span), format!("`{}` is set more than once", a.name));
                continue;
            }
            if declared[k].values.contains(&a.value) {
                values[k] = a.value.clone();
            } else {
                self.res.diags.error(
                    "unknown-attribute-value",
                    Some(&path),
                    Some(a.value_span),
                    format!("`{}` is not a value of `{}`; its values are {}", a.value, a.name, declared[k].values.join(", ")),
                );
            }
        }
        let style = self.res.style_with(cf, ci, &values);
        let mut class = self.class_for(cf, ci);
        for (d, v) in declared.iter().zip(&values) {
            if *v != d.default {
                class.push('-');
                class.push_str(v);
            }
        }
        *self.counter += 1;
        let id = format!("{}-{}", class, self.counter);
        let tag = style.tag();
        let is_void = props::VOID_TAGS.contains(&tag.as_str());
        let inline = style.display() == "inline";
        self.check_no_effect(&path, inst, &style, parent);

        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut children = Vec::new();
        let mut src = None;
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        for e in &inst.body {
            match e {
                Entry::Text(t) => {
                    if is_void {
                        self.res.diags.error("content-in-void-element", Some(&path), Some(t.span), format!("`{}` is an `{tag}` element, which can't hold text", inst.name));
                    }
                    children.push(Child::Text(t.value.clone()));
                }
                Entry::Instance(i) => {
                    if is_void {
                        self.res.diags.error("content-in-void-element", Some(&path), Some(i.name_span), format!("`{}` is an `{tag}` element, which can't hold children", inst.name));
                    }
                    for n in self.instance(fid, i, Some(&style)) {
                        if inline && n.style.display() != "inline" {
                            self.res.diags.error("block-in-inline", Some(&path), Some(i.name_span), format!("`{}` is inline, so it can only hold text and other inline instances", inst.name));
                        }
                        children.push(Child::Node(n));
                    }
                }
                Entry::Attribute(a) if declared.iter().any(|d| d.name == a.name) => {}
                Entry::Attribute(a) => {
                    let name = a.name.as_str();
                    let valid = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                    if props::is_property(name) {
                        self.res.diags.error("style-attribute", Some(&path), Some(a.name_span), format!("`{name}` is a style property; styling belongs in components, not instances"));
                        continue;
                    }
                    if !valid {
                        self.res.diags.error("invalid-attribute-name", Some(&path), Some(a.name_span), format!("`{name}` is not a valid attribute name; use lowercase letters, digits and hyphens"));
                        continue;
                    }
                    if name == "class" || name == "style" {
                        self.res.diags.error("reserved-attribute", Some(&path), Some(a.name_span), format!("`{name}` is set by Emul itself"));
                        continue;
                    }
                    if attrs.iter().any(|(n, _)| n == name) {
                        self.res.diags.error("duplicate-attribute", Some(&path), Some(a.name_span), format!("`{name}` is set more than once"));
                        continue;
                    }
                    if name == "src" {
                        let ctx = Ctx { dir: &dir, root: self.res.root };
                        match props::resolve_image_path(&a.value, &ctx) {
                            Ok(p) => src = Some(p),
                            Err(e) => self.res.diags.error(e.code, Some(&path), Some(a.value_span), e.msg),
                        }
                    }
                    attrs.push((name.to_string(), a.value.clone()));
                }
                _ => {}
            }
        }
        // Every declared attribute is written with a `data-` prefix, also at its default.
        for (d, v) in declared.iter().zip(&values) {
            let name = format!("data-{}", d.name);
            if attrs.iter().any(|(n, _)| *n == name) {
                self.res.diags.error("duplicate-attribute", Some(&path), Some(inst.name_span), format!("`{name}` is set on `{}`, but `{}` is a declared attribute that Emul writes as `{name}`", inst.name, d.name));
                continue;
            }
            attrs.push((name, v.clone()));
        }
        if tag == "img" && !attrs.iter().any(|(n, _)| n == "src") {
            self.res.diags.error("missing-src", Some(&path), Some(inst.name_span), format!("`{}` is an image, but has no `src` attribute", inst.name));
        }
        vec![Node { class, style, attrs, src, children, id }]
    }

    fn check_no_effect(&mut self, path: &Path, inst: &Instance, style: &Style, parent: Option<&Style>) {
        let mut warn = |msg: String| {
            self.res.diags.warning("no-effect", Some(path), Some(inst.name_span), msg);
        };
        let display = style.display();
        let has = |l: L| style.get(l).is_some();
        if display == "inline" {
            if let Some(l) = style.set.keys().find(|l| l.is_box()) {
                warn(format!("`{}` is inline, so `{}` has no effect", inst.name, css_to_camel(l.css())));
            }
        }
        if display != "flex" && display != "grid" && (has(L::RowGap) || has(L::ColumnGap)) {
            warn(format!("`{}` is not a flex or grid container, so its gap has no effect", inst.name));
        }
        if !style.is_positioned() && (has(L::Top) || has(L::Right) || has(L::Bottom) || has(L::Left) || has(L::ZIndex)) {
            warn(format!("`{}` has position static, so top, right, bottom, left and zIndex have no effect", inst.name));
        }
        let parent_display = parent.map(|p| p.display()).unwrap_or("block");
        if parent_display != "flex" && (has(L::FlexGrow) || has(L::FlexShrink) || has(L::FlexBasis)) && style.kw(L::Position) != "absolute" {
            warn(format!("`{}` is not inside a flex container, so its flex properties have no effect", inst.name));
        }
        if has(L::TextOverflow) && style.kw(L::TextOverflow) == "ellipsis" && style.kw(L::Overflow) != "hidden" && style.int(L::LineClamp).is_none() {
            warn(format!("`{}` needs overflow: hidden for textOverflow to show an ellipsis", inst.name));
        }
        if matches!(style.get(L::Height), Some(V::Len(crate::values::Len::Pct(_)))) {
            if let Some(p) = parent {
                if matches!(p.len(L::Height), crate::values::Len::Auto) && p.display() != "grid" && p.display() != "flex" {
                    self.res.diags.warning(
                        "percentage-height-ignored",
                        Some(path),
                        Some(inst.name_span),
                        format!("`{}` has a percentage height, but its parent's height is auto, so it is treated as auto", inst.name),
                    );
                }
            }
        }
    }
}

pub fn css_to_camel(css: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in css.chars() {
        if c == '-' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(src: &str) -> Project {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("emul-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("index.emul");
        std::fs::write(&entry, src).unwrap();
        let p = load(&entry, &dir, None);
        let _ = std::fs::remove_dir_all(&dir);
        p
    }

    fn codes(p: &Project) -> Vec<&'static str> {
        p.diags.list.iter().map(|d| d.code).collect()
    }

    const BUTTON: &str = "component Button:\n    attribute size: small | large = large\n    attribute tone: plain | danger = plain\n    padding: 8px\n    when size == small:\n        padding: 4px\n    when tone == danger:\n        color: #ff0000\n";

    #[test]
    fn instance_values_pick_blocks_and_classes() {
        let src = format!("{BUTTON}\ncomponent Wide extends Button:\n    attribute size = small\n    padding: 20px\n\nexport composition X {{\n    Button {{ }}\n    Button {{ size: small, tone: danger }}\n    Wide {{ }}\n    Wide {{ size: large }}\n}}\n");
        let p = build(&src);
        assert!(!p.diags.has_errors(), "{:?}", p.diags);
        let root = &p.compositions[0].root;
        let classes: Vec<&str> = root.iter().map(|n| n.class.as_str()).collect();
        assert_eq!(classes, ["Button", "Button-small-danger", "Wide", "Wide-large"]);
        let pad = |i: usize| format!("{:?}", root[i].style.get(L::PaddingTop));
        assert!(pad(0).contains('8'));
        assert!(pad(1).contains('4'));
        // The inherited block for `small` wins over the base padding of `Wide`.
        assert!(pad(2).contains('4'));
        assert!(pad(3).contains("20"));
        assert!(root[1].attrs.contains(&("data-tone".into(), "danger".into())));
        assert!(root[0].attrs.contains(&("data-size".into(), "large".into())));
    }

    #[test]
    fn conflicting_blocks() {
        let p = build("component A:\n    attribute s: a | b = a\n    attribute t: c | d = c\n    when s == a:\n        padding: 1px\n    when t == c:\n        paddingTop: 2px\n    when s == b && t == d:\n        margin: 1px\n    when s == a && s == b:\n        margin: 2px\n");
        // Only the first pair can hold together and set the same property.
        assert_eq!(codes(&p).iter().filter(|c| **c == "conflicting-conditions").count(), 1, "{:?}", p.diags);
    }

    #[test]
    fn bad_instance_value_and_redeclared() {
        let src = format!("{BUTTON}\ncomponent B extends Button:\n    attribute size: small | large = small\n\nexport composition X {{\n    Button {{ size: huge }}\n    B {{ }}\n}}\n");
        let c = codes(&build(&src));
        assert!(c.contains(&"unknown-attribute-value"), "{c:?}");
        assert!(c.contains(&"redeclared-attribute"), "{c:?}");
    }
}
