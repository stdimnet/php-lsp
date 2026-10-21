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
        Client::start_with(
            json!({"capabilities": {"general": {"positionEncodings": ["utf-16"]}}, "initializationOptions": init_options}),
        )
    }

    fn start_with(params: Value) -> Client {
        let (server_conn, client_conn) = Connection::memory();
        let server = thread::spawn(move || {
            php_lsp::Server::initialize(server_conn).unwrap().run().unwrap();
        });
        let mut client = Client { conn: client_conn, server: Some(server), next_id: 0 };
        let result = client.request("initialize", params);
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
            let msg = self
                .conn
                .receiver
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("timed out waiting for notification");
            if std::env::var("E2E_DEBUG").is_ok() {
                eprintln!("<- {msg:?}");
            }
            match msg {
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
    let d = loop {
        let d = c.wait_notification("textDocument/publishDiagnostics");
        if d["version"] == 2 {
            break d;
        }
    };
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
    // diagnostics may be republished several times (re-indexing); wait for the new result
    while c.wait_notification("textDocument/publishDiagnostics")["diagnostics"] != json!([]) {}
    c.shutdown();
}

#[test]
fn indexes_workspace_and_navigates() {
    let dir = std::env::temp_dir().join(format!("php-lsp-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/Models")).unwrap();
    std::fs::create_dir_all(dir.join("vendor/acme/lib")).unwrap();
    std::fs::write(
        dir.join("src/Models/User.php"),
        "<?php\nnamespace App\\Models;\n\n/** A user. */\nclass User\n{\n    public function posts(): array { return []; }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("vendor/acme/lib/Helper.php"),
        "<?php\nnamespace Acme;\nfunction help(): int { return 1; }\n",
    )
    .unwrap();
    let root_uri = format!("file://{}", dir.display());
    let mut c = Client::start_with(json!({
        "rootUri": root_uri,
        "capabilities": {"window": {"workDoneProgress": true}},
        "initializationOptions": {"storagePath": dir.join(".storage").display().to_string()},
    }));
    c.wait_notification("indexingEnded");

    let uri = format!("{root_uri}/src/main.php");
    let text =
        "<?php\nuse App\\Models\\User;\n$u = new User();\n$u->posts();\n$n = strlen('x');\n\\Acme\\help();\n";
    c.open(&uri, text);
    c.wait_notification("textDocument/publishDiagnostics");

    // definition in another (unopened) file
    let def = c.request(
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": {"line": 3, "character": 6}}),
    );
    assert!(def[0]["uri"].as_str().unwrap().ends_with("src/Models/User.php"), "{def}");
    assert_eq!(def[0]["range"]["start"]["line"], 6);

    // stubs: hover and definition (materialized on disk)
    let hover = c.request(
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": {"line": 4, "character": 7}}),
    );
    assert!(hover["contents"]["value"].as_str().unwrap().contains("function strlen"), "{hover}");
    let def = c.request(
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": {"line": 4, "character": 7}}),
    );
    let stub_uri = def[0]["uri"].as_str().unwrap().to_string();
    assert!(stub_uri.starts_with("file://") && stub_uri.contains("php-lsp-stubs"), "{stub_uri}");
    let path = php_lsp::uri::path_from_uri(&stub_uri).unwrap();
    assert!(std::fs::read_to_string(path).unwrap().contains("function strlen"));

    // vendor files are indexed
    let hover = c.request(
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": {"line": 5, "character": 7}}),
    );
    assert!(hover["contents"]["value"].as_str().unwrap().contains("function help(): int"), "{hover}");

    // member completion
    let comp = c.request(
        "textDocument/completion",
        json!({"textDocument": {"uri": uri}, "position": {"line": 3, "character": 4}}),
    );
    let labels: Vec<&str> =
        comp["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"posts"), "{labels:?}");

    // references to User across files
    let refs = c.request(
        "textDocument/references",
        json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 10}, "context": {"includeDeclaration": true}}),
    );
    assert_eq!(refs.as_array().unwrap().len(), 3, "{refs}");

    // workspace symbols
    let syms = c.request("workspace/symbol", json!({"query": "User"}));
    assert_eq!(syms[0]["name"], "User");
    assert_eq!(syms[0]["containerName"], "App\\Models");

    // signature help
    c.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": "<?php\nstr_replace('a', \n"}]}),
    );
    let sh = c.request(
        "textDocument/signatureHelp",
        json!({"textDocument": {"uri": uri}, "position": {"line": 1, "character": 17}}),
    );
    assert_eq!(sh["activeParameter"], 1, "{sh}");
    assert!(sh["signatures"][0]["label"].as_str().unwrap().starts_with("str_replace("));

    c.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn editing_features_over_lsp() {
    let mut c = Client::start(json!({}));
    let uri = "file:///edit.php";
    c.open(uri, "<?php\nclass Greeter{\npublic function hi($name){return 'hi '.$name;}\n}\n$g=new Greeter();\n$g->hi('x');\n");
    c.wait_notification("textDocument/publishDiagnostics");

    let edits = c.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}}),
    );
    assert!(!edits.as_array().unwrap().is_empty());

    let prep = c.request(
        "textDocument/prepareRename",
        json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 17}}),
    );
    assert_eq!(prep["placeholder"], "hi");
    let edit = c.request(
        "textDocument/rename",
        json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 17}, "newName": "greet"}),
    );
    assert_eq!(edit["changes"][uri].as_array().unwrap().len(), 2, "{edit}");

    let actions = c.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri}, "range": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 7}}, "context": {"diagnostics": []}}),
    );
    let titles: Vec<&str> =
        actions.as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Add PHPDoc"), "{titles:?}");

    let lenses = c.request("textDocument/codeLens", json!({"textDocument": {"uri": uri}}));
    let resolved = c.request("codeLens/resolve", lenses[0].clone());
    assert_eq!(resolved["command"]["title"], "1 reference");

    let hints = c.request(
        "textDocument/inlayHint",
        json!({"textDocument": {"uri": uri}, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}}),
    );
    assert!(hints.as_array().unwrap().iter().any(|h| h["label"] == "name:"), "{hints}");
    let toks = c.request("textDocument/semanticTokens/full", json!({"textDocument": {"uri": uri}}));
    assert!(!toks["data"].as_array().unwrap().is_empty());
    c.shutdown();
}

/// Needs `PHP_LSP_TOOLS_DIR` with phpstan.phar and php-cs-fixer.phar.
#[test]
#[ignore]
fn external_tools() {
    let tools = std::env::var("PHP_LSP_TOOLS_DIR").expect("PHP_LSP_TOOLS_DIR");
    let dir = std::env::temp_dir().join(format!("php-lsp-tools-e2e-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let file = dir.join("src/a.php");
    let text = "<?php\nnamespace App;\nclass A {\n  public function f(int $x): string { return $x; }\n}\n";
    std::fs::write(&file, text).unwrap();
    let root_uri = format!("file://{}", dir.display());
    let mut c = Client::start_with(json!({
        "rootUri": root_uri,
        "capabilities": {},
        "initializationOptions": {"php-lsp": {
            "tools": {
                "phpstan": {"enable": true, "path": format!("{tools}/phpstan.phar"), "args": ["--level=5"]},
                "phpCsFixer": {"path": format!("{tools}/php-cs-fixer.phar")}
            },
            "format": {"provider": "php-cs-fixer"}
        }},
    }));
    let uri = format!("{root_uri}/src/a.php");
    c.open(&uri, text);
    let d = loop {
        let d = c.wait_notification("textDocument/publishDiagnostics");
        if d["diagnostics"].as_array().unwrap().iter().any(|x| x["source"] == "phpstan") {
            break d;
        }
    };
    let stan =
        d["diagnostics"].as_array().unwrap().iter().find(|x| x["source"] == "phpstan").unwrap().clone();
    assert!(stan["message"].as_str().unwrap().contains("should return string"), "{stan}");
    assert_eq!(stan["range"]["start"]["line"], 3);
    let edits = c.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}}),
    );
    assert!(edits[0]["newText"].as_str().unwrap().contains("class A\n{"), "{edits}");
    c.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
