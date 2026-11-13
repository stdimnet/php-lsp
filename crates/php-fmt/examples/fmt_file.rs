//! Prints the formatted version of a file to stdout.
fn main() {
    let path = std::env::args().nth(1).expect("usage: fmt_file <file>");
    let text = std::fs::read_to_string(path).unwrap();
    let edits = php_fmt::format(&text, &php_fmt::FormatOptions::default()).expect("syntax errors");
    print!("{}", php_fmt::apply(&text, &edits));
}
