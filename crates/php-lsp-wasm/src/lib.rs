//! JavaScript interface of the PHP language server (WebAssembly).
//!
//! ```js
//! import { LanguageServer, format } from "php-lsp-wasm";
//! const server = new LanguageServer(JSON.stringify({ capabilities: {} }));
//! server.send(JSON.stringify({ jsonrpc: "2.0", method: "textDocument/didOpen", params: { ... } }));
//! for (const message of JSON.parse(server.receive())) { ... }
//! ```
//!
//! Everything runs on the calling thread (no background indexing): files
//! become known through `textDocument/didOpen` or the `php/indexFiles`
//! notification of the server.

use wasm_bindgen::prelude::*;

/// A PHP language server speaking LSP (JSON-RPC messages as strings).
#[wasm_bindgen]
pub struct LanguageServer {
    server: php_lsp::Embedded,
    capabilities: String,
}

#[wasm_bindgen]
impl LanguageServer {
    /// Starts a server with the JSON of the LSP `initialize` params.
    #[wasm_bindgen(constructor)]
    pub fn new(initialize_params: &str) -> Result<LanguageServer, JsError> {
        let params: serde_json::Value =
            serde_json::from_str(initialize_params).map_err(|e| JsError::new(&e.to_string()))?;
        let (server, result) = php_lsp::Embedded::new(params).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(LanguageServer { server, capabilities: result.to_string() })
    }

    /// The `initialize` result (JSON): the server's capabilities.
    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> String {
        self.capabilities.clone()
    }

    /// Handles one JSON-RPC message; returns `true` after `exit`.
    pub fn send(&mut self, message: &str) -> Result<bool, JsError> {
        self.server.send(message).map_err(|e| JsError::new(&e.to_string()))
    }

    /// The server's messages since the last call, as a JSON array.
    pub fn receive(&mut self) -> String {
        format!("[{}]", self.server.receive().join(","))
    }
}

/// Formats a document (`fileName` selects PHP / Blade / Twig) with JSON
/// options like `{"tabSize": 4, "insertSpaces": true}`; `undefined` on
/// syntax errors.
#[wasm_bindgen]
pub fn format(text: &str, file_name: &str, options: &str) -> Option<String> {
    let options = serde_json::from_str(options).unwrap_or(serde_json::Value::Null);
    php_lsp::format_document(text, file_name, &options)
}

/// The version of the server.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
