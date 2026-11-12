//! `emul init`: writes the config, the entry file and the .gitignore of a new project.

use crate::config::Format;
use std::path::{Path, PathBuf};

/// Names that the starter `index.emul` declares. The composition must not use them again.
const STARTER_COMPONENTS: [&str; 4] = ["Page", "Card", "Title", "Copy"];

#[derive(Clone, Debug)]
pub struct InitOptions {
    pub dir: PathBuf,
    pub name: Option<String>,
    /// The width as the user typed it. The code checked it before.
    pub width: Option<u32>,
    /// `auto` or a number of px. The code checked it before.
    pub height: Option<String>,
    /// The colour as the user typed it. The code checked it before.
    pub background: Option<String>,
    pub formats: Vec<Format>,
    pub out_dir: Option<String>,
    pub force: bool,
}

pub enum InitError {
    /// The files that exist now, when there is no `--force`.
    Exists(Vec<PathBuf>),
    Io(String),
}

/// True for a capital letter and then letters and digits.
pub fn is_pascal(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_uppercase()) && c.all(|x| x.is_ascii_alphanumeric())
}

/// `my-designs` → `MyDesigns`. The result is `Project` when the name is not valid.
pub fn name_from_dir(dir: &Path) -> String {
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let base = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let raw = base.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name: String = raw
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            let first = c.next().unwrap().to_ascii_uppercase();
            std::iter::once(first).chain(c).collect::<String>()
        })
        .collect();
    if is_pascal(&name) { name } else { "Project".into() }
}

pub fn config_text(name: &str, o: &InitOptions) -> String {
    let formats = if o.formats.is_empty() { vec![Format::Html, Format::Svg, Format::Png] } else { o.formats.clone() };
    let mut s = String::new();
    s.push_str("use @emul/config\n\n");
    s.push_str(&format!("export emul-config {name}:\n"));
    s.push_str("    entry: ./index.emul\n");
    s.push_str(&format!("    outDir: {}\n", o.out_dir.as_deref().unwrap_or("./out")));
    s.push_str("    formats:\n");
    for f in formats {
        s.push_str(&format!("        - {}\n", f.ext()));
    }
    s.push_str(&format!("    width: {}\n", o.width.unwrap_or(1280)));
    s.push_str(&format!("    height: {}\n", o.height.as_deref().unwrap_or("auto")));
    match &o.background {
        Some(b) => s.push_str(&format!("    background: {b}\n")),
        None => s.push_str("    // background: none\n"),
    }
    s.push_str("    // svgFonts: outline\n");
    s
}

pub fn entry_text(name: &str) -> String {
    let comp = if STARTER_COMPONENTS.contains(&name) { format!("{name}Design") } else { name.to_string() };
    format!(
        r#"component Page:
    display: flex
    padding: 48px
    backgroundColor: #f4f4f5

component Card:
    display: flex
    flexDirection: column
    gap: 8px
    padding: 32px
    borderRadius: 12px
    backgroundColor: #ffffff
    boxShadow: 0 4px 12px rgba(0, 0, 0, 0.12)

component Title:
    fontSize: 24px
    fontWeight: 600
    color: #18181b

component Copy:
    fontSize: 16px
    color: #52525b

export composition {comp} {{
    Page {{
        Card {{
            Title {{ "{name}" }}
            Copy {{ "Edit index.emul, then run emul preview." }}
        }}
    }}
}}
"#
    )
}

fn gitignore_text(out_dir: &str) -> String {
    let d = out_dir.trim_start_matches("./").trim_end_matches('/');
    format!("# Emul render output\n/{d}/\n")
}

/// Writes the project. Returns the files that it wrote, in order.
pub fn run(o: &InitOptions) -> Result<Vec<PathBuf>, InitError> {
    let config = o.dir.join("emul.config.pi");
    let entry = o.dir.join("index.emul");
    let gitignore = o.dir.join(".gitignore");
    if !o.force {
        let existing: Vec<PathBuf> = [&config, &entry].into_iter().filter(|p| p.exists()).cloned().collect();
        if !existing.is_empty() {
            return Err(InitError::Exists(existing));
        }
    }
    let mkdir = if o.dir.as_os_str().is_empty() { Path::new(".") } else { o.dir.as_path() };
    std::fs::create_dir_all(mkdir).map_err(|e| InitError::Io(format!("can't create {}: {e}", o.dir.display())))?;
    let name = o.name.clone().unwrap_or_else(|| name_from_dir(&o.dir));
    let mut files = vec![(config, config_text(&name, o)), (entry, entry_text(&name))];
    if !gitignore.exists() {
        files.push((gitignore, gitignore_text(o.out_dir.as_deref().unwrap_or("./out"))));
    }
    let mut written = Vec::new();
    for (path, text) in files {
        std::fs::write(&path, text).map_err(|e| InitError::Io(format!("can't write {}: {e}", path.display())))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(name_from_dir(Path::new("/x/my-designs")), "MyDesigns");
        assert_eq!(name_from_dir(Path::new("/x/site_v2")), "SiteV2");
        assert_eq!(name_from_dir(Path::new("/x/2024")), "Project");
        assert!(is_pascal("Marketing") && !is_pascal("marketing") && !is_pascal("My-App"));
    }

    #[test]
    fn starter_is_formatted_and_avoids_clashes() {
        for name in ["Starter", "Card"] {
            let src = entry_text(name);
            let out = crate::format::format_source(Path::new("index.emul"), &src).ok().unwrap();
            assert_eq!(out, src);
        }
        assert!(entry_text("Card").contains("export composition CardDesign {"));
    }
}
