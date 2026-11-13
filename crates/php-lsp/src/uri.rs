//! Mapping between LSP URIs, file system paths and workspace keys.
//!
//! Workspace keys are decoded `file://` URIs (`file:///home/a b/x.php`), so
//! that the same file always gets the same key no matter how the client
//! percent-encodes it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

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
    let mut s = path.to_string_lossy().replace('\\', "/");
    // Windows verbatim paths: //?/C:/x -> C:/x, //?/UNC/server/share -> //server/share
    if let Some(rest) = s.strip_prefix("//?/UNC/") {
        s = format!("//{rest}");
    } else if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    // UNC paths: //server/share/x -> file://server/share/x
    if let Some(rest) = s.strip_prefix("//") {
        return format!("file://{rest}");
    }
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

/// How the client spells Windows drive letters (`file:///C:/`,
/// `file:///c%3A/`, ...), learned from the URIs it sends, so that answers
/// use the same spelling: 0 = not known yet, else 1 + bit 0: upper case,
/// bit 1: plain colon. Process-wide (one client per process).
static DRIVE_STYLE: AtomicU8 = AtomicU8::new(0);

/// Remembers the drive letter spelling of a client URI (the first one
/// with a drive letter wins).
pub fn learn_style(uri: &str) {
    let Some(rest) = uri.strip_prefix("file:///") else { return };
    let b = rest.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return;
    }
    let plain = rest[1..].starts_with(':');
    let encoded = rest[1..].len() >= 3 && rest[1..4].eq_ignore_ascii_case("%3a");
    if !plain && !encoded {
        return;
    }
    let style = 1 + u8::from(b[0].is_ascii_uppercase()) + 2 * u8::from(plain);
    let _ = DRIVE_STYLE.compare_exchange(0, style, Ordering::Relaxed, Ordering::Relaxed);
}

/// URI string for a workspace key (drive letters as the client spells
/// them, see [`learn_style`]).
pub fn uri_from_key(key: &str) -> String {
    match key.strip_prefix("file://") {
        Some(path) => {
            let mut uri = format!("file://{}", encode_path(path));
            let style = DRIVE_STYLE.load(Ordering::Relaxed);
            let b = uri.as_bytes();
            let drive = b.len() > 11 && b[8].is_ascii_alphabetic() && uri[9..].starts_with("%3A");
            if style > 0 && drive {
                let style = style - 1;
                let letter =
                    if style & 1 != 0 { b[8].to_ascii_uppercase() } else { b[8].to_ascii_lowercase() };
                let colon = if style & 2 != 0 { ":" } else { "%3A" };
                uri = format!("file:///{}{colon}{}", letter as char, &uri[12..]);
            }
            uri
        }
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

    #[test]
    fn windows_paths() {
        // verbatim and UNC paths (issue #9)
        assert_eq!(key_from_path(Path::new(r"\\?\C:\work\a.php")), "file:///c:/work/a.php");
        assert_eq!(key_from_path(Path::new(r"C:\work\a.php")), "file:///c:/work/a.php");
        assert_eq!(key_from_path(Path::new(r"\\?\UNC\srv\share\a.php")), "file://srv/share/a.php");
        assert_eq!(key_from_path(Path::new(r"\\srv\share\a.php")), "file://srv/share/a.php");
        // answers use the client's spelling of drive letters
        learn_style("file:///C:/work/shop");
        learn_style("file:///d%3A/other");
        assert_eq!(uri_from_key("file:///c:/work/shop/src/User.php"), "file:///C:/work/shop/src/User.php");
        assert_eq!(uri_from_key("file:///home/a b.php"), "file:///home/a%20b.php");
    }
}
