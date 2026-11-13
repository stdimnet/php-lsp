//! Parses every `.php` file below the given directories and reports files
//! with syntax errors. Usage: `cargo run --release --example parse_corpus -- <dir>...`

use std::path::{Path, PathBuf};
use std::time::Instant;

use php_syntax::{parse, PhpVersion};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    let mut version = PhpVersion::LATEST;
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--php=") {
            version = v.parse().expect("invalid version");
        } else {
            collect(Path::new(&arg), &mut files);
        }
    }
    let start = Instant::now();
    let mut bytes = 0;
    let mut with_errors = 0;
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        bytes += text.len();
        let p = parse(&text, version);
        assert_eq!(p.syntax_node().text().to_string(), text, "lossless {}", f.display());
        if !p.errors().is_empty() {
            with_errors += 1;
            let e = &p.errors()[0];
            let line = text[..usize::from(e.range.start()).min(text.len())].matches('\n').count() + 1;
            println!("{}:{}: {} ({} errors)", f.display(), line, e.message, p.errors().len());
        }
    }
    eprintln!(
        "{} files, {} with errors, {:.1} MB in {:?}",
        files.len(),
        with_errors,
        bytes as f64 / 1e6,
        start.elapsed()
    );
}
