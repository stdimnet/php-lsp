//! The language server inside another program: LSP messages (JSON) go in,
//! responses and notifications come out, without stdio or sockets.
//!
//! ```
//! use php_lsp::Embedded;
//! use serde_json::json;
//!
//! let (mut server, _capabilities) = Embedded::new(json!({ "capabilities": {} })).unwrap();
//! server.send(&json!({
//!     "jsonrpc": "2.0", "method": "textDocument/didOpen",
//!     "params": { "textDocument": {
//!         "uri": "file:///a.php", "languageId": "php", "version": 1,
//!         "text": "<?php\nfunction greet() {}\ngreet();\n" } }
//! }).to_string()).unwrap();
//! server.send(&json!({
//!     "jsonrpc": "2.0", "id": 1, "method": "textDocument/hover",
//!     "params": { "textDocument": { "uri": "file:///a.php" }, "position": { "line": 2, "character": 1 } }
//! }).to_string()).unwrap();
//! let messages = server.receive();
//! assert!(messages.iter().any(|m| m.contains("\"id\":1") && m.contains("greet")));
//! ```

use std::time::Duration;

use crossbeam_channel::Receiver;
use lsp_server::{Connection, Message};
use serde_json::Value;

use crate::server::{Result, Server};

/// An in-process language server (see the module documentation).
pub struct Embedded {
    server: Server,
    outgoing: Receiver<Message>,
    exited: bool,
}

impl Embedded {
    /// Starts a server with the parameters of an LSP `initialize` request
    /// and returns it together with the `initialize` result (the server's
    /// capabilities).
    pub fn new(initialize_params: Value) -> Result<(Embedded, Value)> {
        let (server_side, client_side) = Connection::memory();
        let (mut server, result) = Server::create(server_side, initialize_params)?;
        server.on_initialized();
        let mut embedded = Embedded { server, outgoing: client_side.receiver, exited: false };
        embedded.run_tasks();
        Ok((embedded, result))
    }

    /// Handles one JSON-RPC message (request, notification or a response
    /// to a server request). Returns `true` after `exit`.
    pub fn send(&mut self, message: &str) -> Result<bool> {
        let message: Message = serde_json::from_str(message)?;
        Ok(self.send_message(message))
    }

    /// Like [`Embedded::send`] for a parsed message.
    pub fn send_message(&mut self, message: Message) -> bool {
        if self.exited {
            return true;
        }
        self.exited = self.server.on_message(message);
        self.run_tasks();
        self.exited
    }

    /// The messages the server sent since the last call (responses,
    /// diagnostics, progress, requests to the client), as JSON. Results of
    /// finished background work (indexing) are handled first.
    pub fn receive(&mut self) -> Vec<String> {
        self.run_tasks();
        self.outgoing.try_iter().filter_map(|m| serde_json::to_string(&m).ok()).collect()
    }

    /// Waits up to `timeout` for background work or a message, then
    /// returns like [`Embedded::receive`].
    pub fn wait(&mut self, timeout: Duration) -> Vec<String> {
        if self.outgoing.is_empty() {
            if let Ok(task) = self.server.task_rx.recv_timeout(timeout) {
                self.server.on_task(task);
            }
        }
        self.receive()
    }

    /// Whether the client sent `exit`.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// The workspace behind the server (for direct API use).
    pub fn workspace(&self) -> &php_ide::Workspace {
        self.server.workspace()
    }

    fn run_tasks(&mut self) {
        while let Ok(task) = self.server.task_rx.try_recv() {
            self.server.on_task(task);
        }
    }
}

/// Formats a document without a running server. `file_name` decides the
/// language (`.blade.php`, `.twig`, else PHP); `options` is JSON like
/// `{"tabSize": 4, "insertSpaces": true, "braces": "psr12", "html": true,
/// "phpVersion": "8.4"}` (all optional). `None` if the code has syntax
/// errors.
pub fn format_document(text: &str, file_name: &str, options: &Value) -> Option<String> {
    let mut config = php_ide::Config::default();
    if let Some(v) = options.get("phpVersion").and_then(Value::as_str).and_then(|v| v.parse().ok()) {
        config.php_version = v;
    }
    let mut ws = php_ide::Workspace::new(config);
    let path = if file_name.contains("://") { file_name.to_string() } else { format!("file:///{file_name}") };
    let id = ws.set_file(&path, text.to_string());
    let opts = php_ide::FormatOptions {
        indent_size: options.get("tabSize").and_then(Value::as_u64).unwrap_or(4).max(1) as usize,
        use_tabs: !options.get("insertSpaces").and_then(Value::as_bool).unwrap_or(true),
        braces: match options
            .get("braces")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "allman" => php_ide::BraceStyle::Allman,
            "k&r" | "kandr" => php_ide::BraceStyle::KAndR,
            _ => php_ide::BraceStyle::Psr12,
        },
        format_html: options.get("html").and_then(Value::as_bool).unwrap_or(true),
        ..Default::default()
    };
    let mut edits = ws.format(id, None, &opts)?;
    edits.sort_by_key(|e| std::cmp::Reverse((e.range.start(), e.range.end())));
    let mut out = text.to_string();
    for e in edits {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    Some(out)
}
