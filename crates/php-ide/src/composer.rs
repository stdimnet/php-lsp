//! Composer awareness: PSR-4 autoload mappings from `composer.json`.

use std::path::{Path, PathBuf};

/// Directory and namespace prefix pairs from `autoload(-dev).psr-4`.
pub fn psr4_mappings(composer_json: &Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read_to_string(composer_json) else { return Vec::new() };
    let root = composer_json.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        let Some(map) = json_object_after(&text, section).and_then(|s| json_object_after(s, "psr-4")) else {
            continue;
        };
        for (prefix, dirs) in parse_simple_object(map) {
            for d in dirs {
                out.push((prefix.trim_end_matches('\\').to_string(), root.join(d.trim_end_matches('/'))));
            }
        }
    }
    // longest directory first
    out.sort_by_key(|(_, d)| std::cmp::Reverse(d.as_os_str().len()));
    out
}

/// The namespace a class file at `key` (a `file://` URI key) should declare
/// according to the nearest `composer.json`.
pub fn expected_namespace(key: &str) -> Option<String> {
    let path = PathBuf::from(key.strip_prefix("file://")?);
    let dir = path.parent()?;
    let mut cur = Some(dir);
    while let Some(d) = cur {
        let cj = d.join("composer.json");
        if cj.exists() {
            for (prefix, base) in psr4_mappings(&cj) {
                if let Ok(rel) = dir.strip_prefix(&base) {
                    let mut ns = prefix.clone();
                    for c in rel.components() {
                        let s = c.as_os_str().to_string_lossy();
                        if !ns.is_empty() {
                            ns.push('\\');
                        }
                        ns.push_str(&s);
                    }
                    return Some(ns);
                }
            }
            return None;
        }
        cur = d.parent();
    }
    None
}

/// Returns the text of the JSON object value of `"key": {...}` (very small
/// JSON reader, enough for composer.json).
fn json_object_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut search = 0;
    while let Some(i) = text[search..].find(&needle) {
        let after = &text[search + i + needle.len()..];
        let after_trim = after.trim_start();
        if let Some(rest) = after_trim.strip_prefix(':') {
            let rest = rest.trim_start();
            if rest.starts_with('{') {
                let mut depth = 0;
                let mut in_str = false;
                let mut esc = false;
                for (j, c) in rest.char_indices() {
                    if in_str {
                        if esc {
                            esc = false;
                        } else if c == '\\' {
                            esc = true;
                        } else if c == '"' {
                            in_str = false;
                        }
                        continue;
                    }
                    match c {
                        '"' => in_str = true,
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(&rest[..=j]);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        search += i + needle.len();
    }
    None
}

/// Parses `{"A\\": "src/", "B\\": ["a/", "b/"]}`.
fn parse_simple_object(obj: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut chars = obj.char_indices().peekable();
    let mut tokens: Vec<(char, String)> = Vec::new();
    while let Some((_, c)) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some((_, c)) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some((_, n)) = chars.next() {
                                s.push(n);
                            }
                        }
                        '"' => break,
                        c => s.push(c),
                    }
                }
                tokens.push(('s', s));
            }
            ':' | ',' | '[' | ']' => tokens.push((c, String::new())),
            _ => {}
        }
    }
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].0 == 's' && tokens.get(i + 1).is_some_and(|t| t.0 == ':') {
            let key = tokens[i].1.clone();
            let mut vals = Vec::new();
            let mut j = i + 2;
            if tokens.get(j).is_some_and(|t| t.0 == '[') {
                j += 1;
                while j < tokens.len() && tokens[j].0 != ']' {
                    if tokens[j].0 == 's' {
                        vals.push(tokens[j].1.clone());
                    }
                    j += 1;
                }
            } else if tokens.get(j).is_some_and(|t| t.0 == 's') {
                vals.push(tokens[j].1.clone());
            }
            out.push((key, vals));
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn psr4() {
        let dir = std::env::temp_dir().join(format!("php-lsp-composer-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src/Http/Controllers")).unwrap();
        std::fs::write(dir.join("composer.json"), r#"{"name": "x/y", "autoload": {"psr-4": {"App\\": "src/"}}, "autoload-dev": {"psr-4": {"Tests\\": ["tests/"]}}}"#).unwrap();
        let key = format!("file://{}", dir.join("src/Http/Controllers/HomeController.php").display());
        assert_eq!(super::expected_namespace(&key).as_deref(), Some("App\\Http\\Controllers"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
