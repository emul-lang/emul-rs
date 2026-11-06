//! Emul: a small design language that compiles compositions of styled components to
//! HTML, SVG and PNG.

pub mod ast;
pub mod build;
pub mod config;
pub mod diag;
pub mod escape;
pub mod fonts;
pub mod format;
pub mod html;
pub mod layout;
pub mod parser;
pub mod png;
pub mod preview;
pub mod project;
pub mod props;
pub mod svg;
pub mod text;
pub mod values;
