# VS Code test

Runs php-lsp inside the real Intelephense VS Code extension (its bundled
server is replaced by `editors/node/php-lsp.js`). See `run.js`; the checks are
in `suite/index.js`, the workspace in `fixture/`.

```sh
cargo build --release -p php-lsp
npm install && xvfb-run -a node run.js   # xvfb-run only needed without a display
```
