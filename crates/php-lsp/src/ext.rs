//! Custom (non standard) LSP requests and notifications.

use lsp_types::TextDocumentIdentifier;
use serde::{Deserialize, Serialize};

/// `php-lsp/syntaxTree`: debug dump of the syntax tree of a document.
pub enum SyntaxTree {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxTreeParams {
    pub text_document: TextDocumentIdentifier,
}

impl lsp_types::request::Request for SyntaxTree {
    type Params = SyntaxTreeParams;
    type Result = Option<String>;
    const METHOD: &'static str = "php-lsp/syntaxTree";
}
