//! The PNG output: the SVG output, with text in outline form, rasterized.

/// Rasterizes an SVG at `scale`. Returns the PNG file and the RGBA pixels.
pub fn rasterize(svg: &str, scale: f32) -> Result<(Vec<u8>, tiny_skia::Pixmap), String> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opts).map_err(|e| format!("the SVG could not be read: {e}"))?;
    let size = tree.size();
    let w = (size.width() * scale).round().max(1.0) as u32;
    let h = (size.height() * scale).round().max(1.0) as u32;
    if w > 16384 || h > 16384 {
        return Err(format!("the PNG would be {w}×{h} pixels; the limit is 16384 on each side"));
    }
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("the PNG could not be allocated")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let png = pixmap.encode_png().map_err(|e| e.to_string())?;
    Ok((png, pixmap))
}
