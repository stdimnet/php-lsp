//! IDE features for PHP as a plain Rust API.
//!
//! The [`Workspace`] owns the source files and answers queries about them.
//! It has no dependency on the Language Server Protocol, so it can be
//! embedded directly into other applications:
//!
//! ```
//! use php_ide::{Workspace, Config};
//!
//! let mut ws = Workspace::new(Config::default());
//! let file = ws.set_file("file:///a.php", "<?php class A { function f() {} }".into());
//! let symbols = ws.document_symbols(file);
//! assert_eq!(symbols[0].name, "A");
//! assert_eq!(symbols[0].children[0].name, "f");
//! ```

mod diagnostics;
mod folding;
mod line_index;
mod selection;
mod symbols;

use std::collections::HashMap;
use std::sync::Arc;

pub use diagnostics::{Diagnostic, DiagnosticCode, Severity};
pub use folding::{Fold, FoldKind};
pub use line_index::{LineCol, LineIndex, PositionEncoding};
pub use php_syntax::{self as syntax, PhpVersion, TextRange, TextSize};
pub use symbols::{Symbol, SymbolKind};

/// Identifies a file inside a [`Workspace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// Analysis configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The PHP version used for parsing and version dependent diagnostics.
    pub php_version: PhpVersion,
    /// Report syntax errors.
    pub syntax_diagnostics: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { php_version: PhpVersion::default(), syntax_diagnostics: true }
    }
}

/// A parsed source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: Arc<str>,
    pub text: Arc<str>,
    pub parse: php_syntax::Parse,
    pub line_index: Arc<LineIndex>,
}

/// Holds all files and answers IDE queries.
#[derive(Debug, Default)]
pub struct Workspace {
    config: Config,
    ids: HashMap<Arc<str>, FileId>,
    files: Vec<Option<SourceFile>>,
}

impl Workspace {
    pub fn new(config: Config) -> Workspace {
        Workspace { config, ids: HashMap::new(), files: Vec::new() }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Changes the configuration. Files are re-parsed if the PHP version changed.
    pub fn set_config(&mut self, config: Config) {
        let reparse = config.php_version != self.config.php_version;
        self.config = config;
        if reparse {
            let version = self.config.php_version;
            for file in self.files.iter_mut().flatten() {
                file.parse = php_syntax::parse(&file.text, version);
            }
        }
    }

    /// Adds or replaces a file. `path` is any unique key, e.g. a URI.
    pub fn set_file(&mut self, path: &str, text: String) -> FileId {
        let id = self.file_id_or_insert(path);
        let path: Arc<str> = path.into();
        let parse = php_syntax::parse(&text, self.config.php_version);
        let line_index = Arc::new(LineIndex::new(&text));
        self.files[id.0 as usize] = Some(SourceFile { path, text: text.into(), parse, line_index });
        id
    }

    pub fn remove_file(&mut self, id: FileId) {
        if let Some(slot) = self.files.get_mut(id.0 as usize) {
            if let Some(file) = slot.take() {
                self.ids.remove(&file.path);
            }
        }
    }

    pub fn file_id(&self, path: &str) -> Option<FileId> {
        self.ids.get(path).copied()
    }

    fn file_id_or_insert(&mut self, path: &str) -> FileId {
        if let Some(&id) = self.ids.get(path) {
            return id;
        }
        let id = FileId(self.files.len() as u32);
        self.files.push(None);
        self.ids.insert(path.into(), id);
        id
    }

    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize).and_then(Option::as_ref)
    }

    pub fn files(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
        self.files.iter().enumerate().filter_map(|(i, f)| Some((FileId(i as u32), f.as_ref()?)))
    }

    pub fn line_index(&self, id: FileId) -> Option<Arc<LineIndex>> {
        self.file(id).map(|f| f.line_index.clone())
    }

    // ----------------------------------------------------------- features

    pub fn diagnostics(&self, id: FileId) -> Vec<Diagnostic> {
        self.file(id).map(|f| diagnostics::diagnostics(&self.config, f)).unwrap_or_default()
    }

    pub fn document_symbols(&self, id: FileId) -> Vec<Symbol> {
        self.file(id).map(|f| symbols::document_symbols(&f.parse.tree())).unwrap_or_default()
    }

    pub fn folding_ranges(&self, id: FileId) -> Vec<Fold> {
        self.file(id).map(|f| folding::folding_ranges(&f.parse.syntax_node(), &f.text)).unwrap_or_default()
    }

    /// For each offset, the nested ranges from innermost to outermost.
    pub fn selection_ranges(&self, id: FileId, offsets: &[TextSize]) -> Vec<Vec<TextRange>> {
        let Some(f) = self.file(id) else { return Vec::new() };
        let root = f.parse.syntax_node();
        offsets.iter().map(|&o| selection::selection_ranges(&root, o)).collect()
    }

    /// Debug dump of the syntax tree.
    pub fn syntax_tree(&self, id: FileId) -> Option<String> {
        self.file(id).map(|f| f.parse.debug_tree())
    }
}
