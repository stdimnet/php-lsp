//! Discovering and indexing files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use globset::{Glob, GlobSet, GlobSetBuilder};
use php_hir::{ExtractOptions, FileSymbols};
use php_index::FileKind;
use rayon::prelude::*;

/// A file analysed by [`index_files`], ready for [`crate::Workspace::add_indexed`].
#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub path: Arc<str>,
    pub text: Arc<str>,
    pub kind: FileKind,
    pub symbols: Arc<FileSymbols>,
}

pub(crate) fn extract_symbols(path: &str, text: &str, opts: ExtractOptions) -> FileSymbols {
    let parse = parse_source(path, text, opts.php_version);
    php_hir::analyze_file(&parse.tree(), opts).1
}

/// Parses a file; Blade templates are parsed as their virtual PHP code.
pub(crate) fn parse_source(path: &str, text: &str, version: php_syntax::PhpVersion) -> php_syntax::Parse {
    if crate::blade::is_blade(path) {
        php_syntax::parse_php_code(&crate::blade::virtual_php(text).text, version)
    } else {
        php_syntax::parse(text, version)
    }
}

/// Parses and analyses files in parallel.
pub fn index_files(files: Vec<(String, Arc<str>, FileKind)>, opts: ExtractOptions) -> Vec<IndexedFile> {
    files
        .into_par_iter()
        .map(|(path, text, kind)| {
            let opts = php_hir::ExtractOptions { stub: kind == FileKind::Stub, ..opts };
            let symbols = Arc::new(extract_symbols(&path, &text, opts));
            IndexedFile { path: path.into(), text, kind, symbols }
        })
        .collect()
}

/// Which files belong to a workspace (mirrors Intelephense's
/// `files.associations`, `files.exclude` and `files.maxSize`).
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub associations: Vec<String>,
    pub exclude: Vec<String>,
    pub max_size: u64,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            associations: vec!["*.php".into(), "*.phtml".into()],
            exclude: [
                "**/.git/**",
                "**/.svn/**",
                "**/.hg/**",
                "**/CVS/**",
                "**/.DS_Store/**",
                "**/node_modules/**",
                "**/bower_components/**",
                "**/vendor/**/{Tests,tests}/**",
                "**/.history/**",
                "**/vendor/**/vendor/**",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            max_size: 1_000_000,
        }
    }
}

fn globset(patterns: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = Glob::new(p) {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Finds all PHP files below `root`. Returns `(path, kind)` pairs where files
/// below a `vendor` directory are [`FileKind::Vendor`].
pub fn collect_files(root: &Path, config: &ScanConfig) -> Vec<(PathBuf, FileKind)> {
    let include = globset(&config.associations);
    let exclude = globset(&config.exclude);
    let mut out = Vec::new();
    let walker = walkdir::WalkDir::new(root).follow_links(true).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let rel = e.path().strip_prefix(root).unwrap_or(e.path());
        // directories: test with a trailing component so `**/x/**` matches
        if e.file_type().is_dir() {
            return !exclude.is_match(rel.join("_"));
        }
        true
    });
    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let name = Path::new(entry.file_name());
        if !include.is_match(name) || exclude.is_match(rel) {
            continue;
        }
        if entry.metadata().map(|m| m.len() > config.max_size).unwrap_or(true) {
            continue;
        }
        let kind = if rel.components().any(|c| c.as_os_str() == "vendor") {
            FileKind::Vendor
        } else {
            FileKind::Workspace
        };
        out.push((path.to_path_buf(), kind));
    }
    out
}
