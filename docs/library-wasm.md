# WebAssembly / npm

`php-lsp-wasm` is the language server compiled to WebAssembly for Node.js,
browsers and web workers. Same features as the binary (the PHP stubs are
embedded), no native code, no file system access. Size: about 6.5 MB
(`.wasm`, 2.9 MB gzipped).

```sh
npm install php-lsp-wasm
```

Or build it: `crates/php-lsp-wasm/build.sh` (needs the
`wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.100) writes the
package to `crates/php-lsp-wasm/pkg`.

## Entry points

| Import | For |
|---|---|
| `php-lsp-wasm` | Node.js (CommonJS / ESM, loads the `.wasm` synchronously) and bundlers (webpack, Vite, Rollup with wasm support) |
| `php-lsp-wasm/web` | Browsers without a bundler: `import init, { LanguageServer } from "php-lsp-wasm/web"; await init();` (optionally `init(urlOfTheWasm)`) |

TypeScript declarations are included.

## API

```ts
class LanguageServer {
  constructor(initializeParams: string);   // JSON of the LSP initialize params
  readonly capabilities: string;           // JSON of the initialize result
  send(message: string): boolean;          // one JSON-RPC message; true after "exit"
  receive(): string;                       // JSON array of the server's messages since the last call
  free(): void;                            // release the memory (wasm-bindgen)
}
function format(text: string, fileName: string, options: string): string | undefined;
function version(): string;
```

- `send` throws on invalid JSON. Requests are answered synchronously: the
  response is in the next `receive()`.
- `receive()` also returns notifications: `textDocument/publishDiagnostics`,
  `window/logMessage`, progress, and requests to the client (e.g.
  `workspace/configuration`, `client/registerCapability`). Answer requests
  with a JSON-RPC response via `send` or ignore them.
- Everything runs on the calling thread; there is no background indexing.
  Run it in a web worker to keep a UI responsive.
- `format()` formats without a server; options and `fileName` as in
  [formatting](formatting.md#without-a-server).

## Project files

There is no file system, so `rootUri` is not scanned. Open documents come
with `textDocument/didOpen`; the other files of a project are passed with
the notification `php/indexFiles`:

```js
server.send(JSON.stringify({
  jsonrpc: "2.0", method: "php/indexFiles",
  params: { files: [
    { uri: "file:///project/src/User.php", text: userPhp },
    { uri: "file:///project/vendor/acme/lib/src/Client.php", text: clientPhp },
  ] },
}));
```

Send it again for changed files; `workspace/didChangeWatchedFiles` with
type `3` (deleted) removes a file.

## Node.js example

```js
import { LanguageServer } from "php-lsp-wasm";

const server = new LanguageServer(JSON.stringify({
  capabilities: {},
  initializationOptions: { "php-lsp": { environment: { phpVersion: "8.3" } } },
}));
let id = 0;
const send = (m) => server.send(JSON.stringify({ jsonrpc: "2.0", ...m }));
const request = (method, params) => {
  send({ id: ++id, method, params });
  return JSON.parse(server.receive()).find((m) => m.id === id && !m.method)?.result;
};

send({ method: "textDocument/didOpen", params: { textDocument: {
  uri: "file:///a.php", languageId: "php", version: 1, text: "<?php\n$d = new DateTime();\n$d->" } } });
const completion = request("textDocument/completion", {
  textDocument: { uri: "file:///a.php" }, position: { line: 2, character: 4 } });
console.log(completion.items.map((i) => i.label));
```

`crates/php-lsp-wasm/tests/node.mjs` (run in CI) covers indexing,
diagnostics, definition, hover, completion and formatting.

## Browser / Monaco

Run the server in a worker and bridge it to any LSP client, for example
[monaco-languageclient](https://github.com/TypeFox/monaco-languageclient)
with a `BrowserMessageReader` / `BrowserMessageWriter`:

```js
// php-worker.js
import init, { LanguageServer } from "php-lsp-wasm/web";

await init();
let server;
self.onmessage = ({ data }) => {
  if (!server && data.method === "initialize") {
    server = new LanguageServer(JSON.stringify(data.params));
    self.postMessage({ jsonrpc: "2.0", id: data.id, result: JSON.parse(server.capabilities) });
    return;
  }
  if (data.method === "initialized") return;   // implied by the constructor
  server.send(JSON.stringify(data));
  for (const message of JSON.parse(server.receive())) self.postMessage(message);
};
```

```js
// main thread
const worker = new Worker(new URL("./php-worker.js", import.meta.url), { type: "module" });
// new MonacoLanguageClient({ ..., connection: { reader: new BrowserMessageReader(worker),
//                                               writer: new BrowserMessageWriter(worker) } })
```

The constructor performs the `initialize` / `initialized` handshake
itself, so the worker answers the client's `initialize` request with the
stored capabilities.
