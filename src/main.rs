use clap::{Args, Parser, Subcommand};
use emul::build::{self, Overrides};
use emul::config::{self, Format, SvgFonts};
use emul::diag::Diags;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "emul", version, about = "Compile Emul designs to HTML, SVG and PNG")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the project, then render every composition exported from the entry file.
    Build {
        /// Render only these compositions.
        compositions: Vec<String>,
        #[command(flatten)]
        flags: RenderFlags,
    },
    /// Report errors and warnings without rendering anything.
    Check {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Rewrite .emul files in their canonical layout.
    Format {
        /// Files or directories to format. Defaults to the project's source root.
        paths: Vec<PathBuf>,
        /// Change nothing, and exit with code 1 if any file is not formatted.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Open a viewer that refreshes whenever a file is saved.
    Preview {
        #[command(flatten)]
        flags: RenderFlags,
        /// The port the HTML view is served on.
        #[arg(long, default_value_t = 4400)]
        port: u16,
        /// Also open the HTML view in the browser at start-up.
        #[arg(long)]
        html: bool,
    },
}

#[derive(Args, Clone)]
struct RenderFlags {
    /// Canvas width in px.
    #[arg(long)]
    width: Option<String>,
    /// Canvas height in px, or `auto`.
    #[arg(long)]
    height: Option<String>,
    /// PNG pixel density.
    #[arg(long)]
    scale: Option<f32>,
    /// Canvas background colour.
    #[arg(long)]
    background: Option<String>,
    /// Render only this format: html, svg or png. May be repeated.
    #[arg(long = "format")]
    format: Vec<String>,
    /// How SVG text is written: outline or embed.
    #[arg(long = "svg-fonts")]
    svg_fonts: Option<String>,
    /// Output directory.
    #[arg(long = "out")]
    out: Option<PathBuf>,
    /// Config file to use.
    #[arg(long)]
    config: Option<PathBuf>,
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::from(2)
}

fn overrides(f: &RenderFlags) -> Result<Overrides, String> {
    let mut o = Overrides::default();
    if let Some(w) = &f.width {
        o.width = Some(config::parse_dim(w).ok_or("--width must be a whole number from 1 to 16384")?);
    }
    if let Some(h) = &f.height {
        o.height = Some(config::parse_height(h).ok_or("--height must be `auto` or a whole number from 1 to 16384")?);
    }
    if let Some(s) = f.scale {
        if s <= 0.0 {
            return Err("--scale must be a positive number".into());
        }
        o.scale = Some(s);
    }
    if let Some(b) = &f.background {
        let toks = emul::values::tokenize(b).map_err(|e| format!("--background: {e}"))?;
        let t = toks.first().ok_or("--background needs a colour")?;
        o.background = Some(emul::values::parse_color(t).map_err(|e| format!("--background: {e}"))?);
    }
    if !f.format.is_empty() {
        let mut v = Vec::new();
        for s in &f.format {
            v.push(Format::parse(s).ok_or_else(|| format!("`{s}` is not a format; use html, svg or png"))?);
        }
        o.formats = Some(v);
    }
    if let Some(s) = &f.svg_fonts {
        o.svg_fonts = Some(match s.as_str() {
            "outline" => SvgFonts::Outline,
            "embed" => SvgFonts::Embed,
            _ => return Err("--svg-fonts is `outline` or `embed`".into()),
        });
    }
    o.out_dir = f.out.clone();
    Ok(o)
}

fn report(diags: &Diags, sources: &std::collections::HashMap<PathBuf, String>) {
    let base = std::env::current_dir().unwrap_or_default();
    // Files without their text loaded, such as the config, are read here for display.
    let mut sources = sources.clone();
    for d in &diags.list {
        if let Some(f) = &d.file {
            if !sources.contains_key(f) {
                if let Ok(t) = std::fs::read_to_string(f) {
                    sources.insert(f.clone(), t);
                }
            }
        }
    }
    let text = diags.render(&sources, &base);
    if !text.is_empty() {
        eprint!("{text}");
    }
    let (e, w) = (diags.error_count(), diags.warning_count());
    if e + w > 0 {
        eprintln!("{} error{}, {} warning{}", e, if e == 1 { "" } else { "s" }, w, if w == 1 { "" } else { "s" });
    }
}

fn emul_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" && name != "tethers" {
                emul_files(&p, out);
            }
        } else if p.extension().is_some_and(|e| e == "emul") {
            out.push(p);
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { config } => {
            let mut diags = Diags::new();
            let cfg = build::load_config(config.as_deref(), &mut diags);
            let proj = build::check(&cfg, None);
            diags.extend(proj.diags);
            report(&diags, &proj.sources);
            if diags.has_errors() { ExitCode::from(1) } else { ExitCode::SUCCESS }
        }
        Command::Build { compositions, flags } => {
            let ov = match overrides(&flags) {
                Ok(o) => o,
                Err(e) => return usage(&e),
            };
            let mut diags = Diags::new();
            let mut cfg = build::load_config(flags.config.as_deref(), &mut diags);
            ov.apply(&mut cfg);
            let only = if compositions.is_empty() { None } else { Some(compositions.as_slice()) };
            let result = build::render(&cfg, &ov, only, diags);
            report(&result.diags, &result.sources);
            if result.diags.list.iter().any(|d| d.code == "unknown-composition") {
                return ExitCode::from(2);
            }
            if result.diags.has_errors() {
                return ExitCode::from(1);
            }
            match build::write(&result, &cfg.out_dir) {
                Ok(files) => {
                    let base = std::env::current_dir().unwrap_or_default();
                    for f in files {
                        eprintln!("wrote {}", emul::diag::display_path(&f, &base));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: can't write the output: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Command::Format { paths, check, config } => {
            let mut diags = Diags::new();
            let cfg = build::load_config(config.as_deref(), &mut diags);
            let mut files = Vec::new();
            let roots = if paths.is_empty() { vec![cfg.root.clone()] } else { paths };
            for p in roots {
                if p.is_dir() {
                    emul_files(&p, &mut files);
                } else {
                    files.push(p);
                }
            }
            let mut sources = std::collections::HashMap::new();
            let mut unformatted = 0;
            for f in files {
                let src = match std::fs::read_to_string(&f) {
                    Ok(s) => s,
                    Err(e) => {
                        diags.error("missing-file", Some(&f), None, format!("can't read the file: {e}"));
                        continue;
                    }
                };
                match emul::format::format_source(&f, &src) {
                    Ok(out) => {
                        if out != src {
                            unformatted += 1;
                            if check {
                                eprintln!("not formatted: {}", f.display());
                            } else if let Err(e) = std::fs::write(&f, out) {
                                diags.error("missing-file", Some(&f), None, format!("can't write the file: {e}"));
                            } else {
                                eprintln!("formatted {}", f.display());
                            }
                        }
                    }
                    Err(d) => diags.extend(d),
                }
                sources.insert(f, src);
            }
            report(&diags, &sources);
            if diags.has_errors() || (check && unformatted > 0) { ExitCode::from(1) } else { ExitCode::SUCCESS }
        }
        Command::Preview { flags, port, html } => {
            let ov = match overrides(&flags) {
                Ok(o) => o,
                Err(e) => return usage(&e),
            };
            match emul::preview::run(emul::preview::PreviewOptions { config: flags.config.clone(), overrides: ov, port, open_html: html }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}
