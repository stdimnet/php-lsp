//! C interface of the PHP language server (see `include/php_lsp.h`).
//!
//! The server speaks the Language Server Protocol: send JSON-RPC messages
//! with [`php_lsp_send`], collect the server's messages with
//! [`php_lsp_receive`] / [`php_lsp_wait`]. Strings returned by this library
//! are freed with [`php_lsp_string_free`].

#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_int, c_uint, CStr, CString};
use std::time::Duration;

use php_lsp::Embedded;
use serde_json::Value;

/// An in-process language server.
pub struct PhpLsp {
    server: Embedded,
}

fn to_c(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map_or(std::ptr::null_mut(), CString::into_raw)
}

unsafe fn from_c<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    CStr::from_ptr(s).to_str().ok()
}

fn messages(list: Vec<String>) -> *mut c_char {
    to_c(format!("[{}]", list.join(",")))
}

/// Starts a server. `initialize_params` is the JSON of the LSP
/// `initialize` params (NULL: defaults). If `result` is not NULL it
/// receives the `initialize` result (the capabilities). Returns NULL on
/// error.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_new(
    initialize_params: *const c_char,
    result: *mut *mut c_char,
) -> *mut PhpLsp {
    let params = match from_c(initialize_params) {
        Some(s) => match serde_json::from_str::<Value>(s) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        },
        None => serde_json::json!({ "capabilities": {} }),
    };
    match std::panic::catch_unwind(|| Embedded::new(params)) {
        Ok(Ok((server, init))) => {
            if !result.is_null() {
                *result = to_c(init.to_string());
            }
            Box::into_raw(Box::new(PhpLsp { server }))
        }
        _ => std::ptr::null_mut(),
    }
}

/// Sends one JSON-RPC message (request, notification or response).
/// Returns 0, 1 after `exit`, -1 for invalid input.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_send(server: *mut PhpLsp, message: *const c_char) -> c_int {
    let (Some(server), Some(message)) = (server.as_mut(), from_c(message)) else { return -1 };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.server.send(message))) {
        Ok(Ok(true)) => 1,
        Ok(Ok(false)) => 0,
        _ => -1,
    }
}

/// The messages the server sent since the last call, as a JSON array
/// (`[]` if none).
#[no_mangle]
pub unsafe extern "C" fn php_lsp_receive(server: *mut PhpLsp) -> *mut c_char {
    let Some(server) = server.as_mut() else { return std::ptr::null_mut() };
    messages(server.server.receive())
}

/// Like [`php_lsp_receive`], waiting up to `timeout_ms` for background work
/// (indexing) when nothing is pending.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_wait(server: *mut PhpLsp, timeout_ms: c_uint) -> *mut c_char {
    let Some(server) = server.as_mut() else { return std::ptr::null_mut() };
    messages(server.server.wait(Duration::from_millis(u64::from(timeout_ms))))
}

/// Stops and frees a server.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_free(server: *mut PhpLsp) {
    if !server.is_null() {
        drop(Box::from_raw(server));
    }
}

/// Formats a document. `file_name` selects the language (`x.blade.php`,
/// `x.twig`, else PHP; NULL: PHP). `options` is JSON such as
/// `{"tabSize":4,"insertSpaces":true,"braces":"psr12","html":true,"phpVersion":"8.4"}`
/// (NULL: defaults). Returns NULL if the code has syntax errors.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_format(
    text: *const c_char,
    file_name: *const c_char,
    options: *const c_char,
) -> *mut c_char {
    let Some(text) = from_c(text) else { return std::ptr::null_mut() };
    let name = from_c(file_name).unwrap_or("untitled.php");
    let options = from_c(options).and_then(|o| serde_json::from_str(o).ok()).unwrap_or(Value::Null);
    match std::panic::catch_unwind(|| php_lsp::format_document(text, name, &options)) {
        Ok(Some(s)) => to_c(s),
        _ => std::ptr::null_mut(),
    }
}

/// The library version (static string, do not free).
#[no_mangle]
pub extern "C" fn php_lsp_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Frees a string returned by this library.
#[no_mangle]
pub unsafe extern "C" fn php_lsp_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
