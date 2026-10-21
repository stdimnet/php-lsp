//! Diagnostics.

use crate::{Config, SourceFile, TextRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Stable diagnostic codes (shown to users and usable for suppression).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    SyntaxError,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::SyntaxError => "P1001",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
    pub code: DiagnosticCode,
    /// The code is unnecessary (rendered faded by editors).
    pub unnecessary: bool,
    /// The code is deprecated (rendered struck through by editors).
    pub deprecated: bool,
}

pub(crate) fn diagnostics(config: &Config, file: &SourceFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if config.syntax_diagnostics {
        for e in file.parse.errors() {
            out.push(Diagnostic {
                range: e.range,
                severity: Severity::Error,
                message: e.message.clone(),
                code: DiagnosticCode::SyntaxError,
                unnecessary: false,
                deprecated: false,
            });
        }
    }
    out
}
