//! Prints the syntax tree of a file: `cargo run -p php-syntax --example tree -- file.php`.

fn main() {
    let path = std::env::args().nth(1).expect("usage: tree <file.php>");
    let text = std::fs::read_to_string(path).expect("readable file");
    print!("{}", php_syntax::parse(&text, php_syntax::PhpVersion::LATEST).debug_tree());
}
