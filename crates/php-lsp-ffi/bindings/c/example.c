/* Example: format code and ask the language server for a hover.
 *
 * Build (from this directory, after `cargo build --release -p php-lsp-ffi`):
 *   cc example.c -I../../include -L../../../../target/release -lphplsp -o example
 * Run:
 *   LD_LIBRARY_PATH=../../../../target/release ./example      (Linux)
 *   DYLD_LIBRARY_PATH=../../../../target/release ./example    (macOS)
 */
#include <stdio.h>
#include <string.h>

#include "php_lsp.h"

int main(void) {
    printf("php-lsp %s\n", php_lsp_version());

    /* 1. formatting without a server */
    char *formatted = php_lsp_format("<?php\nfunction f($a,$b){return $a+$b;}\n", "a.php", NULL);
    printf("formatted:\n%s", formatted);
    php_lsp_string_free(formatted);

    /* 2. a language server in this process */
    char *capabilities = NULL;
    PhpLsp *server = php_lsp_new("{\"capabilities\":{}}", &capabilities);
    if (!server) {
        return 1;
    }
    printf("hover supported: %s\n", strstr(capabilities, "\"hoverProvider\":true") ? "yes" : "no");
    php_lsp_string_free(capabilities);

    php_lsp_send(server,
                 "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/didOpen\",\"params\":{\"textDocument\":"
                 "{\"uri\":\"file:///demo.php\",\"languageId\":\"php\",\"version\":1,"
                 "\"text\":\"<?php\\n/** Says hello. */\\nfunction greet(string $name): string "
                 "{ return $name; }\\ngreet(1, 2);\\n\"}}}");
    php_lsp_send(server,
                 "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"textDocument/hover\",\"params\":"
                 "{\"textDocument\":{\"uri\":\"file:///demo.php\"},\"position\":{\"line\":3,\"character\":2}}}");

    /* responses and notifications (here: the hover and the diagnostics) */
    char *messages = php_lsp_wait(server, 1000);
    printf("messages: %s\n", messages);
    int ok = strstr(messages, "Says hello") != NULL && strstr(messages, "publishDiagnostics") != NULL;
    php_lsp_string_free(messages);

    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"shutdown\"}");
    php_lsp_send(server, "{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}");
    php_lsp_free(server);
    return ok ? 0 : 1;
}
