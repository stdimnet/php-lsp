# Comparison with Intelephense

`compare.js` runs php-lsp and the original Intelephense server side by side on
a project and reports where they differ:

- diagnostics per code (reported by both / only Intelephense / only php-lsp,
  with examples),
- hover and go to definition at sampled positions (member names, calls,
  variables, class names),
- member completion after `->` (share of Intelephense's labels php-lsp also
  offers),
- with `REFACTOR=1`: formatting (files with identical results), find
  references (share of Intelephense's locations), rename and code actions.
  Without a licence Intelephense offers neither rename nor code actions
  (`renameProvider: false`), so these only count php-lsp's results; rename is
  checked by `php-ide`'s `rename_check` example instead.

Options (environment variables): `PHPV=8.3.0` PHP version, `EXACT=1` compare
diagnostic ranges and messages, `STATIC=1` also complete after `Name::`,
`ARROWS=N` completion positions per file, `MAXITEMS=N` completion limit,
`MISSES=1` / `DEFS=1` store every completion / definition difference in the
report, `FMTTABS=1` format with tabs, `FORMATDUMP=dir` write both formatted
versions of differing files; `--php-lsp <binary>` compares another build.

Intelephense is taken from the extension package that
`../vscode-test/run.js` downloads (run that once first). It is only executed
locally for the comparison and is not part of this repository.

```sh
cargo build --release -p php-lsp
node compare.js /path/to/project --files 150 --out report.json
```

Expected differences: php-lsp does not report undefined members on classes
whose ancestors are unknown (e.g. tests without `vendor/`). Find references
of a method or property: Intelephense misses the overrides in sibling
classes whose files come after the file declaring the shared ancestor (its
result depends on the file order; php-lsp reports all of them).
