//! Indexes a project directory and runs a few queries, printing timings.
//! Usage: `cargo run --release -p php-ide --example index_project -- <dir> [ClassName]`

use std::sync::Arc;
use std::time::Instant;

use php_ide::{collect_files, Config, ScanConfig, Workspace};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: index_project <dir> [ClassName]");
    let query = args.next().unwrap_or_else(|| "Collection".into());
    let mut ws = Workspace::new(Config::default());

    let t = Instant::now();
    ws.load_stubs();
    println!("stubs: {} files in {:?}", ws.files().count(), t.elapsed());

    let t = Instant::now();
    let files = collect_files(std::path::Path::new(&root), &ScanConfig::default());
    let scan = t.elapsed();
    let texts: Vec<_> = files
        .into_iter()
        .filter_map(|(p, kind)| {
            let text = std::fs::read_to_string(&p).ok()?;
            Some((format!("file://{}", p.display()), Arc::<str>::from(text), kind))
        })
        .collect();
    let n = texts.len();
    let t2 = Instant::now();
    ws.index_files(texts);
    println!("project: {n} files, scan {scan:?}, index {:?}", t2.elapsed());

    let t = Instant::now();
    let syms = ws.workspace_symbols(&query, 5);
    println!(
        "workspace symbols '{query}': {:?} in {:?}",
        syms.iter().map(|s| &s.name).collect::<Vec<_>>(),
        t.elapsed()
    );

    if let Some(sym) = syms.first() {
        let file = ws.file(sym.file).unwrap().path.clone();
        let text = ws.file(sym.file).unwrap().text.to_string();
        let id = ws.set_file(&file, text);
        let t = Instant::now();
        let refs = ws.references(id, sym.range.start(), true);
        println!("references of {}: {} in {:?}", sym.name, refs.len(), t.elapsed());
        let t = Instant::now();
        let h = ws.hover(id, sym.range.start());
        println!(
            "hover in {:?}: {}",
            t.elapsed(),
            h.map(|h| h.markdown.lines().take(4).collect::<Vec<_>>().join(" | ")).unwrap_or_default()
        );
    }
}
