//! Comment-preserving YAML editing.
//!
//! A round-trip YAML library would drop comments, key order, and formatting.
//! This module finds a dotted key path in the original text and replaces only
//! that value's lines. It handles block-style mappings and sequences; nested
//! flow-style collections inside a patched path are rejected.

use std::cell::OnceCell;

use anyhow::{Result, bail};
use serde_json::Value;

/// A dotted path segment. Segments match with or without quotes, so
/// `split."example.com"` and `split.example.com` both work.
pub type Path = Vec<String>;

/// A YAML document that can be patched while preserving everything else.
#[derive(Debug, Clone)]
pub struct YamlEditor {
    text: String,
    indent_width: usize,
    /// Parsed document, rebuilt after a mutation. Readers parse once.
    parsed: OnceCell<Value>,
}

/// Where a key's value lives in the line array.
#[derive(Debug, Clone, Copy)]
struct ValueSpan {
    key_line: usize,
    /// First line of the value block (may equal `key_line` for inline values).
    first: usize,
    /// One past the last line of the value block.
    end: usize,
}

impl YamlEditor {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let indent_width = detect_indent_width(&text);
        Self {
            text,
            indent_width,
            parsed: OnceCell::new(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
    fn lines(&self) -> Vec<String> {
        self.text.split('\n').map(str::to_string).collect()
    }

    fn join(&mut self, lines: Vec<String>) {
        self.text = lines.join("\n");
        self.parsed = OnceCell::new();
    }

    /// Reads a scalar as a string, if the path resolves to one.
    pub fn get_str(&self, path: &Path) -> Option<String> {
        match self.get_value(path)? {
            Value::String(s) => Some(s),
            Value::Bool(b) => Some(b.to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// Reads a boolean, accepting Go-style `"true"`/`"false"` strings.
    pub fn get_bool(&self, path: &Path) -> Option<bool> {
        match self.get_value(path)? {
            Value::Bool(b) => Some(b),
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn get_string_list(&self, path: &Path) -> Vec<String> {
        match self.get_value(path) {
            Some(Value::Array(items)) => items
                .into_iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s),
                    Value::Bool(b) => Some(b.to_string()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Resolves a dotted path through the parsed document.
    pub fn get_value(&self, path: &Path) -> Option<Value> {
        let document = self
            .parsed
            .get_or_init(|| serde_yaml_ng::from_str::<Value>(&self.text).unwrap_or(Value::Null));
        let mut current = document.clone();
        for segment in path {
            current = match current {
                Value::Object(map) => map.get(segment).cloned()?,
                _ => return None,
            };
        }
        Some(current)
    }

    /// Sets a value at `path`, creating intermediate mappings as needed.
    pub fn set(&mut self, path: &Path, value: Value) -> Result<()> {
        if path.is_empty() {
            bail!("cannot patch the document root");
        }
        self.patch(path, Some(value))
    }

    /// Removes the key at `path`. Missing keys are a no-op.
    pub fn remove(&mut self, path: &Path) -> Result<()> {
        if path.is_empty() {
            bail!("cannot patch the document root");
        }
        self.patch(path, None)
    }

    fn patch(&mut self, path: &Path, value: Option<Value>) -> Result<()> {
        let mut lines = self.lines();

        // Descends the ancestors, recording each block's span.
        let mut parent: Option<ValueSpan> = None;
        let mut child_indent = 0usize;
        let mut missing: Option<usize> = None;

        for (index, segment) in path[..path.len() - 1].iter().enumerate() {
            let (start, end) = match parent {
                Some(span) => (span.first, span.end),
                None => (0, lines.len()),
            };

            match find_key(&lines, start, end, child_indent, segment) {
                Some(found) => {
                    child_indent = child_indent_of(&lines, &found, self.indent_width);
                    parent = Some(found);
                }
                None => {
                    missing = Some(index);
                    break;
                }
            }
        }

        // The leaf sits one level below its parent. Each missing segment in the
        // chain adds another level.
        let leaf_indent = match missing {
            Some(at) => child_indent + (path.len() - 1 - at) * self.indent_width,
            None => child_indent,
        };
        let rendered = match value.as_ref() {
            Some(value) => Some(render_value(value, leaf_indent, self.indent_width)?),
            None => None,
        };

        // An ancestor is absent: build the rest of the chain inside the deepest
        // block that exists. Writing the whole path from the root would
        // duplicate the ancestors that are already there.
        if let Some(missing_at) = missing {
            let Some(rendered) = rendered else {
                // Nothing to remove, and nothing to add.
                return Ok(());
            };
            insert_chain(
                &mut lines,
                parent,
                child_indent,
                &path[missing_at..],
                &rendered,
                self.indent_width,
            );
            self.join(lines);
            return Ok(());
        }

        // Every ancestor exists, so the leaf belongs in `parent`'s block.
        let (block_start, block_end) = match parent {
            Some(span) => (span.first, span.end),
            None => (0, lines.len()),
        };
        let leaf = path.last().expect("paths are never empty");

        if let Some(found) = find_key(&lines, block_start, block_end, leaf_indent, leaf) {
            match rendered {
                Some(rendered) => {
                    // `splice` replaces the removed range in place, so only the
                    // new lines belong in the replacement iterator.
                    lines.splice(
                        found.key_line..found.end,
                        [format!(
                            "{}{}:{}",
                            " ".repeat(leaf_indent),
                            render_scalar(leaf),
                            rendered
                        )],
                    );
                }
                None => {
                    lines.drain(found.key_line..found.end);
                }
            }
            self.join(lines);
            return Ok(());
        }

        let Some(rendered) = rendered else {
            // Removing a key that is not there is a no-op.
            return Ok(());
        };

        let insertion = format!(
            "{}{}:{}",
            " ".repeat(leaf_indent),
            render_scalar(leaf),
            rendered
        );
        match parent {
            Some(span) => {
                // Append after the parent's last child.
                let at = trim_trailing_blank(&lines, span.first, span.end);
                lines.insert(at, insertion);
            }
            None => {
                // A top-level key: append at the end of the document.
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(insertion);
            }
        }

        self.join(lines);
        Ok(())
    }
}

/// Parses a dotted path, honouring quoted segments.
///
/// `dns.nameservers.split."example.com"` yields four segments with the quotes
/// removed, so a domain containing dots stays a single key.
pub fn parse_path(input: &str) -> Path {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\'' => in_quotes = !in_quotes,
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '.' if !in_quotes => {
                segments.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    segments.push(current);
    segments
}

/// Renders a JSON value as the tail of a `key:` line at `base_indent`, plus any
/// nested lines the value needs.
fn render_value(value: &Value, base_indent: usize, indent_width: usize) -> Result<String> {
    let child_indent = base_indent + indent_width;

    Ok(match value {
        Value::Null => " null".into(),
        Value::Bool(b) => format!(" {b}"),
        Value::Number(n) => format!(" {n}"),
        Value::String(s) => format!(" {}", render_scalar(s)),
        Value::Array(items) if items.is_empty() => " []".into(),
        Value::Array(items) => {
            let mut out = String::new();
            for item in items {
                match item {
                    Value::Object(map) => {
                        // `- key: value`, with continuation keys aligned under
                        // the first one.
                        let mut first = true;
                        for (key, nested) in map {
                            let rendered =
                                render_value(nested, child_indent + indent_width, indent_width)?;
                            if first {
                                out.push_str(&format!(
                                    "\n{}- {}:{}",
                                    " ".repeat(child_indent),
                                    key,
                                    rendered
                                ));
                                first = false;
                            } else {
                                out.push_str(&format!(
                                    "\n{}{}:{}",
                                    " ".repeat(child_indent + indent_width),
                                    key,
                                    rendered
                                ));
                            }
                        }
                    }
                    other => {
                        out.push_str(&format!(
                            "\n{}- {}",
                            " ".repeat(child_indent),
                            render_value(other, child_indent, indent_width)?.trim_start()
                        ));
                    }
                }
            }
            out
        }
        Value::Object(map) if map.is_empty() => " {}".into(),
        Value::Object(map) => {
            let mut out = String::new();
            for (key, nested) in map {
                out.push_str(&format!(
                    "\n{}{}:{}",
                    " ".repeat(child_indent),
                    key,
                    render_value(nested, child_indent, indent_width)?
                ));
            }
            out
        }
    })
}

/// Tests whether a character ends the emitted line, or is one a YAML reader
/// folds as if it had. A raw one inside a scalar would let a value add keys of
/// its own.
fn breaks_line(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}

/// Quotes a scalar only when YAML would otherwise reinterpret it.
fn render_scalar(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value != value.trim()
        || value.contains(": ")
        || value.contains('#')
        || value.chars().any(breaks_line)
        || value.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
            '`',
        ])
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
        )
        || value.parse::<f64>().is_ok();

    if needs_quoting {
        format!("\"{}\"", escape_quoted(value))
    } else {
        value.to_string()
    }
}

/// Escapes the body of a double-quoted scalar. Line breaks and control
/// characters become escapes, so they cannot end the line the scalar is on.
fn escape_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if breaks_line(c) => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Returns the number of leading spaces.
fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_skippable(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// Returns the key a line assigns, if it is a mapping entry.
fn line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('-') {
        return None;
    }
    let (key, _) = trimmed.split_once(':')?;
    let key = key.trim().trim_matches('"').trim_matches('\'');
    if key.is_empty() { None } else { Some(key) }
}

/// Finds the assignment of `key` at exactly `indent` within `[start, end)`.
fn find_key(
    lines: &[String],
    start: usize,
    end: usize,
    indent: usize,
    key: &str,
) -> Option<ValueSpan> {
    let mut index = start;
    while index < end && index < lines.len() {
        let line = &lines[index];

        if is_skippable(line) {
            index += 1;
            continue;
        }

        let current_indent = line_indent(line);
        if current_indent < indent {
            // Left the block.
            return None;
        }

        if current_indent == indent && line_key(line) == Some(key) {
            let value_end = block_end(lines, index, indent);
            return Some(ValueSpan {
                key_line: index,
                first: index + 1,
                end: value_end,
            });
        }

        index += 1;
    }
    None
}

/// Returns the end index of the block that starts after `key_line`.
fn block_end(lines: &[String], key_line: usize, indent: usize) -> usize {
    let mut index = key_line + 1;
    let mut last_content = key_line + 1;

    while index < lines.len() {
        let line = &lines[index];
        if is_skippable(line) {
            index += 1;
            continue;
        }
        if line_indent(line) <= indent {
            break;
        }
        last_content = index + 1;
        index += 1;
    }

    last_content
}

fn trim_trailing_blank(lines: &[String], start: usize, end: usize) -> usize {
    let mut at = end;
    while at > start && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    at
}

fn detect_indent_width(text: &str) -> usize {
    for line in text.lines() {
        let indent = line_indent(line);
        if indent > 0 && !is_skippable(line) {
            return indent;
        }
    }
    2
}

/// Returns the indentation used by a mapping's children. A mapping with no
/// children yet gets its own indentation plus one level.
fn child_indent_of(lines: &[String], span: &ValueSpan, indent_width: usize) -> usize {
    if span.first < span.end {
        line_indent(&lines[span.first])
    } else {
        line_indent(&lines[span.key_line]) + indent_width
    }
}

/// Inserts the missing tail of a path inside the deepest block that exists.
///
/// `path` starts at the first segment the document does not already have, and
/// `indent` is the indentation its first segment must use.
fn insert_chain(
    lines: &mut Vec<String>,
    parent: Option<ValueSpan>,
    indent: usize,
    path: &[String],
    rendered: &str,
    indent_width: usize,
) {
    let mut block: Vec<String> = Vec::new();
    for (depth, segment) in path[..path.len() - 1].iter().enumerate() {
        block.push(format!(
            "{}{}:",
            " ".repeat(indent + depth * indent_width),
            render_scalar(segment)
        ));
    }
    block.push(format!(
        "{}{}:{}",
        " ".repeat(indent + (path.len() - 1) * indent_width),
        render_scalar(path.last().expect("paths are never empty")),
        rendered
    ));

    match parent {
        Some(span) => {
            // Inside the existing block, after its last child.
            let at = trim_trailing_blank(lines, span.first, span.end);
            for (offset, line) in block.into_iter().enumerate() {
                lines.insert(at + offset, line);
            }
        }
        None => {
            // A new top-level block, separated by a blank line.
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.extend(block);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CONFIG: &str = r#"# Headscale configuration
server_url: https://headscale.example.com
listen_addr: 0.0.0.0:8080

# DNS settings
dns:
  magic_dns: true
  base_domain: headscale.example.com
  override_local_dns: false
  nameservers:
    global:
      - 1.1.1.1
    split:
      old.example.com:
        - 9.9.9.9
  search_domains:
    - example.com
  extra_records:
    - name: git.example.com
      type: A
      value: 100.64.0.5

policy:
  mode: database
"#;

    /// A raw line break in a value would let a request body add its own keys
    /// to the Headscale config.
    #[test]
    fn a_line_break_in_a_value_stays_inside_its_scalar() {
        let mut editor = YamlEditor::new("dns:\n  base_domain: example.com\n");
        editor
            .patch(
                &parse_path("dns.search_domains"),
                Some(json!(["ok.example.com", "x\rserver_url:\r  https://evil.example"])),
            )
            .unwrap();

        let text = editor.as_str();
        assert!(!text.contains('\r'), "{text}");
        assert!(
            text.lines()
                .all(|line| !line.trim_start().starts_with("server_url")),
            "{text}"
        );

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).unwrap();
        assert_eq!(
            parsed["dns"]["search_domains"][1].as_str(),
            Some("x\rserver_url:\r  https://evil.example")
        );
    }

    /// The same trick through a path segment, which becomes a key.
    #[test]
    fn a_line_break_in_a_key_is_quoted() {
        let mut editor = YamlEditor::new("dns:\n  base_domain: example.com\n");
        editor
            .patch(
                &parse_path("dns.nameservers.split.\"a\nb\""),
                Some(json!(["1.1.1.1"])),
            )
            .unwrap();

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(editor.as_str()).unwrap();
        assert_eq!(parsed["dns"]["base_domain"].as_str(), Some("example.com"));
        assert_eq!(
            parsed["dns"]["nameservers"]["split"]["a\nb"][0].as_str(),
            Some("1.1.1.1")
        );
    }

    #[test]
    fn reads_nested_values() {
        let editor = YamlEditor::new(CONFIG);
        assert_eq!(
            editor.get_str(&parse_path("dns.base_domain")).as_deref(),
            Some("headscale.example.com")
        );
        assert_eq!(editor.get_bool(&parse_path("dns.magic_dns")), Some(true));
        assert_eq!(
            editor.get_string_list(&parse_path("dns.nameservers.global")),
            vec!["1.1.1.1"]
        );
        assert_eq!(
            editor.get_string_list(&parse_path("dns.search_domains")),
            vec!["example.com"]
        );
    }

    #[test]
    fn replaces_a_scalar_and_keeps_comments() {
        let mut editor = YamlEditor::new(CONFIG);
        editor
            .set(&parse_path("dns.base_domain"), Value::String("ts.example.net".into()))
            .unwrap();

        let text = editor.as_str();
        assert!(text.contains("# Headscale configuration"));
        assert!(text.contains("# DNS settings"));
        assert!(text.contains("base_domain: ts.example.net"));
        assert!(!text.contains("base_domain: headscale.example.com"));
        // Sibling keys are untouched.
        assert!(text.contains("magic_dns: true"));
        assert!(text.contains("policy:\n  mode: database\n"));
    }

    #[test]
    fn replaces_a_boolean() {
        let mut editor = YamlEditor::new(CONFIG);
        editor.set(&parse_path("dns.magic_dns"), Value::Bool(false)).unwrap();
        assert!(editor.as_str().contains("magic_dns: false"));
    }

    #[test]
    fn replaces_a_sequence() {
        let mut editor = YamlEditor::new(CONFIG);
        editor
            .set(
                &parse_path("dns.search_domains"),
                serde_json::json!(["a.example", "b.example"]),
            )
            .unwrap();

        let text = editor.as_str();
        assert!(text.contains("search_domains:\n    - a.example\n    - b.example"));
        // The following block survived.
        assert!(text.contains("extra_records:"));
        assert!(text.contains("policy:"));
    }

    #[test]
    fn adds_a_new_split_nameserver() {
        let mut editor = YamlEditor::new(CONFIG);
        editor
            .set(
                &parse_path("dns.nameservers.split.\"new.example.com\""),
                serde_json::json!(["8.8.8.8"]),
            )
            .unwrap();

        let text = editor.as_str();
        assert!(text.contains("new.example.com:"));
        assert!(text.contains("- 8.8.8.8"));
        // Existing entries survive.
        assert!(text.contains("old.example.com:"));
        assert!(text.contains("- 9.9.9.9"));
    }

    #[test]
    fn removes_a_key() {
        let mut editor = YamlEditor::new(CONFIG);
        editor
            .remove(&parse_path("dns.nameservers.split.\"old.example.com\""))
            .unwrap();

        let text = editor.as_str();
        assert!(!text.contains("old.example.com"));
        assert!(text.contains("global:"));
        assert!(text.contains("search_domains:"));
    }

    #[test]
    fn creating_a_missing_parent_chain_appends_a_block() {
        let mut editor = YamlEditor::new("dns:\n  magic_dns: true\n");
        editor
            .set(&parse_path("oidc.allowed_domains"), serde_json::json!(["corp.example"]))
            .unwrap();

        let text = editor.as_str();
        assert!(text.contains("oidc:\n  allowed_domains:\n    - corp.example"));
        // The original content is still intact.
        assert!(text.contains("dns:\n  magic_dns: true"));
    }

    /// The split-DNS case: `dns` and `nameservers` exist, `split` does not.
    /// Creating the whole chain from the root duplicated them and produced a
    /// document Headscale refused to load.
    #[test]
    fn creates_a_missing_intermediate_key_inside_its_parent() {
        let mut editor = YamlEditor::new(
            "dns:\n  magic_dns: true\n  nameservers:\n    global:\n      - 1.1.1.1\npolicy:\n  mode: database\n",
        );
        editor
            .set(
                &parse_path("dns.nameservers.split.\"corp.example\""),
                serde_json::json!(["10.0.0.53"]),
            )
            .unwrap();

        let text = editor.as_str();

        // Exactly one `dns:` and one `nameservers:` key. Match whole keys:
        // `magic_dns:` and `override_local_dns:` also contain "dns:".
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().split_once(':').map(|(key, _)| key))
            .collect();
        assert_eq!(keys.iter().filter(|key| **key == "dns").count(), 1, "{text}");
        assert_eq!(
            keys.iter().filter(|key| **key == "nameservers").count(),
            1,
            "{text}"
        );

        // `split` landed inside `nameservers`, indented one level deeper.
        assert!(
            text.contains("    split:\n      corp.example:\n        - 10.0.0.53"),
            "{text}"
        );

        // The rest of the document survived, in order.
        assert!(text.contains("  nameservers:\n    global:\n      - 1.1.1.1\n    split:"));
        assert!(text.contains("policy:\n  mode: database"));

        // The result must still parse as YAML.
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text)
            .expect("the patched document must stay valid YAML");
    }

    #[test]
    fn adds_a_second_split_entry_beside_the_first() {
        let mut editor = YamlEditor::new(CONFIG);
        editor
            .set(
                &parse_path("dns.nameservers.split.\"corp.example\""),
                serde_json::json!(["10.0.0.53"]),
            )
            .unwrap();
        editor
            .set(
                &parse_path("dns.nameservers.split.\"other.example\""),
                serde_json::json!(["10.0.0.54"]),
            )
            .unwrap();

        let text = editor.as_str();
        assert!(text.contains("corp.example:"));
        assert!(text.contains("other.example:"));
        assert_eq!(text.matches("nameservers:").count(), 1, "{text}");
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text).unwrap();
    }

    #[test]
    fn quoted_paths_keep_dots_in_a_single_segment() {
        let path = parse_path("dns.nameservers.split.\"example.com\"");
        assert_eq!(path.len(), 4);
        assert_eq!(path[3], "example.com");
    }

    #[test]
    fn removing_a_missing_key_is_a_noop() {
        let mut editor = YamlEditor::new(CONFIG);
        let before = editor.as_str().to_string();
        editor.remove(&parse_path("dns.does_not_exist")).unwrap();
        assert_eq!(editor.as_str(), before);
    }

    #[test]
    fn scalars_are_quoted_only_when_needed() {
        assert_eq!(render_scalar("ts.example.net"), "ts.example.net");
        assert_eq!(render_scalar("true"), "\"true\"");
        assert_eq!(render_scalar("8080"), "\"8080\"");
        assert_eq!(render_scalar(""), "\"\"");
    }

    #[test]
    fn reads_a_document_with_a_leading_indented_list() {
        let editor = YamlEditor::new("a:\n    - one\n    - two\nb: 2\n");
        assert_eq!(editor.get_string_list(&parse_path("a")), vec!["one", "two"]);
        assert_eq!(editor.get_str(&parse_path("b")).as_deref(), Some("2"));
    }
}


