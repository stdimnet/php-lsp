/*
 * php-lsp: PHP language server as a C library (libphplsp).
 *
 * The server speaks the Language Server Protocol (LSP): send JSON-RPC
 * messages with php_lsp_send(), read the server's messages (responses,
 * diagnostics, progress) with php_lsp_receive() or php_lsp_wait().
 * All strings are UTF-8 JSON. Strings returned by the library are freed
 * with php_lsp_string_free(). A server handle must not be used from two
 * threads at the same time.
 */
#ifndef PHP_LSP_H
#define PHP_LSP_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct PhpLsp PhpLsp;

/* Starts a server. initialize_params: JSON of the LSP initialize params
 * (NULL for defaults). If result is not NULL it receives the initialize
 * result (capabilities). Returns NULL on error. */
PhpLsp *php_lsp_new(const char *initialize_params, char **result);

/* Sends one JSON-RPC message. Returns 0, 1 after "exit", -1 on invalid
 * input. */
int php_lsp_send(PhpLsp *server, const char *message);

/* Messages sent by the server since the last call, as a JSON array
 * ("[]" if none). */
char *php_lsp_receive(PhpLsp *server);

/* Like php_lsp_receive(), but waits up to timeout_ms for background work
 * (workspace indexing) when nothing is pending. */
char *php_lsp_wait(PhpLsp *server, unsigned int timeout_ms);

/* Stops and frees a server. */
void php_lsp_free(PhpLsp *server);

/* Formats a PHP / Blade / Twig document without a server. file_name
 * selects the language (NULL: PHP). options: JSON like
 * {"tabSize":4,"insertSpaces":true,"braces":"psr12","html":true,
 *  "phpVersion":"8.4"} (NULL: defaults). Returns NULL on syntax errors. */
char *php_lsp_format(const char *text, const char *file_name, const char *options);

/* The library version (static, do not free). */
const char *php_lsp_version(void);

/* Frees a string returned by the library. */
void php_lsp_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* PHP_LSP_H */
