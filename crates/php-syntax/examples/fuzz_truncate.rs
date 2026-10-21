//! Robustness check: parses many truncated / mutated variants of the given
//! files and makes sure the parser neither panics nor loses text.

use php_syntax::{parse, PhpVersion};

fn main() {
    let mut seed: u64 = 0x9e3779b97f4a7c15;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut runs = 0;
    for path in std::env::args().skip(1) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let step = (text.len() / 40).max(1);
        let mut cut = 0;
        while cut < text.len() {
            if text.is_char_boundary(cut) {
                let t = &text[..cut];
                assert_eq!(parse(t, PhpVersion::LATEST).syntax_node().text().to_string(), t);
                runs += 1;
            }
            cut += step;
        }
        for _ in 0..40 {
            let mut bytes = text.clone().into_bytes();
            for _ in 0..5 {
                if bytes.is_empty() {
                    break;
                }
                let i = (rand() as usize) % bytes.len();
                if bytes[i].is_ascii() {
                    let set = b"{}()[];$'\"<?>\\/*#-=:,&|";
                    let replacement = set[(rand() as usize) % set.len()];
                    bytes[i] = replacement;
                }
            }
            if let Ok(t) = String::from_utf8(bytes) {
                assert_eq!(parse(&t, PhpVersion::LATEST).syntax_node().text().to_string(), t);
                runs += 1;
            }
        }
    }
    eprintln!("{runs} parses ok");
}
