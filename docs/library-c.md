# C library

`libphplsp` runs the complete language server inside your process. You
send LSP messages as JSON strings and read the server's answers; there is no
child process, no stdio and no socket. Any language with a C FFI can use it.

## Getting it

- Release archive `libphplsp-<version>-<target>`: `include/php_lsp.h`,
  `lib/` (`libphplsp.so` / `libphplsp.dylib` / `phplsp.dll` + import
  library, and the static `libphplsp.a` / `phplsp.lib`), `bindings/`.
- From source: `cargo build --release -p php-lsp-ffi`, output in
  `target/release/`, header in `crates/php-lsp-ffi/include/php_lsp.h`.

Linking the static library needs the system libraries Rust uses; on Linux
`-lpthread -ldl -lm -lrt -lutil`. `cargo rustc --release -p php-lsp-ffi
--crate-type staticlib -- --print native-static-libs` prints the exact list
for a platform.

## API

```c
typedef struct PhpLsp PhpLsp;

PhpLsp *php_lsp_new(const char *initialize_params, char **result);
int     php_lsp_send(PhpLsp *server, const char *message);
char   *php_lsp_receive(PhpLsp *server);
char   *php_lsp_wait(PhpLsp *server, unsigned int timeout_ms);
void    php_lsp_free(PhpLsp *server);

char       *php_lsp_format(const char *text, const char *file_name, const char *options);
const char *php_lsp_version(void);
void        php_lsp_string_free(char *s);
```

| Function | Description |
|---|---|
| `php_lsp_new` | Starts a server. `initialize_params` is the JSON of LSP `initialize` params (`NULL`: `{"capabilities":{}}`); `rootUri` / `workspaceFolders` start indexing that directory, `initializationOptions` and settings work as for the binary. `*result` receives the `initialize` result (capabilities). `NULL` on error. The `initialized` notification is implied |
| `php_lsp_send` | Handles one JSON-RPC message: a request, a notification or the response to a server request. Returns `0`, `1` after `exit`, `-1` for invalid JSON |
| `php_lsp_receive` | Everything the server sent since the last call, as a JSON array (`"[]"` when nothing): responses, `publishDiagnostics`, progress, log messages, requests to the client |
| `php_lsp_wait` | Like `php_lsp_receive`, but when nothing is queued it waits up to `timeout_ms` for background work (indexing) to finish |
| `php_lsp_free` | Stops the server and frees it |
| `php_lsp_format` | Formats a document without a server ([formatting](formatting.md#without-a-server)); `NULL` when it cannot be formatted |
| `php_lsp_version` | Version string; static, do not free |
| `php_lsp_string_free` | Frees every `char *` the library returned |

Rules:

- All strings are UTF-8. Positions in messages use UTF-16 code units
  unless the client capabilities negotiate `positionEncoding` `utf-8`.
- Requests are answered synchronously: after `php_lsp_send` of a request
  the response is already in the queue for `php_lsp_receive`.
- A handle must not be used from two threads at once; different handles
  are independent. Indexing runs on internal worker threads.
- Files that are not open are read from disk (workspace root) or can be
  passed with the notification `php/indexFiles`
  (`{"files":[{"uri":"file:///p/a.php","text":"<?php ..."}]}`) when there
  is no file system (see [LSP reference](lsp.md#custom-notifications)).

## Example (C)

`crates/php-lsp-ffi/bindings/c/example.c`, built in CI:

```c
#include <stdio.h>
#include "php_lsp.h"

int main(void) {
    char *caps = NULL;
    PhpLsp *server = php_lsp_new("{\"capabilities\":{}}", &caps);
    php_lsp_string_free(caps);

    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didOpen\",\"params\":"
                         "{\"textDocument\":{\"uri\":\"file:///demo.php\",\"languageId\":\"php\","
                         "\"version\":1,\"text\":\"<?php\\nfunction greet() {}\\ngreet();\\n\"}}}");
    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\"params\":"
                         "{\"textDocument\":{\"uri\":\"file:///demo.php\"},\"position\":{\"line\":2,\"character\":1}}}");

    char *messages = php_lsp_receive(server);   /* [ {hover response}, {publishDiagnostics} ] */
    puts(messages);
    php_lsp_string_free(messages);

    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"shutdown\"}");
    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}");
    php_lsp_free(server);
}
```

```sh
cc example.c -Iinclude -Llib -lphplsp -o example
LD_LIBRARY_PATH=lib ./example
```

## Python

`bindings/python/php_lsp.py` is a ready ctypes binding (tested in CI):

```python
from php_lsp import PhpLsp   # PHP_LSP_LIB=/path/to/libphplsp.so

lsp = PhpLsp(initialize_params={"capabilities": {}, "rootUri": "file:///path/to/project"})
print(lsp.format("<?php\nif($a){echo 1;}\n"))
lsp.notify("textDocument/didOpen", {"textDocument": {
    "uri": "file:///path/to/project/a.php", "languageId": "php", "version": 1, "text": "<?php\n$x = new DateTime();\n$x->"}})
items = lsp.request("textDocument/completion", {
    "textDocument": {"uri": "file:///path/to/project/a.php"}, "position": {"line": 2, "character": 4}})
diagnostics = [m for m in lsp.receive() if m.get("method") == "textDocument/publishDiagnostics"]
lsp.close()
```

`request()` returns the result of the matching response and drops other
messages it reads; use `send()` + `receive()` when you need everything.

## Other languages

The pattern is always: load the library, call `php_lsp_new`, exchange
strings, free returned strings with `php_lsp_string_free`.

**C++**: include `php_lsp.h` (it has `extern "C"`); wrap returned strings
in `std::unique_ptr<char, decltype(&php_lsp_string_free)>`.

**Go** (cgo):

```go
// #cgo LDFLAGS: -lphplsp
// #include <stdlib.h>
// #include "php_lsp.h"
import "C"
import "unsafe"

func receive(s *C.PhpLsp) string {
	p := C.php_lsp_receive(s)
	defer C.php_lsp_string_free(p)
	return C.GoString(p)
}

func send(s *C.PhpLsp, msg string) {
	m := C.CString(msg)
	defer C.free(unsafe.Pointer(m))
	C.php_lsp_send(s, m)
}
```

**C#** (P/Invoke): declare the functions with
`[DllImport("phplsp")]`, return `IntPtr`, convert with
`Marshal.PtrToStringUTF8` and free with `php_lsp_string_free`.

**Java** 22+ (FFM, `java.lang.foreign`), or JNA: `Native.load("phplsp",
...)` with `Pointer` return types, `getString(0, "UTF-8")`.

**Node.js**: prefer the [WebAssembly package](library-wasm.md); with
native code use `koffi` or `ffi-napi` the same way.

**PHP** (FFI extension): `FFI::cdef()` with the function declarations of
`php_lsp.h` (without the `#` preprocessor lines) and `'libphplsp.so'`;
read returned strings with `FFI::string`, then `php_lsp_string_free`.
