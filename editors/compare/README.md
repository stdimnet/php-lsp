# Comparison with Intelephense

`compare.js` runs php-lsp and the original Intelephense server side by side on
a project and reports where they differ:

- diagnostics per code (reported by both / only Intelephense / only php-lsp,
  with examples),
- hover and go to definition at sampled positions (member names, calls,
  variables, class names),
- member completion after `->` (share of Intelephense's labels php-lsp also
  offers).

Intelephense is taken from the extension package that
`../vscode-test/run.js` downloads (run that once first). It is only executed
locally for the comparison and is not part of this repository.

```sh
cargo build --release -p php-lsp
node compare.js /path/to/project --files 150 --out report.json
```

Expected differences: php-lsp does not report undefined members on classes
whose ancestors are unknown (e.g. tests without `vendor/`), and it has no
type checking of arguments (Intelephense `P1006`) yet.
