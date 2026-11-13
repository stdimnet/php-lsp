"""Python binding for libphplsp (ctypes), with a small demo.

Usage: PHP_LSP_LIB=/path/to/libphplsp.so python3 php_lsp.py
"""
import ctypes
import json
import os
import sys


class PhpLsp:
    """A PHP language server running in this process."""

    def __init__(self, library=None, initialize_params=None):
        path = library or os.environ.get("PHP_LSP_LIB", "libphplsp.so")
        lib = ctypes.CDLL(path)
        lib.php_lsp_new.restype = ctypes.c_void_p
        lib.php_lsp_new.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)]
        lib.php_lsp_send.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        for name in ("php_lsp_receive", "php_lsp_wait", "php_lsp_format"):
            getattr(lib, name).restype = ctypes.c_void_p
        lib.php_lsp_receive.argtypes = [ctypes.c_void_p]
        lib.php_lsp_wait.argtypes = [ctypes.c_void_p, ctypes.c_uint]
        lib.php_lsp_format.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p]
        lib.php_lsp_free.argtypes = [ctypes.c_void_p]
        lib.php_lsp_string_free.argtypes = [ctypes.c_void_p]
        lib.php_lsp_version.restype = ctypes.c_char_p
        self.lib = lib
        result = ctypes.c_void_p()
        params = json.dumps(initialize_params or {"capabilities": {}}).encode()
        self.handle = lib.php_lsp_new(params, ctypes.byref(result))
        if not self.handle:
            raise RuntimeError("php_lsp_new failed")
        self.capabilities = json.loads(self._take(result.value))
        self.next_id = 0

    def _take(self, ptr):
        if not ptr:
            return None
        text = ctypes.string_at(ptr).decode()
        self.lib.php_lsp_string_free(ptr)
        return text

    def version(self):
        return self.lib.php_lsp_version().decode()

    def send(self, message):
        """Sends one JSON-RPC message (a dict)."""
        return self.lib.php_lsp_send(self.handle, json.dumps(message).encode())

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def request(self, method, params, timeout_ms=2000):
        """Sends a request and returns its result (other messages are dropped)."""
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        for message in self.receive(timeout_ms):
            if message.get("id") == self.next_id and "method" not in message:
                return message.get("result")
        return None

    def receive(self, timeout_ms=0):
        """The server's messages (responses, diagnostics, ...) as dicts."""
        if timeout_ms:
            ptr = self.lib.php_lsp_wait(self.handle, timeout_ms)
        else:
            ptr = self.lib.php_lsp_receive(self.handle)
        return json.loads(self._take(ptr))

    def format(self, text, file_name="untitled.php", options=None):
        """Formats a document; None on syntax errors."""
        ptr = self.lib.php_lsp_format(text.encode(), file_name.encode(), json.dumps(options or {}).encode())
        return self._take(ptr)

    def close(self):
        if self.handle:
            self.request("shutdown", None)
            self.notify("exit", None)
            self.lib.php_lsp_free(self.handle)
            self.handle = None


if __name__ == "__main__":
    lsp = PhpLsp()
    print("php-lsp", lsp.version())
    print(lsp.format("<?php\nif($a){echo 1;}\n"))
    uri = "file:///demo.php"
    text = "<?php\nclass User { public function name(): string { return ''; } }\n$u = new User();\n$u->\n"
    lsp.notify("textDocument/didOpen", {"textDocument": {"uri": uri, "languageId": "php", "version": 1, "text": text}})
    items = lsp.request("textDocument/completion", {"textDocument": {"uri": uri}, "position": {"line": 3, "character": 4}})
    labels = [i["label"] for i in (items["items"] if isinstance(items, dict) else items)]
    print("completion:", labels)
    lsp.close()
    sys.exit(0 if "name" in labels else 1)
