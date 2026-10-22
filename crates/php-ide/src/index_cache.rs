//! On-disk cache of the symbols extracted from files, so that a restart only
//! parses files whose content changed.
//!
//! Entries are keyed by path and a hash of the file's text (the text is read
//! anyway). A cache written by another php-lsp version or for another PHP
//! version is ignored, as is a damaged file.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use php_hir::{ExtractOptions, FileSymbols};
use php_index::FileKind;
use serde::{Deserialize, Serialize};

/// Bumped whenever the cached data changes shape.
const FORMAT: u32 = 1;
const MAGIC: &[u8; 8] = b"PHPLSPIX";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Header {
    format: u32,
    version: String,
    php_version: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    hash: u64,
    stub: bool,
    symbols: FileSymbols,
}

#[derive(Serialize)]
struct EntryRef<'a> {
    path: &'a str,
    hash: u64,
    stub: bool,
    symbols: &'a FileSymbols,
}

type Map = HashMap<String, (u64, bool, Arc<FileSymbols>)>;

/// The cache of one workspace (see [`IndexCache::open`]).
pub struct IndexCache {
    path: PathBuf,
    header: Header,
    /// Loaded from disk.
    old: Map,
    /// Used during this run; written by [`IndexCache::save`].
    fresh: Mutex<Map>,
    hits: AtomicUsize,
}

/// FNV-1a over 8-byte words: fast and stable across Rust versions (unlike
/// `DefaultHasher`).
fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let bytes = text.as_bytes();
    let mut words = bytes.chunks_exact(8);
    for w in &mut words {
        h ^= u64::from_le_bytes(w.try_into().unwrap_or_default());
        h = h.wrapping_mul(0x0100_0000_01b3).rotate_left(23);
    }
    for b in words.remainder() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h ^ bytes.len() as u64
}

impl IndexCache {
    /// The cache for the workspace `roots` in `dir` (created on save).
    pub fn open(dir: &Path, roots: &[PathBuf], opts: ExtractOptions) -> IndexCache {
        let mut sorted: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        sorted.sort();
        let name = format!("index-{:016x}.bin", hash(&sorted.join("\n")));
        let header = Header {
            format: FORMAT,
            version: env!("CARGO_PKG_VERSION").into(),
            php_version: opts.php_version.to_string(),
        };
        let path = dir.join(name);
        let old = Self::load(&path, &header).unwrap_or_default();
        IndexCache { path, header, old, fresh: Mutex::new(HashMap::new()), hits: AtomicUsize::new(0) }
    }

    fn load(path: &Path, header: &Header) -> Option<Map> {
        let mut r = BufReader::new(std::fs::File::open(path).ok()?);
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic).ok()?;
        if &magic != MAGIC {
            return None;
        }
        let stored: Header = bincode::deserialize_from(&mut r).ok()?;
        if &stored != header {
            return None;
        }
        let entries: Vec<Entry> = bincode::deserialize_from(&mut r).ok()?;
        Some(entries.into_iter().map(|e| (e.path, (e.hash, e.stub, Arc::new(e.symbols)))).collect())
    }

    /// Where the cache is stored.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of entries loaded from disk.
    pub fn len(&self) -> usize {
        self.old.len()
    }

    pub fn is_empty(&self) -> bool {
        self.old.is_empty()
    }

    /// Files taken from the cache so far.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// The symbols of `path`, from the cache when its text is unchanged.
    pub(crate) fn symbols(
        &self,
        path: &str,
        text: &str,
        kind: FileKind,
        extract: impl FnOnce() -> FileSymbols,
    ) -> Arc<FileSymbols> {
        let h = hash(text);
        let stub = kind == FileKind::Stub;
        let symbols = match self.old.get(path) {
            Some((old_hash, old_stub, s)) if *old_hash == h && *old_stub == stub => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                s.clone()
            }
            _ => Arc::new(extract()),
        };
        if let Ok(mut fresh) = self.fresh.lock() {
            fresh.insert(path.to_string(), (h, stub, symbols.clone()));
        }
        symbols
    }

    /// Writes the entries used during this run (files that disappeared are
    /// dropped). The file is replaced atomically.
    pub fn save(&self) -> std::io::Result<()> {
        let fresh = self.fresh.lock().map_err(|_| std::io::Error::other("poisoned"))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut w = BufWriter::new(std::fs::File::create(&tmp)?);
            w.write_all(MAGIC)?;
            bincode::serialize_into(&mut w, &self.header).map_err(std::io::Error::other)?;
            let entries: Vec<EntryRef> = fresh
                .iter()
                .map(|(path, (hash, stub, symbols))| EntryRef { path, hash: *hash, stub: *stub, symbols })
                .collect();
            bincode::serialize_into(&mut w, &entries).map_err(std::io::Error::other)?;
            w.flush()?;
        }
        std::fs::rename(&tmp, &self.path)
    }

    /// Deletes the stored cache.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl std::fmt::Debug for IndexCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexCache").field("path", &self.path).field("entries", &self.old.len()).finish()
    }
}

/// `options` for extracting `kind` files.
pub(crate) fn opts_for(opts: ExtractOptions, kind: FileKind) -> ExtractOptions {
    ExtractOptions { stub: kind == FileKind::Stub, ..opts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("php-lsp-cache-unit-{}", std::process::id()));
        let opts = ExtractOptions::new(php_syntax::PhpVersion::PHP_8_4);
        let text = "<?php\n/** Doc. */\nclass A { public int $x = 1; public function f(?string $s = null): static {} }\n";
        let extract = || crate::indexer::extract_symbols("a.php", text, opts);
        let cache = IndexCache::open(&dir, std::slice::from_ref(&dir), opts);
        let first = cache.symbols("a.php", text, FileKind::Workspace, extract);
        cache.save().unwrap();
        let cache = IndexCache::open(&dir, std::slice::from_ref(&dir), opts);
        assert_eq!(cache.len(), 1);
        let second = cache.symbols("a.php", text, FileKind::Workspace, || panic!("not cached"));
        assert_eq!(first, second);
        assert_eq!(cache.hits(), 1);
        // another PHP version: not used
        let other = IndexCache::open(
            &dir,
            std::slice::from_ref(&dir),
            ExtractOptions::new(php_syntax::PhpVersion::PHP_7_4),
        );
        assert!(other.is_empty());
        cache.clear();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash(""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(hash("a"), hash("b"));
        assert_ne!(hash("abcdefgh1"), hash("abcdefgh2"));
    }
}
