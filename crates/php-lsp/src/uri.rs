//! Mapping between LSP URIs, file system paths and workspace keys.
//!
//! Workspace keys are decoded `file://` URIs (`file:///home/a b/x.php`), so
//! that the same file always gets the same key no matter how the client
//! percent-encodes it.

use std::path::{Path, PathBuf};

/// Percent-decodes a URI string.
pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) =
                std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a path for use in a `file://` URI.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Workspace key for a client URI.
pub fn key_from_uri(uri: &str) -> String {
    match uri.strip_prefix("file://") {
        Some(rest) => {
            let mut path = decode(rest);
            // normalize Windows drive letters: /C:/ -> /c:/
            let b = path.as_bytes();
            if b.len() > 2 && b[0] == b'/' && b[2] == b':' && b[1].is_ascii_alphabetic() {
                let lower = b[1].to_ascii_lowercase() as char;
                path.replace_range(1..2, &lower.to_string());
            }
            format!("file://{path}")
        }
        None => uri.to_string(),
    }
}

/// Workspace key for a file system path.
pub fn key_from_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        // Windows: C:/x -> file:///c:/x
        let mut s = s;
        if s.len() > 1 && s.as_bytes()[1] == b':' {
            let lower = s[..1].to_ascii_lowercase();
            s.replace_range(..1, &lower);
        }
        format!("file:///{s}")
    }
}

/// URI string for a workspace key.
pub fn uri_from_key(key: &str) -> String {
    match key.strip_prefix("file://") {
        Some(path) => format!("file://{}", encode_path(path)),
        None => key.to_string(),
    }
}

/// File system path for a client URI.
pub fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let key = key_from_uri(uri);
    let path = key.strip_prefix("file://")?;
    let b = path.as_bytes();
    if b.len() > 2 && b[0] == b'/' && b[2] == b':' {
        return Some(PathBuf::from(&path[1..]));
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = key_from_uri("file:///home/me/my%20project/a.php");
        assert_eq!(key, "file:///home/me/my project/a.php");
        assert_eq!(uri_from_key(&key), "file:///home/me/my%20project/a.php");
        assert_eq!(key_from_path(Path::new("/home/me/my project/a.php")), key);
        assert_eq!(key_from_uri("file:///C%3A/x/a.php"), "file:///c:/x/a.php");
        assert_eq!(path_from_uri("file:///c%3A/x/a.php").unwrap(), PathBuf::from("c:/x/a.php"));
    }
}
