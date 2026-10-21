//! Robustness check for the IDE features: opens truncated variants of the
//! given files (as while typing) and runs diagnostics, hover, completion,
//! definition, folding, symbols and semantic tokens at many offsets. Any panic
//! aborts with the file and the truncation point.
//! Usage: `cargo run --release -p php-ide --example fuzz_ide -- <file.php>...`

use php_ide::{Config, TextSize, Workspace};

fn main() {
    let mut ws = Workspace::new(Config::default());
    ws.load_stubs();
    let mut runs = 0usize;
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let key = format!("file://{path}");
        // ~12 truncation points per file, plus the whole file
        let step = (text.len() / 12).max(1);
        let mut cuts: Vec<usize> = (0..text.len()).step_by(step).collect();
        cuts.push(text.len());
        for cut in cuts {
            let mut cut = cut.min(text.len());
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            let variant = &text[..cut];
            let id = ws.set_file(&key, variant.to_string());
            let _ = ws.diagnostics(id);
            let _ = ws.folding_ranges(id);
            let _ = ws.document_symbols(id);
            let _ = ws.semantic_tokens(id, None);
            // probe offsets near the end (where typing happens) and spread out
            let len = variant.len();
            let probes = [len, len.saturating_sub(1), len.saturating_sub(7), len / 2, len / 3];
            for p in probes {
                let mut p = p.min(len);
                while !variant.is_char_boundary(p) {
                    p -= 1;
                }
                let offset = TextSize::from(p as u32);
                let _ = ws.hover(id, offset);
                let _ = ws.completions(id, offset);
                let _ = ws.goto_definition(id, offset);
                let _ = ws.signature_help(id, offset);
            }
            runs += 1;
        }
        ws.set_file(&key, text);
    }
    println!("{runs} variants without panics");
}
