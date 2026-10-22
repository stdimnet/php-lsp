# Configuration

## Where settings come from

The server reads settings from three places, later ones win:

1. `initializationOptions` of the `initialize` request (the Intelephense
   options listed [below](#initialization-options), plus any settings),
2. `workspace/configuration` (asked for the sections `intelephense` and
   `php-lsp` when the client supports it),
3. `workspace/didChangeConfiguration`.

Two sections are read: `intelephense` and `php-lsp`. When both set the same
key, `php-lsp` wins, so an existing Intelephense configuration keeps working
and can be overridden selectively. Keys can be nested
(`{"php-lsp": {"format": {"braces": "allman"}}}`) or dotted
(`{"php-lsp.format.braces": "allman"}`). Unknown keys are ignored.

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `environment.phpVersion` | `8.4` | PHP version the code targets (`5.3` to `8.5`, also `7.4.3`). Controls syntax gating, version dependent diagnostics and deprecations |
| `environment.includePaths` | `[]` | Extra directories indexed like `vendor/` (absolute or relative to the workspace) |
| `files.associations` | `["*.php", "*.phtml"]` | Files treated as PHP. `*.blade.php` is always Blade, `*.twig` is Twig |
| `files.exclude` | `**/.git/**`, `**/.svn/**`, `**/.hg/**`, `**/CVS/**`, `**/.DS_Store/**`, `**/node_modules/**`, `**/bower_components/**`, `**/vendor/**/{Tests,tests}/**`, `**/.history/**`, `**/vendor/**/vendor/**` | Globs not indexed |
| `files.maxSize` | `1000000` | Files larger than this (bytes) are not indexed |
| `stubs` | Intelephense's default list (`Core`, `standard`, `date`, `json`, `pcre`, `SPL`, `mbstring`, `PDO`, ...) | phpstorm-stubs extensions to load; add e.g. `redis`, `swoole`, `wordpress` |
| `completion.insertUseDeclaration` | `true` | Completing a class from another namespace adds the `use` |
| `completion.maxItems` | `100` | Maximum number of completion items |
| `diagnostics.enable` | `true` | All diagnostics on / off |
| `diagnostics.run` | `onType` | `onSave`: publish only when a file is saved |
| `diagnostics.<group>` | `true` | Switch a group off; `"on"` / `"off"` also work. Groups below |
| `inlayHint.parameterNames` | `true` | `foo(name: $x)` hints |
| `inlayHint.parameterTypes` | `true` | Types of untyped closure / arrow function parameters |
| `inlayHint.returnTypes` | `true` | Inferred return types of functions, methods, closures and arrow functions without a declared one |
| `format.enable` | `true` | Answer formatting requests |
| `format.braces` | `psr12` | `psr12`, `allman` (all braces on their own line) or `k&r` (all braces on the same line) |
| `format.html` | `true` | Also format the HTML, CSS and JavaScript of templates (see [formatting](formatting.md)) |
| `format.provider` | `builtin` | `php-cs-fixer` or `phpcbf`: format with an external tool instead |
| `index.cache` | `true` | Keep the extracted symbols on disk so a restart only parses changed files |
| `tools.<tool>.enable` | `false` | Run an external tool (see [external tools](#external-tools)) |
| `tools.<tool>.path` | `vendor/bin/<tool>`, else `PATH` | Executable |
| `tools.<tool>.args` | `[]` | Extra arguments |

Diagnostic groups (`diagnostics.<group>`): `syntax`, `undefinedTypes`,
`undefinedFunctions`, `undefinedConstants`, `undefinedClassConstants`,
`undefinedMethods`, `undefinedProperties`, `undefinedVariables`,
`unusedSymbols`, `deprecated`, `argumentCount`, `implementationErrors`,
`typeErrors`, `unreachableCode`, `languageConstraints`, `memberAccess`,
`suspectCode`, `duplicateSymbols`. The codes are listed in
[diagnostics](diagnostics.md).

Intelephense settings that php-lsp does not need are accepted and ignored
(`licenceKey`, `telemetry.*`, `trace.*`, `maxMemory`, ...).

### Example (VS Code `settings.json`)

```json
{
  "intelephense.environment.phpVersion": "8.3",
  "intelephense.diagnostics.undefinedProperties": false,
  "intelephense.stubs": ["Core", "standard", "pcre", "SPL", "json", "mbstring", "redis"],
  "php-lsp.format.braces": "allman",
  "php-lsp.tools.phpstan.enable": true,
  "php-lsp.tools.phpstan.args": ["--level=6"]
}
```

### Example (Neovim)

```lua
settings = {
  intelephense = { files = { maxSize = 5000000 } },
  ['php-lsp'] = { tools = { phpstan = { enable = true } }, index = { cache = false } },
}
```

## Initialization options

Intelephense's `initializationOptions` are accepted:

| Option | Meaning |
|---|---|
| `storagePath` | Directory for the index cache (`<storagePath>/php-lsp-index`) and the unpacked stubs |
| `globalStoragePath` | Accepted, not used |
| `clearCache` | `true`: discard the index cache and index from scratch |
| `licenceKey` | Ignored; every feature works without one |
| `isVscode` | Enables VS Code commands in results (trigger signature help after completing a function, editor commands for code lenses) |

Without a storage path the cache lives in `$XDG_CACHE_HOME/php-lsp`, else
`~/.cache/php-lsp`, else the temporary directory. Any other key in
`initializationOptions` is read like an `intelephense` setting.

## Command line

```text
php-lsp [--stdio | --socket=<port> | --node-ipc]
```

| Flag | Meaning |
|---|---|
| `--stdio` | LSP over stdin / stdout (the default) |
| `--socket=<port>` | Connect to a client listening on `127.0.0.1:<port>` |
| `--node-ipc` | Node.js IPC channel (fd 3), as used by VS Code extensions |
| `--version`, `-V` | Print the version |
| `--help`, `-h` | Usage |

Unknown flags (`--clientProcessId=...`) are ignored. The server logs with
`window/logMessage`; only fatal startup errors go to stderr.

The Node.js launcher (`node/php-lsp.js`) finds the binary through
`PHP_LSP_BIN`, then `bin/php-lsp` next to the launcher, then `PATH`.

## Index cache

At startup the server indexes the workspace folders, `vendor/` (via
Composer's `autoload_*` files and PSR-4 mappings), `environment.includePaths`
and the selected stubs in the background. With `index.cache` the symbols of
each file are stored with the file's size and modification time; the next
start only parses changed files (12.6k files: 0.85 s cold, 0.2 s cached).
The command "Index workspace" (`php-lsp.indexWorkspace`, or Intelephense's
`intelephense/index`) and the `clearCache` option rebuild it.

## External tools

Tools run in the background after a file is opened or saved; their findings
are merged with php-lsp's diagnostics (source `phpstan`, `psalm`, `phpcs`).

| Tool key | Used for |
|---|---|
| `phpstan` | diagnostics |
| `psalm` | diagnostics |
| `phpcs` | diagnostics |
| `phpCsFixer` | formatting with `format.provider: "php-cs-fixer"` |
| `phpcbf` | formatting with `format.provider: "phpcbf"` |
| `phpunit` | the "▶ Run test" code lenses (`php-lsp.runTest`) |

Each looks for `vendor/bin/<tool>` in the project first, then the `PATH`.
The tools' own configuration files (`phpstan.neon`, `psalm.xml`,
`phpcs.xml`, `.php-cs-fixer.php`, `phpunit.xml`) are used as usual.
