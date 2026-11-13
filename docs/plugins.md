# Plugins

Framework support (Laravel, Symfony, WordPress) is written as plugins of
`php-ide`. A plugin answers a few questions the type inference and the
completion ask; everything else (hover, go to definition, diagnostics,
completion of members) follows automatically from those answers.

Plugins are Rust code registered on a `php_ide::Workspace`:

```rust
ws.register_plugin(std::sync::Arc::new(MyPlugin));
```

The built-in ones live in `crates/php-ide/src/plugins/` (`laravel.rs`,
`symfony.rs`, `wordpress.rs`) and are good references.

## The trait

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Overrides the return type of a function / method call.
    fn call_return_type(&self, cx: &PluginCx, call: &CallInfo) -> Option<Type> { None }

    /// Type of a property that is not declared (magic properties).
    fn property_type(&self, cx: &PluginCx, class: &Fqn, name: &str) -> Option<Type> { None }

    /// Possible values of a string argument.
    fn string_candidates(&self, cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> { Vec::new() }

    /// Type of a variable a template (`cx.file`) receives from its caller.
    fn template_variable_type(&self, cx: &PluginCx, name: &str) -> Option<Type> { None }
}
```

| Hook | Used for | Example |
|---|---|---|
| `call_return_type` | inferred type of a call; drives member completion, hover, diagnostics, go to definition on the result | `app(Mailer::class)` is a `Mailer`, `$em->getRepository(User::class)` is a `UserRepository` |
| `property_type` | type of an undeclared property | Eloquent relation methods as properties: `$user->posts` |
| `string_candidates` | completion inside a string argument; with `location` also go to definition, hover and document links of that string | route names, view names, config keys, hook names |
| `template_variable_type` | variables of a template file (`cx.file`) passed by its callers | Blade views get the variables of `view('x', [...])` |

Inputs:

- `CallInfo { callee, args }`: `Callee::Function(Fqn)` or
  `Callee::Method { class, name, is_static }` (the class is the resolved
  type of the receiver); `callee.is_function("app")` compares
  case-insensitively. Arguments are `ArgInfo::ClassString(Fqn)` for
  `Foo::class`, `ArgInfo::String(s)` for string literals, `ArgInfo::Other`.
- `StringArg { callee, index, value }`: the string literal the cursor is
  in and its argument position.
- `PluginCx { ws, file }`: the workspace (index, files) and the file being
  analysed; `project_root()` (the directory with `composer.json`),
  `is_subtype(class, "Illuminate\\Database\\Eloquent\\Model")`,
  `files_under(prefix)`.

Outputs are `php_ide::hir::Type` (e.g. `Type::Named(Fqn::new("App\\Mailer"),
vec![])`, generics as the second field) and `StringCandidate { value,
detail, kind, location: Option<(file_key, line)> }`.

## Example

[`crates/php-ide/examples/plugin.rs`](../crates/php-ide/examples/plugin.rs)
(`cargo run -p php-ide --example plugin`) types `container('mailer')` and
completes the service names:

```rust
struct Services;

const SERVICES: &[(&str, &str)] = &[("mailer", "App\\Mailer"), ("logger", "App\\Logger")];

impl Plugin for Services {
    fn name(&self) -> &'static str {
        "services"
    }

    fn call_return_type(&self, _cx: &PluginCx, call: &CallInfo) -> Option<Type> {
        if !call.callee.is_function("container") {
            return None;
        }
        let ArgInfo::String(name) = call.args.first()? else { return None };
        let class = SERVICES.iter().find(|(n, _)| n == name)?.1;
        Some(Type::Named(Fqn::new(class), Vec::new()))
    }

    fn string_candidates(&self, _cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> {
        if !arg.callee.is_function("container") || arg.index != 0 {
            return Vec::new();
        }
        SERVICES
            .iter()
            .map(|(name, class)| StringCandidate {
                value: name.to_string(),
                detail: Some(class.to_string()),
                kind: CompletionKind::Constant,
                location: None,
            })
            .collect()
    }
}
```

## Guidelines

- Hooks are called often (every inference of a call). Keep them cheap:
  return early on the callee name, and cache data computed from the
  workspace with `php_ide::plugins::Cached<T>`, keyed by
  `cx.ws.revision()` (it changes whenever a file changes).
- Read project files through `cx.ws` (`files_under`, `file`), not from
  disk, so unsaved editor contents are seen.
- Return `None` / an empty list when unsure; the next plugin or the normal
  inference takes over.

## Using a plugin in the language server

The server registers the built-in plugins. Your own are added by embedding
the server:

```rust
let (connection, io_threads) = lsp_server::Connection::stdio();
let mut server = php_lsp::Server::initialize(connection)?;
server.register_plugin(std::sync::Arc::new(Services));
server.run()?;
io_threads.join()?;
```

`php_lsp::Embedded::register_plugin` does the same for the in-process
server. Plugins of general interest are welcome in
`crates/php-ide/src/plugins/` (registered in `Plugins::default()`).
