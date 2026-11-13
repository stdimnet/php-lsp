# php-lsp-wasm

The [php-lsp](https://github.com/stdimnet/php-lsp) PHP language server compiled
to WebAssembly: completion, hover, diagnostics, go to definition, references,
rename, code actions, formatting (PHP, HTML templates, Blade) and more, in
Node.js, browsers and web workers. It speaks the Language Server Protocol;
messages are JSON strings.

```js
// Node.js / bundlers
import { LanguageServer, format } from "php-lsp-wasm";
// browsers without a bundler: import init, { ... } from "php-lsp-wasm/web"; await init();

const server = new LanguageServer(JSON.stringify({ capabilities: {} }));
server.send(JSON.stringify({
  jsonrpc: "2.0", method: "textDocument/didOpen",
  params: { textDocument: { uri: "file:///a.php", languageId: "php", version: 1, text: "<?php\necho strlen('x');\n" } },
}));
server.send(JSON.stringify({
  jsonrpc: "2.0", id: 1, method: "textDocument/hover",
  params: { textDocument: { uri: "file:///a.php" }, position: { line: 1, character: 6 } },
}));
for (const message of JSON.parse(server.receive())) console.log(message);

console.log(format("<?php\nif($a){echo 1;}\n", "a.php", "{}"));
```

Project files that are not open can be indexed with the notification
`php/indexFiles` (`{"files": [{"uri": "...", "text": "..."}]}`).
See the [documentation](https://github.com/stdimnet/php-lsp/tree/main/docs).
