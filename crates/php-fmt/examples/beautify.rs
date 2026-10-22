//! Formats files with the js-beautify port: `beautify html|html-only|js|css <out dir> <files>...`
//! (writes `<out dir>/<n>.out` per file; for comparisons with js-beautify).

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("usage: beautify html|template <out dir> <files>...");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out).expect("out dir");
    for (n, f) in args.enumerate() {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let result = match mode.as_str() {
            "html" => php_fmt::beautify::html(&text, 4, false),
            "html-only" => php_fmt::beautify::html_only(&text, 4, false),
            "js" => php_fmt::beautify::javascript(&text, 4, false),
            "css" => php_fmt::beautify::css(&text, 4, false),
            _ => panic!("unknown mode {mode}"),
        };
        std::fs::write(out.join(format!("{n}.out")), result).expect("write");
    }
}
