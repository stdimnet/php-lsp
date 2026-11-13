# Features

Everything Intelephense offers, its premium features included, works
without a licence key. Features marked *extra* are not in Intelephense.

## Code intelligence

| Feature | LSP request | Notes |
|---|---|---|
| Diagnostics | `textDocument/publishDiagnostics` | Intelephense's 131 codes, messages and severities ([list](diagnostics.md)); PHPStan, Psalm, PHPCS optionally merged in |
| Hover | `textDocument/hover` | Signature, PHPDoc (Markdown), inferred types of variables, constant values |
| Completion | `textDocument/completion` | Classes, functions, constants (with auto import), members after `->` / `::` / `?->`, variables, keywords, named arguments, `match` / array keys of shapes, PHPDoc tags and types, `@param` / `@return` skeletons after `/**`, string candidates of framework plugins, HTML tags / attributes in templates |
| Signature help | `textDocument/signatureHelp` | Active parameter, named arguments |
| Go to definition / declaration | `textDocument/definition`, `declaration` | Also for strings framework plugins know (`view('home')`, `route('login')`, `config('app.name')`, hook names) |
| Go to type definition | `textDocument/typeDefinition` | Class of the inferred type |
| Go to implementation | `textDocument/implementation` | Implementing / extending classes, overriding methods |
| Find references | `textDocument/references` | Overridden / implementing methods (also through traits), `@param` tags, constructors via `new`, global variables, promoted constructor parameters with their property, `compact('name')`, string arguments naming a global function (`add_filter('x', 'fn')`) and `@see fn()` |
| Document highlights | `textDocument/documentHighlight` | Read / write accesses |
| Document symbols | `textDocument/documentSymbol` | Hierarchical outline; Blade sections / stacks / slots, Twig blocks / macros |
| Workspace symbols | `workspace/symbol` | Fuzzy; members with `Class::member` |
| Call hierarchy | `callHierarchy/*` | Incoming and outgoing calls |
| Type hierarchy | `typeHierarchy/*` | Supertypes and subtypes |
| Inlay hints | `textDocument/inlayHint` | Parameter names, inferred return types and closure parameter types |
| Semantic tokens | `textDocument/semanticTokens/full`, `range` | Classes, functions, methods, properties, parameters, variables, constants; modifiers declaration, static, readonly, deprecated, abstract, defaultLibrary |
| Folding | `textDocument/foldingRange` | Blocks, lists, comments, imports, `#region`, Blade directive blocks |
| Selection ranges | `textDocument/selectionRange` | Syntax aware expand selection |
| Document links | `textDocument/documentLink` | `include` / `require` paths, URLs in comments |
| Linked editing | `textDocument/linkedEditingRange` | HTML tag pairs in templates |
| Code lenses | `textDocument/codeLens` | References / implementations counts, parent method, "▶ Run test" for PHPUnit tests (*extra*) |

## Type inference

- Declared types, PHPDoc (`@var`, `@param`, `@return`, `@property`,
  `@method`, `@mixin`) including PHPStan / Psalm syntax: generics
  (`@template`, `@extends`, `@implements`, `@use`), array shapes,
  `list<T>`, `class-string<T>`, conditional return types, `key-of`,
  `value-of`, literal types, `@phpstan-*` / `@psalm-*` aliases.
- Global variables: `$x = new Foo()` at file level types `global $x;`
  and file-level uses of `$x` in other files (like Intelephense).
- Flow analysis: assignments, `instanceof` / `is_*()` / `null` checks,
  early returns, `assert()`, `match`, `foreach` element types, `list()`
  destructuring, closures with `use`, `static` / `$this` / `self` / `parent`.
- Built-in functions through phpstorm-stubs, with PHP version dependent
  signatures.

## Refactorings and code actions

All of these are premium in Intelephense or not available there (*extra*):

| Action | Kind |
|---|---|
| Rename (symbols, variables, parameters, files with PSR-4 namespace update) | `textDocument/rename`, `workspace/willRenameFiles` |
| Import class / function, fully qualify | quick fix |
| Remove unused import, sort imports | quick fix / source |
| Implement abstract methods / interface methods | quick fix |
| Add PHPDoc | refactor |
| Generate getter / setter / both | refactor |
| Generate constructor, promote constructor properties | refactor (*extra*) |
| Extract variable / method / function / constant | refactor (*extra*) |
| Inline variable | refactor (*extra*) |
| Add return type | refactor (*extra*) |
| Change namespace to match PSR-4 | quick fix (*extra*) |
| Format document / range | `textDocument/formatting`, `rangeFormatting` ([formatting](formatting.md)) |

## Frameworks

Built-in plugins ([write your own](plugins.md)):

- **Laravel:** `app()` / `resolve()` / `make()` container types, Eloquent
  relations as properties (`$user->posts` is `Collection<int, Post>`),
  completion and go to definition for `config()`, `view()`, `route()`,
  `__()` / `trans()` and `env()` keys.
- **Blade:** templates are analysed as PHP: hover, completion,
  definition, references, rename and diagnostics in `{{ }}`, `{!! !!}`,
  directives and `@php` blocks; variable types come from the controllers'
  `view()` / `->with()` / `compact()` calls, `$loop` is typed. Directive
  snippets, HTML completion, view names in `@include` / `@extends`, folding,
  outline, semantic tokens, HTML formatting like Intelephense.
- **Symfony:** typed `$container->get(Foo::class)` and Doctrine
  `getRepository()`, route names (`#[Route(name: ...)]`, YAML) and template
  names for `redirectToRoute()`, `generateUrl()`, `render()`.
- **Twig** (*extra*, for clients that send `.twig` files): variable types
  from `render()` / `#[Template]`, `{% set %}`, `{% for %}`, `loop`, `app`;
  hover, completion, definition for attributes (`user.name` is
  `getName()`), tags, filters, functions, tests, template and route names;
  links, folding, outline, semantic tokens. No diagnostics, no formatting.
- **WordPress:** action / filter hook names in `add_action()`,
  `add_filter()`, ... taken from `do_action()` / `apply_filters()` calls,
  with go to definition. The `wordpress` stubs are available via `stubs`.

## HTML in PHP files

Outside `<?php ... ?>`: completion of tags, attributes and closing tags,
linked editing of tag pairs and formatting of HTML, CSS and JavaScript.

## Comparison with Intelephense

Measured with `editors/compare` on Laravel, Symfony, WordPress and
PHP-Parser:

| Area | Result |
|---|---|
| Diagnostics | all codes checked case by case for PHP 7.4 to 8.5 |
| Formatting | identical output for 1197 of 1200 files (300 per project, templates included) |
| Find references | 99.1 to 99.97 % of Intelephense's locations |
| Rename | ~1500 renames: 99.3 % add no diagnostic and round-trip exactly |
