//! Escapes, as Piton specifies. Backslashes wrap special characters, with a space on
//! each side, as in `\ { \`. You can stack backslashes. The wrapper at the end must
//! be the same as the wrapper at the start.

/// If an escape wrapper opens at byte `i` of `s`, returns `(end, content)`. `end` is
/// the byte just after the wrapper that closes the escape. Returns `Err(())` when a
/// wrapper opens but does not close. Returns `Ok(None)` when `i` does not start a wrapper.
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
    // The content can be empty only in the special form `\  \`. The search starts at
    // the space that opens the content. Thus `\ \` is not an empty wrapper.
    if search > 0 {
        search -= 1;
    }
    while let Some(rel) = s[search..].find(&closing) {
        let at = search + rel;
        let end = at + closing.len();
        // The run at the end must have exactly n backslashes.
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

/// Removes all escape wrappers from `s`. If a wrapper does not close, returns the byte
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
