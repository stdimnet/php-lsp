# LSP reference

## Transport and protocol

- JSON-RPC over stdio, a TCP socket or Node IPC
  ([command line](configuration.md#command-line)); in process via the
  libraries.
- Text sync: incremental (`TextDocumentSyncKind.Incremental`), `didSave`.
- Position encoding: UTF-8 when the client offers it
  (`general.positionEncodings`), UTF-32 when offered without UTF-16,
  else UTF-16.
- Workspace folders with change notifications; multiple roots.
- Progress: indexing reports `$/progress` (token `php-lsp/indexing`) when
  the client supports `window.workDoneProgress`.
- Requests are handled one after another; `$/cancelRequest` is accepted and
  ignored.

## Requests

| Method | |
|---|---|
| `textDocument/hover` | |
| `textDocument/completion` | trigger characters `$ > : \ @ # [ < /` |
| `textDocument/signatureHelp` | trigger characters `( ,` |
| `textDocument/definition`, `textDocument/declaration` | |
| `textDocument/typeDefinition`, `textDocument/implementation` | |
| `textDocument/references`, `textDocument/documentHighlight` | |
| `textDocument/documentSymbol`, `workspace/symbol` | |
| `textDocument/foldingRange`, `textDocument/selectionRange` | |
| `textDocument/prepareRename`, `textDocument/rename` | |
| `workspace/willRenameFiles` | `*.php` files and folders: renames the class and updates the PSR-4 namespace |
| `textDocument/codeAction` | quick fixes, refactorings, `source.organizeImports` |
| `textDocument/codeLens`, `codeLens/resolve` | |
| `textDocument/formatting`, `textDocument/rangeFormatting` | |
| `textDocument/semanticTokens/full`, `textDocument/semanticTokens/range` | |
| `textDocument/inlayHint` | |
| `textDocument/documentLink` | |
| `textDocument/linkedEditingRange` | HTML tag pairs |
| `textDocument/prepareCallHierarchy`, `callHierarchy/incomingCalls`, `callHierarchy/outgoingCalls` | |
| `textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes` | |
| `workspace/executeCommand` | commands below |
| `shutdown` | |

## Notifications from the client

`initialized`, `exit`, `textDocument/didOpen`, `didChange`, `didClose`,
`didSave`, `workspace/didChangeConfiguration`,
`workspace/didChangeWatchedFiles`, `workspace/didChangeWorkspaceFolders`.

## Requests to the client

| Method | When |
|---|---|
| `workspace/configuration` | after `initialized` and on `didChangeConfiguration` (sections `intelephense`, `php-lsp`) |
| `client/registerCapability` | file watchers for the `files.associations` patterns (`**/*.php`, ...) when dynamic registration is supported |
| `window/workDoneProgress/create` | before indexing |

The server also sends `textDocument/publishDiagnostics`,
`window/logMessage` and `window/showMessage` (PHPUnit results).

## Commands

| Command | Arguments | |
|---|---|---|
| `php-lsp.runTest` | `[uri, filter?]` | run PHPUnit for a file / `Class::method` (the "▶ Run test" lens); result as `window/showMessage`, output as `window/logMessage` |
| `php-lsp.indexWorkspace` | none | discard the index and the cache, index again |

## Custom requests

| Method | Params | Result |
|---|---|---|
| `php-lsp/syntaxTree` | `{ textDocument }` | the syntax tree as text (debugging) |
| `cancelIndexing` | none | `null`; stops a running indexing job (Intelephense) |

## Custom notifications

From the client:

| Method | Params | |
|---|---|---|
| `php/indexFiles` | `{ files: [{ uri, text }] }` | add or update project files without a file system (WebAssembly, virtual workspaces) |
| `indexWorkspace`, `intelephense/index` | none | re-index (Intelephense's names) |

From the server (Intelephense compatible, used by its VS Code extension
for the status bar):

| Method | |
|---|---|
| `indexingStarted` | indexing begins |
| `indexingEnded` | indexing finished |
