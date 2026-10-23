# Formatting

The built-in formatter (`php-fmt`) reproduces Intelephense's output: on
Laravel, Symfony, WordPress and PHP-Parser 97 % of the files come out
byte-identical (`REFACTOR=1 editors/compare/compare.js`). It only changes
whitespace: tokens are never added, removed or reordered (the one exception
is a trailing `?>` in PHP-only files, removed like PSR-12 asks), and never
merged: `- -$x` keeps its space (Intelephense turns it into the decrement
`--$x`). Formatting twice gives the same result. A file with syntax errors is not formatted.

## Rules

PSR-12 / PER coding style, as Intelephense applies it:

- indentation from the editor's `tabSize` / `insertSpaces`,
- braces: classes and functions on the next line, control structures on the
  same line (`format.braces`: `psr12`, `allman`, `k&r`),
- one space around binary operators, `=>`, `.`, after `,` and keywords,
  none inside parentheses,
- one statement per line; multi-line argument / parameter / array lists put
  one element per line, keep alignment of `=>` and `=` chosen by the author,
- blank lines around `namespace` and `use` blocks, existing blank lines are
  kept (no maximum, like Intelephense),
- empty bodies as `{}`,
- chained method calls and continued expressions get one extra indent level,
- heredoc / nowdoc and string contents, comments and inline HTML text are
  never touched,
- the file ends with exactly one newline (not for templates).

Range formatting formats the whole file and returns only the edits that
touch the range (all edits for templates).

## Templates (HTML + PHP)

Files mixing HTML and PHP (WordPress themes, `.phtml`, Blade views) are
formatted the way Intelephense does it:

1. The HTML is formatted with a port of js-beautify's HTML beautifier,
   including `<script>` (JavaScript beautifier) and `<style>` (CSS
   beautifier), with Intelephense's options: indent from the editor,
   `wrap_attributes: preserve`, templating `auto`, PHP regions as opaque
   placeholders.
2. Each PHP region influences the HTML indentation like in Intelephense: a
   region that opens blocks (`<?php if ($a): ?>`, `<?php foreach (...) { ?>`)
   indents the HTML after it, a region that closes them (`<?php endif; ?>`)
   outdents.
3. The PHP code is formatted, aligned to the column of its `<?php` tag.

The port gives identical results to js-beautify (and to Intelephense's
patched copy) on 846 WordPress templates, 129 Blade views and the CSS / JS
test inputs. Text in `<pre>`, `<textarea>` and other unformatted elements
is kept. If the formatted HTML would change any non-whitespace character the
HTML step is skipped.

Switch the HTML step off with `format.html: false`: then only the PHP
regions are formatted (and Blade views only in their `@php` blocks).

## Blade

Blade views (`*.blade.php`) are formatted as HTML templates: directives and
`{{ }}` echoes are kept as written and indented like the HTML around them,
`@php ... @endphp` blocks are formatted as PHP.

## External formatters

`format.provider` hands formatting to another tool:

| Value | Tool | Configuration |
|---|---|---|
| `builtin` | php-lsp's formatter | the settings above |
| `php-cs-fixer` | PHP-CS-Fixer (`tools.phpCsFixer.path` / `.args`) | `.php-cs-fixer.php` / `.php-cs-fixer.dist.php` |
| `phpcbf` | PHP_CodeSniffer's fixer (`tools.phpcbf.path` / `.args`) | `phpcs.xml` |

php-cs-fixer runs on a temporary copy, phpcbf reads the text from stdin;
the result replaces the document.

## Without a server

```sh
# Rust
php_lsp::format_document(text, "a.php", &json!({"tabSize": 2}))
# C
php_lsp_format(text, "a.php", "{\"tabSize\":2}")
# JavaScript
format(text, "a.php", JSON.stringify({ tabSize: 2 }))
```

Options: `tabSize` (4), `insertSpaces` (true), `braces` (`psr12`), `html`
(true), `phpVersion` (`8.4`). `file_name` selects the language (`.php`,
`.blade.php`). The result is `null` when the document cannot be
formatted (syntax errors, unsupported language).
