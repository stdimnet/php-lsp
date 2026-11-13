# Rust library

Two crates are meant for embedding:

| Crate | What you get |
|---|---|
| `php-ide` | The IDE features as a plain Rust API (`Workspace`): no JSON, no LSP, byte offsets in, typed results out |
| `php-lsp` | The complete language server: `Server` on any `lsp_server::Connection`, `Embedded` for JSON messages in process, `format_document` |

```toml
[dependencies]
php-ide = { git = "https://github.com/stdimnet/php-lsp", features = ["parallel"] }
php-lsp = { git = "https://github.com/stdimnet/php-lsp" }
```

Feature `parallel` (default): parallel indexing and reference search with rayon. Turn it off
(`default-features = false`) for `wasm32` or single-threaded hosts.

The lower crates can be used alone too: `php-syntax` (lossless parser,
`php_syntax::parse`), `php-hir` (names, PHPDoc, types), `php-index` (symbol
index), `php-fmt` (formatter, `php_fmt::format`), `php-stubs`,
`php-tools`. API documentation: `cargo doc --workspace --no-deps --open`.

## `php_ide::Workspace`

```rust
use php_ide::{Config, FileKind, FormatOptions, LineCol, Workspace};

let mut ws = Workspace::new(Config::default());
ws.load_stubs();                                           // PHP's built-ins (phpstorm-stubs)
ws.index_files(vec![(                                      // files that are not open
    "file:///p/src/User.php".into(),
    "<?php\nnamespace App;\nclass User { public function name(): string { return ''; } }\n".into(),
    FileKind::Workspace,
)]);
let id = ws.set_file("file:///p/index.php", "<?php\n$u = new App\\User();\n$u->name();\n".into());

let lines = ws.line_index(id).unwrap();
let offset = lines.offset(LineCol { line: 2, col: 5 });
let hover = ws.hover(id, offset);                          // Option<Hover> (Markdown)
let defs = ws.goto_definition(id, offset);                 // Vec<Location>
let items = ws.completions(id, lines.offset(LineCol { line: 2, col: 4 }));
let diagnostics = ws.diagnostics(id);                      // Vec<Diagnostic>
let edits = ws.format(id, None, &FormatOptions::default()); // Option<Vec<TextEdit>>
```

Runnable: `cargo run -p php-ide --example embed`
([`crates/php-ide/examples/embed.rs`](../crates/php-ide/examples/embed.rs)).

Concepts:

- **Files** are keyed by a URI-like string (`file:///...`). `set_file`
  opens or updates a document (full text) and returns its `FileId`;
  `close_file`, `remove_file`, `file`, `files`, `file_id`.
- **Kinds**: `FileKind::Workspace` (project code, diagnostics and
  references), `Vendor` (dependencies: only symbols), `Stub`.
- **Indexing**: `index_files` parses in parallel and adds the symbols;
  `php_ide::collect_files(root, &ScanConfig)` lists the PHP files of a
  directory with the `files.*` rules; `php_ide::composer` reads
  `composer.json` / `vendor/composer/autoload_*`. `IndexCache` stores the
  extracted symbols on disk.
- **Offsets** are byte offsets (`TextSize`). `LineIndex` converts to and
  from `LineCol` (line and byte column); `to_encoding` / `from_encoding`
  convert columns to LSP positions (`PositionEncoding` UTF-8 / UTF-16 /
  UTF-32).
- **Configuration**: `Config` (PHP version, `DiagnosticsConfig`, stubs,
  auto import, completion limit, `InlayHintsConfig`); `set_config` changes
  it later.
- Queries take `&self`; changes (`set_file`, `index_files`, ...) take
  `&mut self`. `php_ide::index_files` (the free function) parses files
  without a workspace, e.g. on a background thread, and
  `Workspace::add_indexed` adds the result.

Methods (all take a `FileId` and usually a `TextSize`):

| Area | Methods |
|---|---|
| Analysis | `diagnostics`, `type_at`, `syntax_tree`, `parse` |
| Navigation | `hover`, `goto_definition`, `goto_type_definition`, `goto_implementation`, `references`, `document_highlights`, `definition_at`, `string_targets` |
| Editing | `completions`, `signature_help`, `prepare_rename`, `rename` (returns a `SourceChange`), `code_actions`, `format`, `file_rename_edits`, `linked_editing_ranges` |
| Structure | `document_symbols`, `workspace_symbols`, `folding_ranges`, `selection_ranges`, `document_links`, `semantic_tokens`, `inlay_hints` |
| Hierarchies | `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`, `prepare_type_hierarchy`, `supertypes`, `subtypes`, `code_lenses`, `resolve_code_lens` |
| Extension | `register_plugin` ([plugins](plugins.md)) |

## `php_lsp::Server`

The language server on your own transport:

```rust
let (connection, io_threads) = lsp_server::Connection::stdio();   // or ::listen / ::connect / ::memory
let server = php_lsp::Server::initialize(connection)?;          // LSP handshake
// server.register_plugin(...) here for your own plugins
server.run()?;                                                   // until "exit"
io_threads.join()?;
```

`php_lsp::run_stdio()`, `run_socket(port)` and `run_node_ipc()` do this for
the standard transports (the binary is a thin `main` around them).

## Stack size

Deeply nested code (long `.` chains in generated files) is walked
recursively. `run_stdio` / `run_socket` / `run_node_ipc` and the threads
php-lsp starts use `php_ide::STACK_SIZE` (256 MB reserved, used only as
needed). Call `Workspace`, `Server::run` or `Embedded` from a thread with
such a stack too, not from the main thread (1 MB on Windows):

```rust
std::thread::Builder::new()
    .stack_size(php_ide::STACK_SIZE)
    .spawn(move || { /* use the workspace / server here */ })?;
```

## `php_lsp::Embedded`

The server without any transport, driven with JSON strings (this is what
the C library and the WebAssembly package wrap):

```rust
use php_lsp::Embedded;
use serde_json::json;

let (mut server, capabilities) = Embedded::new(json!({ "capabilities": {} }))?;
server.send(&json!({
    "jsonrpc": "2.0", "method": "textDocument/didOpen",
    "params": { "textDocument": {
        "uri": "file:///a.php", "languageId": "php", "version": 1,
        "text": "<?php\nfunction greet() {}\ngreet();\n" } }
}).to_string())?;
server.send(&json!({
    "jsonrpc": "2.0", "id": 1, "method": "textDocument/hover",
    "params": { "textDocument": { "uri": "file:///a.php" }, "position": { "line": 2, "character": 1 } }
}).to_string())?;
for message in server.receive() {
    println!("{message}");    // the hover response, publishDiagnostics, ...
}
```

| Method | |
|---|---|
| `Embedded::new(params)` | `initialize` + `initialized`; returns the server and the `initialize` result |
| `send(&str)` / `send_message(Message)` | handle one message; `true` after `exit` |
| `receive()` | messages sent since the last call (after applying finished background work) |
| `wait(Duration)` | like `receive`, waiting for background work when nothing is queued |
| `exited()` | `exit` was received |
| `workspace()` | the underlying `php_ide::Workspace` |
| `register_plugin(plugin)` | add a [plugin](plugins.md) |

## Version

`php_lsp::VERSION` and `php_ide::VERSION` hold the crates' version (the
same as `serverInfo.version` and `php-lsp --version`).

## `php_lsp::format_document`

```rust
let formatted: Option<String> =
    php_lsp::format_document("<?php\nif($a){echo 1;}\n", "a.php", &json!({ "tabSize": 2 }));
```

See [formatting](formatting.md#without-a-server) for the options.
