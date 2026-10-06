//! HuJSON support.
//!
//! Headscale's ACL policy is Human JSON: JSON plus `//`, `#` and `/* */`
//! comments and trailing commas. [`strip`] normalises it into plain JSON while
//! preserving every byte inside string literals; `serde_json` does the rest.

/// Removes comments and trailing commas, yielding strict JSON.
///
/// Returns an error for an unterminated string or block comment. Silently
/// producing invalid JSON would surface as a confusing parse error later.
pub fn strip(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                // Copy the string literal verbatim, honouring escapes.
                let start = index;
                index += 1;
                loop {
                    if index >= bytes.len() {
                        return Err("unterminated string literal".into());
                    }
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                out.push_str(&input[start..index]);
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'/' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'*' => {
                index += 2;
                loop {
                    if index + 1 >= bytes.len() {
                        return Err("unterminated block comment".into());
                    }
                    if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            b'#' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b',' => {
                // Drop the comma when the next significant token is a closing
                // bracket or brace.
                let mut lookahead = index + 1;
                while lookahead < bytes.len() {
                    match bytes[lookahead] {
                        b' ' | b'\t' | b'\r' | b'\n' => lookahead += 1,
                        b']' | b'}' => {
                            index += 1;
                            break;
                        }
                        b'/' if lookahead + 1 < bytes.len()
                            && (bytes[lookahead + 1] == b'/' || bytes[lookahead + 1] == b'*') =>
                        {
                            // A comment follows the comma; skip it and keep
                            // looking for the closing bracket.
                            lookahead = skip_comment(bytes, lookahead)?;
                            continue;
                        }
                        _ => {
                            out.push(',');
                            index += 1;
                            break;
                        }
                    }
                }
                if lookahead >= bytes.len() {
                    // Trailing comma at end of input: harmless, drop it.
                    index += 1;
                }
            }
            byte if byte.is_ascii() => {
                out.push(byte as char);
                index += 1;
            }
            _ => {
                // Copy the whole UTF-8 sequence so non-ASCII content outside
                // string literals survives byte-for-byte.
                let ch = input[index..].chars().next().unwrap_or('\u{fffd}');
                out.push(ch);
                index += ch.len_utf8();
            }
        }
    }

    Ok(out)
}

fn skip_comment(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut index = start;
    if index + 1 >= bytes.len() {
        return Err("unterminated comment".into());
    }
    match bytes[index + 1] {
        b'/' => {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        }
        b'*' => {
            index += 2;
            loop {
                if index + 1 >= bytes.len() {
                    return Err("unterminated block comment".into());
                }
                if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                    index += 2;
                    break;
                }
                index += 1;
            }
        }
        _ => return Err("unterminated comment".into()),
    }
    Ok(index)
}

/// Parses HuJSON into a `serde_json::Value`.
pub fn parse(input: &str) -> Result<serde_json::Value, String> {
    let stripped = strip(input)?;
    serde_json::from_str(&stripped).map_err(|err| err.to_string())
}

/// Whether the text contains any HuJSON-only construct (a comment).
///
/// Structured editors rewrite the policy as plain JSON, which drops comments.
/// The UI warns when saving would discard them.
pub fn has_comments(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
            }
            b'#' => return true,
            b'/' if index + 1 < bytes.len()
                && (bytes[index + 1] == b'/' || bytes[index + 1] == b'*') =>
            {
                return true;
            }
            _ => index += 1,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_line_and_block_comments() {
        let input = r#"{
            // a line comment
            "a": 1, # a hash comment
            /* block */ "b": 2
        }"#;
        let value: serde_json::Value = parse(input).unwrap();
        assert_eq!(value["a"], 1);
        assert_eq!(value["b"], 2);
    }

    #[test]
    fn comment_markers_inside_strings_are_preserved() {
        let input = r##"{"url": "https://example.com//path", "note": "# not a comment"}"##;
        let value: serde_json::Value = parse(input).unwrap();
        assert_eq!(value["url"], "https://example.com//path");
        assert_eq!(value["note"], "# not a comment");
    }

    #[test]
    fn escapes_inside_strings_are_preserved() {
        let input = r#"{"quoted": "a \" // b"}"#;
        let value: serde_json::Value = parse(input).unwrap();
        assert_eq!(value["quoted"], "a \" // b");
    }

    #[test]
    fn drops_trailing_commas() {
        let input = r#"{"a": [1, 2, 3,], "b": 2,}"#;
        let value: serde_json::Value = parse(input).unwrap();
        assert_eq!(value["a"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn trailing_comma_followed_by_comment_is_dropped() {
        let input = r#"{"a": 1, // done
        }"#;
        let value: serde_json::Value = parse(input).unwrap();
        assert_eq!(value["a"], 1);
    }

    #[test]
    fn detects_comment_presence() {
        assert!(has_comments("{\"a\":1} // hi"));
        assert!(has_comments("/* hi */ {}"));
        assert!(!has_comments(r#"{"a": "// not a comment"}"#));
    }

    #[test]
    fn unterminated_constructs_error() {
        assert!(parse("{\"a\": \"unterminated").is_err());
        assert!(parse("/* unterminated").is_err());
    }

    #[test]
    fn plain_json_passes_through() {
        let value: serde_json::Value = parse(r#"{"a":1}"#).unwrap();
        assert_eq!(value["a"], 1);
    }
}
