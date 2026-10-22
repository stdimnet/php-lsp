# Third-party notices

php-lsp includes or is derived from the following works.

## JetBrains phpstorm-stubs

The PHP stubs embedded by `crates/php-stubs` (`crates/php-stubs/stubs`) are
taken from [JetBrains/phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs),
licensed under the Apache License 2.0 (see `LICENSE-APACHE`).

## js-beautify

`crates/php-fmt/src/beautify` is a Rust port of the HTML, CSS and JavaScript
beautifiers of [js-beautify](https://github.com/beautifier/js-beautify) 1.15.1.

> The MIT License (MIT)
>
> Copyright (c) 2007-2018 Einar Lielmanis, Liam Newman, and contributors.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## Rust crates

The binaries link Rust crates from crates.io (rowan, lsp-server, lsp-types,
serde, serde_json, regex, rayon, walkdir, globset, crossbeam-channel,
bincode, miniz_oxide, wasm-bindgen and their dependencies), each under its
own license (MIT and/or Apache-2.0); see `Cargo.lock` for the full list.
