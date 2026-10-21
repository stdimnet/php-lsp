//! `php-lsp` binary. Accepts the same transport flags as Intelephense
//! (`--stdio`, `--socket=<port>`) so it can be used as a drop-in replacement.

use std::process::ExitCode;

const USAGE: &str = "\
php-lsp - PHP language server

USAGE:
    php-lsp [--stdio | --socket=<port>]

OPTIONS:
    --stdio            communicate over stdin/stdout (default)
    --socket=<port>    connect to a client listening on 127.0.0.1:<port>
    --version          print the version
    --help             print this help
";

fn main() -> ExitCode {
    let mut socket = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--stdio" => {}
            "--version" | "-V" => {
                println!("php-lsp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--node-ipc" => {
                eprintln!("php-lsp: --node-ipc is not supported yet, use --stdio");
                return ExitCode::FAILURE;
            }
            _ if arg.starts_with("--socket=") => match arg["--socket=".len()..].parse::<u16>() {
                Ok(port) => socket = Some(port),
                Err(_) => {
                    eprintln!("php-lsp: invalid port in {arg}");
                    return ExitCode::FAILURE;
                }
            },
            // Ignore unknown flags (e.g. `--clientProcessId=` passed by some clients).
            _ => {}
        }
    }
    let result = match socket {
        Some(port) => php_lsp::run_socket(port),
        None => php_lsp::run_stdio(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("php-lsp: {e}");
            ExitCode::FAILURE
        }
    }
}
