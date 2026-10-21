//! Runs all diagnostics over every workspace file of a project and prints a
//! summary per code (useful to find false positives).
//! Usage: `cargo run --release -p php-ide --example diagnose_project -- <dir> [CODE] [N]`
//! (`ONLY=.blade.php` restricts the report to files with that suffix)

use std::collections::BTreeMap;
use std::sync::Arc;

use php_ide::{collect_files, Config, FileKind, ScanConfig, Workspace};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: diagnose_project <dir> [CODE] [N]");
    let show = args.next();
    let n: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
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
    let only = std::env::var("ONLY").unwrap_or_default();
    let ids: Vec<_> = ws
        .files()
        .filter(|(_, f)| f.kind == FileKind::Workspace && f.path.ends_with(only.as_str()))
        .map(|(id, _)| id)
        .collect();
    let t = std::time::Instant::now();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut shown = 0;
    for id in &ids {
        for d in ws.diagnostics(*id) {
            *counts.entry(d.code.as_str()).or_default() += 1;
            if show.as_deref() == Some(d.code.as_str()) && shown < n {
                shown += 1;
                let f = ws.file(*id).unwrap();
                let lc = f.line_index.line_col(d.range.start());
                println!(
                    "{}:{}: {} [{}]",
                    f.path.trim_start_matches("file://"),
                    lc.line + 1,
                    d.message,
                    f.text[d.range].chars().take(60).collect::<String>()
                );
            }
        }
    }
    println!("{} files in {:?}", ids.len(), t.elapsed());
    for (code, n) in counts {
        println!("{code}: {n}");
    }
}
