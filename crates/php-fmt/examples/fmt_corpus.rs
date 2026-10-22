//! Formats every `.php` file below the given directories and checks that
//! the token stream is unchanged and formatting is idempotent. Prints the
//! files with the most changed lines (for PSR-12 code this should be ~0).

use std::path::{Path, PathBuf};

use php_fmt::{apply, format, FormatOptions};
use php_syntax::{PhpVersion, SyntaxKind};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e == "php") {
            out.push(p);
        }
    }
}

fn tokens(text: &str) -> Vec<(SyntaxKind, String)> {
    php_syntax::parse(text, PhpVersion::LATEST)
        .syntax_node()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() != SyntaxKind::WHITESPACE)
        .map(|t| {
            // doc comments may be re-indented
            let text = if t.kind() == SyntaxKind::OPEN_TAG {
                t.text().trim_end().to_string()
            } else if t.kind() == SyntaxKind::DOC_COMMENT || t.kind() == SyntaxKind::COMMENT {
                t.text().split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                t.text().to_string()
            };
            (t.kind(), text)
        })
        .collect()
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        collect(Path::new(&a), &mut files);
    }
    let opts = FormatOptions::default();
    let (mut ok, mut skipped, mut changed_lines, mut failures) = (0, 0, 0usize, 0);
    let mut worst: Vec<(usize, PathBuf)> = Vec::new();
    let start = std::time::Instant::now();
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let Ok(edits) = format(&text, &opts) else {
            skipped += 1;
            continue;
        };
        let out = apply(&text, &edits);
        if tokens(&out) != tokens(&text) {
            failures += 1;
            println!("TOKENS CHANGED: {}", f.display());
            continue;
        }
        let again = apply(&out, &format(&out, &opts).unwrap());
        if again != out {
            failures += 1;
            println!("NOT IDEMPOTENT: {}", f.display());
            continue;
        }
        let n = text.lines().zip(out.lines()).filter(|(a, b)| a != b).count();
        changed_lines += n;
        worst.push((n, f.clone()));
        ok += 1;
    }
    worst.sort_by_key(|w| std::cmp::Reverse(w.0));
    for (n, f) in worst.iter().take(8) {
        println!("{n:6} changed lines: {}", f.display());
    }
    eprintln!("{ok} ok, {skipped} skipped (syntax errors), {failures} failures, {changed_lines} changed lines, {:?}", start.elapsed());
}
