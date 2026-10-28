//! `php-lsp` binary. Accepts the same transport flags as Intelephense
//! (`--stdio`, `--socket=<port>`, `--node-ipc`) so it can be used as a drop-in
//! replacement.

use std::process::ExitCode;

const USAGE: &str = "\
php-lsp - PHP language server

USAGE:
    php-lsp [--stdio | --socket=<port> | --node-ipc]

OPTIONS:
    --stdio            communicate over stdin/stdout (default)
    --socket=<port>    connect to a client listening on 127.0.0.1:<port>
    --node-ipc         use the Node.js IPC channel (NODE_CHANNEL_FD)
    --version          print the version
    --help             print this help
";

fn main() -> ExitCode {
    let mut socket = None;
    let mut node_ipc = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--stdio" => {}
            "--version" | "-V" => {
                println!("php-lsp {}", php_lsp::VERSION);
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--node-ipc" => node_ipc = true,
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
        None if node_ipc => php_lsp::run_node_ipc(),
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
