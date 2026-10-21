//! PHP language server.
//!
//! Embeddable: create a [`lsp_server::Connection`] (stdio, TCP or an in-memory
//! pair) and hand it to [`Server::initialize`]:
//!
//! ```no_run
//! let (connection, io_threads) = lsp_server::Connection::stdio();
//! php_lsp::Server::initialize(connection).unwrap().run().unwrap();
//! io_threads.join().unwrap();
//! ```

pub mod config;
pub mod convert;
pub mod ext;
mod handlers;
pub mod node_ipc;
mod server;
pub mod uri;

pub use lsp_server;
pub use server::{Result, Server, SERVER_NAME};

/// Runs the server over stdin/stdout until the client exits.
pub fn run_stdio() -> Result<()> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    Server::initialize(connection)?.run()?;
    io_threads.join()?;
    Ok(())
}

/// Runs the server over the Node IPC channel (`--node-ipc`).
pub fn run_node_ipc() -> Result<()> {
    let (connection, writer) = node_ipc::connect()?;
    Server::initialize(connection)?.run()?;
    let _ = writer.join();
    Ok(())
}

/// Connects to a client listening on `127.0.0.1:port` (Intelephense's
/// `--socket=<port>` mode) and runs the server.
pub fn run_socket(port: u16) -> Result<()> {
    let (connection, io_threads) = lsp_server::Connection::connect(("127.0.0.1", port))?;
    Server::initialize(connection)?.run()?;
    io_threads.join()?;
    Ok(())
}
