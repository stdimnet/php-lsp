//! Using php-ide as a library: diagnostics, completion, hover, definition
//! and formatting for code held in memory.
//! Run: `cargo run -p php-ide --example embed`

use php_ide::{Config, FormatOptions, LineCol, TextSize, Workspace};

fn main() {
    let mut ws = Workspace::new(Config::default());
    // PHP's built-in functions and classes (phpstorm-stubs, embedded)
    ws.load_stubs();

    // project files that are not open (indexed for completion / navigation)
    ws.index_files(vec![(
        "file:///project/src/User.php".into(),
        "<?php\nnamespace App;\nclass User {\n    /** The display name. */\n    public function name(): string { return ''; }\n}\n".into(),
        php_ide::FileKind::Workspace,
    )]);

    // an open document
    let text = "<?php\nuse App\\User;\n$user = new User();\n$user->name();\necho strlen($user, 1);\n";
    let id = ws.set_file("file:///project/index.php", text.to_string());
    let lines = ws.line_index(id).expect("open file");
    let at = |line: u32, col: u32| -> TextSize { lines.offset(LineCol { line, col }) };

    for d in ws.diagnostics(id) {
        let start = lines.line_col(d.range.start());
        println!("{}:{} {} {}", start.line + 1, start.col + 1, d.code.as_str(), d.message);
    }

    let completions = ws.completions(id, at(3, 7));
    let labels: Vec<&str> = completions.items.iter().map(|i| i.label.as_str()).collect();
    println!("completion after `$user->`: {labels:?}");

    if let Some(hover) = ws.hover(id, at(3, 9)) {
        println!("hover:\n{}", hover.markdown);
    }

    for loc in ws.goto_definition(id, at(3, 9)) {
        println!("definition: {}", ws.file(loc.file).map_or("?", |f| &f.path));
    }

    let edits = ws.format(id, None, &FormatOptions::default()).unwrap_or_default();
    println!("{} formatting edits", edits.len());
}
