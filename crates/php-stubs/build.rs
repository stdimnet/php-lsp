//! Packs `stubs/**/*.php` into one deflate compressed blob.
//!
//! Format (before compression), all integers little endian:
//! `count: u32` followed by `count` records of
//! `ext_len: u16, ext, path_len: u16, path, text_len: u32, text`.

use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e == "php") {
            out.push(p);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("stubs");
    println!("cargo:rerun-if-changed=stubs");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut raw: Vec<u8> = Vec::new();
    raw.extend((files.len() as u32).to_le_bytes());
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let ext = rel.split('/').next().unwrap().to_string();
        let text = std::fs::read(f).unwrap();
        raw.extend((ext.len() as u16).to_le_bytes());
        raw.extend(ext.as_bytes());
        raw.extend((rel.len() as u16).to_le_bytes());
        raw.extend(rel.as_bytes());
        raw.extend((text.len() as u32).to_le_bytes());
        raw.extend(&text);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec(&raw, 8);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("stubs.bin");
    std::fs::write(out, compressed).unwrap();
}
