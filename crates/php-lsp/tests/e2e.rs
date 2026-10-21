//! End-to-end tests talking LSP to an in-process server.

use std::thread;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

struct Client {
    conn: Connection,
    server: Option<thread::JoinHandle<()>>,
    next_id: i32,
}

impl Client {
    fn start(init_options: Value) -> Client {
        let (server_conn, client_conn) = Connection::memory();
        let server = thread::spawn(move || {
            php_lsp::Server::initialize(server_conn).unwrap().run().unwrap();
        });
        let mut client = Client { conn: client_conn, server: Some(server), next_id: 0 };
        let result = client.request(
            "initialize",
            json!({
                "capabilities": {"general": {"positionEncodings": ["utf-16"]}},
                "initializationOptions": init_options,
            }),
        );
        assert_eq!(result["serverInfo"]["name"], "php-lsp");
        assert_eq!(result["capabilities"]["positionEncoding"], "utf-16");
        client.notify("initialized", json!({}));
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn
            .sender
            .send(Message::Request(Request { id: id.clone(), method: method.into(), params }))
            .unwrap();
        loop {
            match self.conn.receiver.recv().unwrap() {
                Message::Response(r) if r.id == id => return r.response_result.expect("request failed"),
                _ => {}
            }
        }
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn.sender.send(Message::Notification(Notification { method: method.into(), params })).unwrap();
    }

    fn wait_notification(&self, method: &str) -> Value {
        loop {
            match self.conn.receiver.recv().unwrap() {
                Message::Notification(n) if n.method == method => return n.params,
                _ => {}
            }
        }
    }

    fn open(&self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}}),
        );
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        self.server.take().unwrap().join().unwrap();
    }
}

#[test]
fn diagnostics_symbols_folding_selection() {
    let mut c = Client::start(json!({}));
    let uri = "file:///project/a.php";
    c.open(uri, "<?php\nclass A {\n    function f() {\n        return 1\n    }\n}\n");

    let diags = c.wait_notification("textDocument/publishDiagnostics");
    assert_eq!(diags["uri"], uri);
    let d = &diags["diagnostics"][0];
    assert_eq!(d["message"], "expected `;`");
    assert_eq!(d["range"]["start"], json!({"line": 3, "character": 16}));
    assert_eq!(d["source"], "php-lsp");

    let symbols = c.request("textDocument/documentSymbol", json!({"textDocument": {"uri": uri}}));
    assert_eq!(symbols[0]["name"], "A");
    assert_eq!(symbols[0]["kind"], 5);
    assert_eq!(symbols[0]["children"][0]["name"], "f");
    assert_eq!(symbols[0]["children"][0]["kind"], 6);

    let folds = c.request("textDocument/foldingRange", json!({"textDocument": {"uri": uri}}));
    let folds: Vec<(u64, u64)> = folds
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["startLine"].as_u64().unwrap(), f["endLine"].as_u64().unwrap()))
        .collect();
    assert_eq!(folds, vec![(1, 4), (2, 3)]);

    let sel = c.request(
        "textDocument/selectionRange",
        json!({"textDocument": {"uri": uri}, "positions": [{"line": 3, "character": 15}]}),
    );
    assert_eq!(
        sel[0]["range"],
        json!({"start": {"line": 3, "character": 15}, "end": {"line": 3, "character": 16}})
    );
    assert!(sel[0]["parent"].is_object());

    c.shutdown();
}

#[test]
fn incremental_changes_with_utf16() {
    let mut c = Client::start(json!({}));
    let uri = "file:///b.php";
    c.open(uri, "<?php\n$s = '😀'; $x = 1\n");
    let d = c.wait_notification("textDocument/publishDiagnostics");
    assert_eq!(d["diagnostics"].as_array().unwrap().len(), 1);
    // insert the missing `;` after `1` (utf-16 column: emoji counts as 2)
    c.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"range": {"start": {"line": 1, "character": 18}, "end": {"line": 1, "character": 18}}, "text": ";"}]
        }),
    );
    let d = c.wait_notification("textDocument/publishDiagnostics");
    assert_eq!(d["version"], 2);
    assert_eq!(d["diagnostics"], json!([]));
    let tree = c.request("php-lsp/syntaxTree", json!({"textDocument": {"uri": uri}}));
    assert!(tree.as_str().unwrap().contains("SEMICOLON@"));
    c.shutdown();
}

#[test]
fn php_version_from_intelephense_settings() {
    let c = Client::start(json!({}));
    let uri = "file:///c.php";
    c.notify(
        "workspace/didChangeConfiguration",
        json!({"settings": {"intelephense": {"environment": {"phpVersion": "7.4.0"}}}}),
    );
    c.open(uri, "<?php enum A {}\n");
    let d = c.wait_notification("textDocument/publishDiagnostics");
    assert!(d["diagnostics"][0]["message"].as_str().unwrap().contains("enums"));
    c.notify(
        "workspace/didChangeConfiguration",
        json!({"settings": {"intelephense.environment.phpVersion": "8.3"}}),
    );
    let d = c.wait_notification("textDocument/publishDiagnostics");
    assert_eq!(d["diagnostics"], json!([]));
    c.shutdown();
}
