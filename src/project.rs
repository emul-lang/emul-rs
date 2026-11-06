//! Loads a project: parses every file reachable from the entry, resolves imports and
//! exports, resolves component inheritance, and builds the instance tree of every
//! rendered composition.

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
    /// Names known in this file: local declarations and imports.
    pub scope: HashMap<String, Binding>,
    /// Names this file exports.
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
    /// The class name used in HTML, which is the component's declared name, made unique.
    pub class: String,
    pub style: Arc<Style>,
    pub attrs: Vec<(String, String)>,
    /// The resolved image file for `img` instances.
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

/// Resolves an import path, relative to `from_dir`, or to the root when it starts with `/`.
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
        // Load every imported file, so circular imports are simply already loaded.
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

    /// Fills in every file's scope and exports. Exports can depend on other files'
    /// exports, so this repeats until nothing changes.
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
        // Imports and re-exports, iterated to a fixed point.
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
        // Now bind imports and report problems.
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

    /// What an import of `name` in file `fid` points at, looking through import items directly.
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

/// Looks up a name in a file's scope, marking imports used.
fn lookup(files: &[LoadedFile], fid: FileId, name: &str) -> Option<Option<DeclRef>> {
    match files[fid].scope.get(name)? {
        Binding::Local(d) => Some(Some(*d)),
        Binding::Imported { decl, used, .. } => {
            used.set(true);
            Some(*decl)
        }
    }
}

struct Resolver<'a> {
    root: &'a Path,
    files: &'a [LoadedFile],
    diags: &'a mut Diags,
    styles: HashMap<(FileId, usize), Arc<Style>>,
    /// Declarations as written, merged through inheritance: (property, value, file of the
    /// declaration, longhands). Kept in merge order for the tie-break between equally
    /// specific shorthands.
    merged: HashMap<(FileId, usize), Vec<(String, Vec<(L, V)>)>>,
    visiting: HashSet<(FileId, usize)>,
    used: HashSet<DeclRef>,
}

impl<'a> Resolver<'a> {
    fn merged_decls(&mut self, fid: FileId, ci: usize) -> Vec<(String, Vec<(L, V)>)> {
        if let Some(m) = self.merged.get(&(fid, ci)) {
            return m.clone();
        }
        let comp = &self.files[fid].components[ci];
        let path = self.files[fid].path.clone();
        if !self.visiting.insert((fid, ci)) {
            self.diags.error("extends-cycle", Some(&path), Some(comp.name_span), format!("`{}` extends itself", comp.name));
            return Vec::new();
        }
        let mut merged: Vec<(String, Vec<(L, V)>)> = Vec::new();
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
                }
                Some(Some(DeclRef::Composition(..))) => {
                    self.diags.error("extends-composition", Some(&path), Some(*sp), format!("`{base}` is a composition; components can only extend components"));
                }
                Some(None) => {} // import already reported
                None => {
                    self.diags.error("unknown-component", Some(&path), Some(*sp), format!("`{base}` is not a component declared in or imported into this file"));
                }
            }
        }
        // The component's own properties.
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let ctx = Ctx { dir: &dir, root: self.root };
        let mut own: HashMap<String, Span> = HashMap::new();
        let mut own_longhands: Vec<(String, u8, Vec<L>, Span)> = Vec::new();
        for item in &comp.body {
            let ComponentItem::Property(p) = item else { continue };
            if own.contains_key(&p.name) {
                self.diags.error("duplicate-property", Some(&path), Some(p.name_span), format!("`{}` is set more than once", p.name));
                continue;
            }
            own.insert(p.name.clone(), p.name_span);
            if !props::is_property(&p.name) {
                let hint = suggest(&p.name);
                self.diags.error("unknown-property", Some(&path), Some(p.name_span), format!("`{}` is not a property{hint}", p.name));
                continue;
            }
            match props::parse_property(&p.name, &p.value, &ctx) {
                Ok(lh) => {
                    own_longhands.push((p.name.clone(), props::level(&p.name).unwrap(), lh.iter().map(|(l, _)| *l).collect(), p.name_span));
                    merged.retain(|(n, _)| *n != p.name);
                    merged.push((p.name.clone(), lh));
                }
                Err(e) => {
                    let span = if e.code == "unknown-font-family" || e.code.starts_with("missing") || e.code == "unsupported-image" || e.code == "invalid-html-tag" {
                        p.value_span
                    } else {
                        p.value_span
                    };
                    self.diags.error(e.code, Some(&path), Some(span), format!("{}: {}", p.name, e.msg));
                }
            }
        }
        // A shorthand and a property it covers, both set here.
        for (n, lvl, ls, sp) in &own_longhands {
            for (n2, lvl2, ls2, _) in &own_longhands {
                if lvl < lvl2 && ls2.iter().all(|l| ls.contains(l)) {
                    self.diags.warning(
                        "shorthand-overlap",
                        Some(&path),
                        Some(*sp),
                        format!("`{n}` and `{n2}` are both set; `{n2}` wins for what it covers"),
                    );
                }
            }
        }
        self.visiting.remove(&(fid, ci));
        self.merged.insert((fid, ci), merged.clone());
        merged
    }

    fn style(&mut self, fid: FileId, ci: usize) -> Arc<Style> {
        if let Some(s) = self.styles.get(&(fid, ci)) {
            return s.clone();
        }
        let decls = self.merged_decls(fid, ci);
        // The shorthand rule: for each longhand, the most specific declaration wins; ties
        // go to whichever comes later in merge order.
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
        let style = Arc::new(Style { set: best.into_iter().map(|(l, (_, _, v))| (l, v)).collect() });
        self.styles.insert((fid, ci), style.clone());
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

/// Levenshtein distance.
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

    let mut res = Resolver { root, files: &files, diags: &mut diags, styles: HashMap::new(), merged: HashMap::new(), visiting: HashSet::new(), used: HashSet::new() };
    // Resolve every component, so every file's errors are reported even if unused.
    for fid in 0..files.len() {
        for ci in 0..files[fid].components.len() {
            res.style(fid, ci);
        }
    }

    // Which compositions are rendered: those exported from the entry.
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

    // Build trees, checking every composition in every file.
    let mut class_names: HashMap<(FileId, usize), String> = HashMap::new();
    let mut taken: HashMap<String, usize> = HashMap::new();
    let mut compositions = Vec::new();
    let mut counter = 0usize;
    for fid in 0..files.len() {
        for (ci, comp) in files[fid].compositions.iter().enumerate() {
            let mut b = TreeBuilder { res: &mut res, class_names: &mut class_names, taken: &mut taken, counter: &mut counter };
            let root = b.root(fid, comp);
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
                b.res.diags.warning(
                    "not-rendered",
                    Some(&files[fid].path),
                    Some(comp.name_span),
                    format!("`{}` is exported here but not from the entry file, so it is never rendered", comp.name),
                );
            }
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
            if !c.exported && !f.exports.values().any(|d| *d == r) {
                diags.warning("unused-declaration", Some(&f.path), Some(c.name_span), format!("composition `{}` is not exported, so it is never rendered", c.name));
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
}

impl<'r, 'a> TreeBuilder<'r, 'a> {
    fn root(&mut self, fid: FileId, comp: &Composition) -> Vec<Node> {
        let path = self.res.files[fid].path.clone();
        let mut out = Vec::new();
        for e in &comp.body {
            match e {
                Entry::Instance(i) => {
                    if let Some(n) = self.instance(fid, i, None) {
                        out.push(n);
                    }
                }
                Entry::Text(t) => self.res.diags.error("content-in-root", Some(&path), Some(t.span), "the root of a composition holds instances only, not text"),
                Entry::Attribute(a) => self.res.diags.error("content-in-root", Some(&path), Some(a.name_span), "the root of a composition holds instances only, not attributes"),
                _ => {}
            }
        }
        out
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

    fn instance(&mut self, fid: FileId, inst: &Instance, parent: Option<&Style>) -> Option<Node> {
        let path = self.res.files[fid].path.clone();
        let (cf, ci) = match lookup(self.res.files, fid, &inst.name) {
            Some(Some(DeclRef::Component(cf, ci))) => (cf, ci),
            Some(Some(DeclRef::Composition(..))) => {
                self.res.diags.error("instance-of-composition", Some(&path), Some(inst.name_span), format!("`{}` is a composition; only components can be placed in a composition", inst.name));
                return None;
            }
            Some(None) => return None,
            None => {
                self.res.diags.error("unknown-component", Some(&path), Some(inst.name_span), format!("`{}` is not a component declared in or imported into this file", inst.name));
                return None;
            }
        };
        self.res.used.insert(DeclRef::Component(cf, ci));
        let style = self.res.style(cf, ci);
        let class = self.class_for(cf, ci);
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
                    if let Some(n) = self.instance(fid, i, Some(&style)) {
                        if inline && n.style.display() != "inline" {
                            self.res.diags.error("block-in-inline", Some(&path), Some(i.name_span), format!("`{}` is inline, so it can only hold text and other inline instances", inst.name));
                        }
                        children.push(Child::Node(n));
                    }
                }
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
        if tag == "img" && !attrs.iter().any(|(n, _)| n == "src") {
            self.res.diags.error("missing-src", Some(&path), Some(inst.name_span), format!("`{}` is an image, but has no `src` attribute", inst.name));
        }
        Some(Node { class, style, attrs, src, children, id })
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
