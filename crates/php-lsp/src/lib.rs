//! PHP language server.
//!
//! In another program: [`Embedded`] takes LSP messages as JSON strings and
//! returns the server's messages, no stdio needed. Or create a
//! [`lsp_server::Connection`] (stdio, TCP or an in-memory pair) and hand it
//! to [`Server::initialize`]:
//!
//! ```no_run
//! let (connection, io_threads) = lsp_server::Connection::stdio();
//! php_lsp::Server::initialize(connection).unwrap().run().unwrap();
//! io_threads.join().unwrap();
//! ```

pub mod config;
pub mod convert;
mod embedded;
pub mod ext;
mod handlers;
pub mod node_ipc;
mod server;
pub mod uri;

pub use embedded::{format_document, Embedded};
pub use lsp_server;
pub use server::{Result, Server, SERVER_NAME, VERSION};

/// Runs the server over stdin/stdout until the client exits.
pub fn run_stdio() -> Result<()> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    run_on_large_stack(connection)?;
    io_threads.join()?;
    Ok(())
}

/// Runs the server over the Node IPC channel (`--node-ipc`).
pub fn run_node_ipc() -> Result<()> {
    let (connection, writer) = node_ipc::connect()?;
    run_on_large_stack(connection)?;
    let _ = writer.join();
    Ok(())
}

/// Connects to a client listening on `127.0.0.1:port` (Intelephense's
/// `--socket=<port>` mode) and runs the server.
pub fn run_socket(port: u16) -> Result<()> {
    let (connection, io_threads) = lsp_server::Connection::connect(("127.0.0.1", port))?;
    run_on_large_stack(connection)?;
    io_threads.join()?;
    Ok(())
}

/// Requests are handled on a thread with a large stack: deeply nested
/// syntax trees are walked recursively, and the main thread has only 1 MB
/// on Windows.
fn run_on_large_stack(connection: lsp_server::Connection) -> Result<()> {
    let thread = std::thread::Builder::new().name("php-lsp".into()).stack_size(php_ide::STACK_SIZE).spawn(
        move || -> std::result::Result<(), String> {
            Server::initialize(connection).and_then(Server::run).map_err(|e| e.to_string())
        },
    )?;
    match thread.join() {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err("the server thread panicked".into()),
    }
}
