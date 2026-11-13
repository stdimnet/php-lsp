//! Indexes a project and prints type, hover and definition at file:line:col.
//! Usage: `cargo run --release -p php-ide --example query -- <dir> <file> <line> <col>` (1-based)

use std::sync::Arc;

use php_ide::{collect_files, Config, LineCol, ScanConfig, Workspace};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (root, file, line, col) =
        (&args[0], &args[1], args[2].parse::<u32>().unwrap(), args[3].parse::<u32>().unwrap());
    let mut ws = Workspace::new(Config::default());
    ws.load_stubs();
    let texts: Vec<_> = collect_files(std::path::Path::new(root), &ScanConfig::default())
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
    let key = format!("file://{file}");
    let text = std::fs::read_to_string(file).unwrap();
    let id = ws.set_file(&key, text);
    let idx = ws.line_index(id).unwrap();
    let offset = idx.offset(LineCol { line: line - 1, col: col - 1 });
    println!("type: {:?}", ws.type_at(id, offset));
    println!("definition: {:?}", ws.definition_at(id, offset));
    println!("hover: {:?}", ws.hover(id, offset).map(|h| h.markdown));
}
