// Smoke test of the Node.js build: node tests/node.mjs [pkg dir]
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import path from "node:path";

const require = createRequire(import.meta.url);
const pkg = path.resolve(process.argv[2] || new URL("../pkg", import.meta.url).pathname);
const { LanguageServer, format, version } = require(path.join(pkg, "node/php_lsp_wasm.js"));

const server = new LanguageServer(JSON.stringify({ capabilities: {} }));
assert.ok(JSON.parse(server.capabilities).capabilities.hoverProvider);
const send = (m) => server.send(JSON.stringify({ jsonrpc: "2.0", ...m }));
const response = (id) => JSON.parse(server.receive()).find((m) => m.id === id && !m.method);

send({ method: "php/indexFiles", params: { files: [{ uri: "file:///src/User.php", text: "<?php\nnamespace App;\nclass User { public function name(): string { return ''; } }\n" }] } });
send({ method: "textDocument/didOpen", params: { textDocument: { uri: "file:///main.php", languageId: "php", version: 1,
  text: "<?php\nuse App\\User;\n$u = new User();\n$u->name();\necho strlen(1, 2);\n" } } });
const diagnostics = JSON.parse(server.receive()).find((m) => m.method === "textDocument/publishDiagnostics");
assert.ok(diagnostics, "diagnostics published");

send({ id: 1, method: "textDocument/definition", params: { textDocument: { uri: "file:///main.php" }, position: { line: 3, character: 5 } } });
const def = response(1).result;
assert.equal((Array.isArray(def) ? def[0] : def).uri, "file:///src/User.php");

send({ id: 2, method: "textDocument/hover", params: { textDocument: { uri: "file:///main.php" }, position: { line: 4, character: 7 } } });
assert.match(response(2).result.contents.value, /strlen/);

send({ id: 3, method: "textDocument/completion", params: { textDocument: { uri: "file:///main.php" }, position: { line: 3, character: 4 } } });
const items = response(3).result;
assert.ok((items.items || items).some((i) => i.label === "name"));

assert.equal(format("<?php\nif($a){echo 1;}\n", "a.php", "{}"), "<?php\nif ($a) {\n    echo 1;\n}\n");
assert.equal(format("<div>\n<p>x</p>\n</div>\n", "v.blade.php", "{}"), "<div>\n    <p>x</p>\n</div>");
console.log(`php-lsp-wasm ${version()}: ok`);
