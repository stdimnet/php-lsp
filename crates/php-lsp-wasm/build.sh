#!/bin/sh
# Builds the npm package into ./pkg (or $1): Node.js, bundler and web
# variants of the WebAssembly module.
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.100:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.100
set -e
cd "$(dirname "$0")"
OUT=${1:-pkg}
cargo build --release -p php-lsp-wasm --target wasm32-unknown-unknown
WASM=../../target/wasm32-unknown-unknown/release/php_lsp_wasm.wasm
rm -rf "$OUT"
mkdir -p "$OUT"
for target in nodejs bundler web; do
  dir=$target
  [ "$target" = nodejs ] && dir=node
  wasm-bindgen "$WASM" --target "$target" --out-dir "$OUT/$dir"
done
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1)
sed "s/\"version\": \"0.0.0\"/\"version\": \"$VERSION\"/" js/package.json > "$OUT/package.json"
cp js/README.md "$OUT/README.md"
echo "built $OUT (version $VERSION)"
