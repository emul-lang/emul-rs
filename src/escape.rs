//! Escapes, following Piton: special characters are wrapped in backslashes with a
//! space on each side, as in `\ { \`. Backslashes may be stacked, and the closing
//! wrapper must match the opening one.

/// If an escape wrapper opens at byte `i` of `s`, returns `(end, content)`, where `end`
/// is the byte just past the closing wrapper. Returns `Err(())` when a wrapper opens
/// but never closes, and `Ok(None)` when `i` does not start a wrapper.
pub fn scan_wrapper(s: &str, i: usize) -> Result<Option<(usize, String)>, ()> {
    let bytes = s.as_bytes();
    if bytes.get(i) != Some(&b'\\') {
        return Ok(None);
    }
    let mut n = 0;
    while bytes.get(i + n) == Some(&b'\\') {
        n += 1;
    }
    if bytes.get(i + n) != Some(&b' ') {
        return Ok(None);
    }
    let content_start = i + n + 1;
    let closing: String = format!(" {}", "\\".repeat(n));
    let mut search = content_start;
    // The content may be empty only in the degenerate form `\  \`; search from the
    // space that opens the content so `\ \` is not read as a wrapper of nothing.
    if search > 0 {
        search -= 1;
    }
    while let Some(rel) = s[search..].find(&closing) {
        let at = search + rel;
        let end = at + closing.len();
        // The closing run must be exactly n backslashes long.
        if bytes.get(end) == Some(&b'\\') {
            search = at + 1;
            continue;
        }
        if at < content_start {
            search = at + 1;
            continue;
        }
        return Ok(Some((end, s[content_start..at].to_string())));
    }
    Err(())
}

/// Removes every escape wrapper from `s`. On an unclosed wrapper, returns the byte
/// offset where it opens.
pub fn unescape(s: &str) -> Result<String, usize> {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match scan_wrapper(s, i) {
            Ok(Some((end, content))) => {
                out.push_str(&content);
                i = end;
            }
            Ok(None) => {
                let ch = s[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
            Err(()) => return Err(i),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(unescape(r"\ { \").unwrap(), "{");
        assert_eq!(unescape(r"a\ , \ b").unwrap(), "a, b");
        assert_eq!(unescape(r#"She said \ " \ hi"#).unwrap(), "She said \" hi");
        assert_eq!(unescape(r"\\ \ x \ \\").unwrap(), r"\ x \");
        assert_eq!(unescape(r"plain \n text").unwrap(), r"plain \n text");
        assert!(unescape(r"\ never closed").is_err());
    }
}
