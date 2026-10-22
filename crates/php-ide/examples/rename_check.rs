//! Consistency check of rename on a real project: renames sampled symbols,
//! checks that the result has no new diagnostics and that renaming back
//! restores every file.
//! Usage: `cargo run --release -p php-ide --example rename_check -- <dir> [FILES] [PER_FILE]`

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use php_ide::syntax::{SyntaxKind, TextSize};
use php_ide::{collect_files, Config, FileId, FileKind, ScanConfig, Workspace};

fn apply(text: &str, edits: &[php_ide::TextEdit]) -> String {
    let mut edits: Vec<_> = edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    let mut out = text.to_string();
    for e in edits {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    out
}

fn codes(ws: &Workspace, id: FileId) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for d in ws.diagnostics(id) {
        *m.entry(d.code.as_str().to_string()).or_default() += 1;
    }
    m
}

/// Positions of renamable names: members, declarations, variables.
fn candidates(ws: &Workspace, id: FileId, max: usize) -> Vec<TextSize> {
    let text = &ws.file(id).unwrap().text;
    let root = php_ide::syntax::parse(text, ws.config().php_version).syntax_node();
    let all: Vec<TextSize> = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| match t.kind() {
            SyntaxKind::VARIABLE => t.text() != "$this",
            SyntaxKind::IDENT => {
                t.parent().is_some_and(|p| matches!(p.kind(), SyntaxKind::MEMBER_NAME | SyntaxKind::NAME))
            }
            _ => false,
        })
        .map(|t| t.text_range().start() + TextSize::from(1))
        .collect();
    if all.len() <= max {
        return all;
    }
    let step = all.len() as f64 / max as f64;
    (0..max).map(|i| all[(i as f64 * step) as usize]).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: rename_check <dir> [FILES] [PER_FILE]");
    let n_files: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let per_file: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut ws = Workspace::new(Config::default());
    ws.load_stubs();
    let files = collect_files(std::path::Path::new(&root), &ScanConfig::default());
    let texts: Vec<_> = files
        .into_iter()
        .filter_map(|(p, kind)| {
            Some((
                format!("file://{}", p.display()),
                Arc::<str>::from(std::fs::read_to_string(&p).ok()?),
                kind,
            ))
        })
        .collect();
    ws.index_files(texts);
    let mut ids: Vec<FileId> = ws
        .files()
        .filter(|(_, f)| {
            f.kind == FileKind::Workspace && f.path.ends_with(".php") && !f.path.ends_with(".blade.php")
        })
        .map(|(id, _)| id)
        .collect();
    if ids.len() > n_files {
        let step = ids.len() as f64 / n_files as f64;
        ids = (0..n_files).map(|i| ids[(i as f64 * step) as usize]).collect();
    }
    let (mut renamed, mut refused, mut failures) = (0, 0, 0);
    let mut edits_total = 0;
    let started = std::time::Instant::now();
    for (n, id) in ids.iter().enumerate() {
        for offset in candidates(&ws, *id, per_file) {
            let Ok((range, old)) = ws.prepare_rename(*id, offset) else {
                refused += 1;
                continue;
            };
            let new_name =
                if old.starts_with('$') { format!("$zzRenamed{n}") } else { format!("zzRenamed{n}") };
            let Ok(change) = ws.rename(*id, offset, &new_name) else {
                refused += 1;
                continue;
            };
            let originals: HashMap<FileId, String> =
                change.edits.iter().map(|(f, _)| (*f, ws.file(*f).unwrap().text.to_string())).collect();
            let before: HashMap<FileId, _> = change.edits.iter().map(|(f, _)| (*f, codes(&ws, *f))).collect();
            let at = {
                let f = ws.file(*id).unwrap();
                let lc = f.line_index.line_col(range.start());
                format!("{}:{}:{} {old}", f.path, lc.line + 1, lc.col + 1)
            };
            // the new position of the renamed name in this file
            let mut new_offset = range.start();
            for e in change.edits.iter().filter(|(f, _)| f == id).flat_map(|(_, e)| e) {
                if e.range.end() <= range.start() {
                    new_offset += TextSize::from(e.new_text.len() as u32);
                    new_offset -= e.range.len();
                }
            }
            for (f, edits) in &change.edits {
                edits_total += edits.len();
                let path = ws.file(*f).unwrap().path.to_string();
                let text = apply(&originals[f], edits);
                ws.set_file(&path, text);
            }
            renamed += 1;
            let mut problem = Vec::new();
            for (f, old_codes) in &before {
                let new_codes = codes(&ws, *f);
                for (code, count) in &new_codes {
                    if count > old_codes.get(code).unwrap_or(&0) {
                        problem.push(format!("new {code} in {}", ws.file(*f).unwrap().path));
                    }
                }
            }
            // rename back
            match ws.rename(*id, new_offset + TextSize::from(1), &old) {
                Ok(back) => {
                    for (f, edits) in &back.edits {
                        let path = ws.file(*f).unwrap().path.to_string();
                        let text = apply(&ws.file(*f).unwrap().text, edits);
                        ws.set_file(&path, text);
                    }
                    for (f, original) in &originals {
                        if *ws.file(*f).unwrap().text != **original {
                            problem.push(format!("not restored: {}", ws.file(*f).unwrap().path));
                        }
                    }
                }
                Err(e) => problem.push(format!("rename back failed: {}", e.0)),
            }
            for (f, original) in &originals {
                let path = ws.file(*f).unwrap().path.to_string();
                ws.set_file(&path, original.clone());
            }
            if !problem.is_empty() {
                failures += 1;
                println!("{at}: {}", problem.join("; "));
            }
        }
    }
    eprintln!(
        "{renamed} renamed ({edits_total} edits), {refused} refused, {failures} with problems, {:?}",
        started.elapsed()
    );
}
