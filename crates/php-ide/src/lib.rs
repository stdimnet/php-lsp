//! IDE features for PHP as a plain Rust API.
//!
//! The [`Workspace`] owns the source files and the global symbol index and
//! answers queries about them. It has no dependency on the Language Server
//! Protocol, so it can be embedded directly into other applications:
//!
//! ```
//! use php_ide::{Config, Workspace};
//!
//! let mut ws = Workspace::new(Config::default());
//! let file = ws.set_file("file:///a.php", "<?php class A { function f() {} }".into());
//! let symbols = ws.document_symbols(file);
//! assert_eq!(symbols[0].name, "A");
//! assert_eq!(symbols[0].children[0].name, "f");
//! ```

mod blade;
mod classify;
mod code_actions;
mod completion;
pub mod composer;
mod diagnostics;
mod doc_types;
mod folding;
mod hierarchy;
mod hover;
mod html;
mod index_cache;
mod indexer;
mod infer;
mod inlay_hints;
mod line_index;
mod links;
mod lint;
mod lint_compat;
mod lint_types;
mod navigation;
pub mod plugins;
mod refactor;
mod rename;
mod render;
mod selection;
mod sema;
mod semantic_tokens;
mod signature_help;
mod symbols;
mod twig;
mod typecheck;
mod workspace_symbols;

use std::collections::HashMap;
use std::sync::Arc;

pub use classify::Definition;
pub use code_actions::{CodeAction, CodeActionKind};
pub use completion::{CompletionItem, CompletionKind, CompletionList, TextEdit};
pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
pub use folding::{Fold, FoldKind};
pub use hierarchy::{CodeLens, CodeLensKind, HierarchyCall, HierarchyItem, ResolvedCodeLens};
pub use hover::Hover;
pub use index_cache::IndexCache;
pub use indexer::{collect_files, index_files, index_files_cached, IndexedFile, ScanConfig};
pub use inlay_hints::{InlayHint, InlayHintKind, InlayHintsConfig};
pub use line_index::{LineCol, LineIndex, PositionEncoding};
pub use links::DocumentLink;
pub use navigation::{Location, Reference, ReferenceKind};
pub use php_fmt::{BraceStyle, FormatOptions};
pub use php_hir as hir;
pub use php_index::{FileId, FileKind, Index};
pub use php_syntax::{self as syntax, PhpVersion, TextRange, TextSize};
pub use rename::{RenameError, SourceChange};
pub use semantic_tokens::{SemanticToken, SemanticTokenType, TOKEN_MODIFIERS, TOKEN_TYPES};
pub use signature_help::{ParameterInfo, SignatureHelp, SignatureInfo};
pub use symbols::{Symbol, SymbolKind};
pub use workspace_symbols::WorkspaceSymbol;

use php_hir::{ExtractOptions, FileScopes, FileSymbols};

/// Path prefix of embedded stub files.
pub const STUB_SCHEME: &str = "phpstub://";

/// Analysis configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The PHP version used for parsing and version dependent analysis.
    pub php_version: PhpVersion,
    /// Which diagnostics are reported.
    pub diagnostics: DiagnosticsConfig,
    /// Stub extensions to load (see [`php_stubs::DEFAULT_STUBS`]).
    pub stubs: Vec<String>,
    /// Insert `use` statements when completing class names.
    pub auto_import: bool,
    /// Maximum number of completion items.
    pub max_completion_items: usize,
    pub inlay_hints: InlayHintsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            php_version: PhpVersion::default(),
            diagnostics: DiagnosticsConfig::default(),
            stubs: php_stubs::DEFAULT_STUBS.iter().map(|s| s.to_string()).collect(),
            auto_import: true,
            max_completion_items: 100,
            inlay_hints: InlayHintsConfig::default(),
        }
    }
}

/// A file known to the workspace.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: Arc<str>,
    pub kind: FileKind,
    pub text: Arc<str>,
    pub line_index: Arc<LineIndex>,
    /// Set for open documents; other files are parsed on demand.
    open: Option<OpenFile>,
}

#[derive(Debug, Clone)]
struct OpenFile {
    parse: php_syntax::Parse,
    scopes: Arc<FileScopes>,
}

impl SourceFile {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// Holds all files and the symbol index and answers IDE queries.
#[derive(Debug, Default)]
pub struct Workspace {
    config: Config,
    ids: HashMap<Arc<str>, FileId>,
    files: Vec<Option<SourceFile>>,
    index: Index,
    stubs_loaded: Vec<String>,
    /// While `false` (e.g. during initial indexing) diagnostics about
    /// undefined symbols are suppressed.
    index_not_ready: bool,
    plugins: plugins::Plugins,
    /// Incremented on every change (used to invalidate plugin caches).
    revision: u64,
    /// Used by [`Workspace::index_files`] (and so for the stubs).
    index_cache: Option<Arc<IndexCache>>,
}

impl Workspace {
    pub fn new(config: Config) -> Workspace {
        Workspace { config, ..Default::default() }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    /// Changes whenever a file is added, changed or removed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Adds a framework plugin (Laravel, Symfony and WordPress are built in).
    pub fn register_plugin(&mut self, plugin: Arc<dyn plugins::Plugin>) {
        self.plugins.0.push(plugin);
    }

    pub fn plugins(&self) -> &[Arc<dyn plugins::Plugin>] {
        &self.plugins.0
    }

    /// Whether the index is complete enough to report undefined symbols.
    pub fn index_ready(&self) -> bool {
        !self.index_not_ready
    }

    pub fn set_index_ready(&mut self, ready: bool) {
        self.index_not_ready = !ready;
    }

    /// Changes the configuration. Files are re-analysed if the PHP version
    /// changed; stubs are reloaded if the stub list changed.
    pub fn set_config(&mut self, config: Config) {
        let version_changed = config.php_version != self.config.php_version;
        let stubs_changed = config.stubs != self.config.stubs;
        self.config = config;
        if version_changed {
            let ids: Vec<FileId> = self.files().map(|(id, _)| id).collect();
            for id in ids {
                let f = self.file(id).unwrap();
                let (path, text, kind, open) = (f.path.clone(), f.text.clone(), f.kind, f.is_open());
                if open {
                    self.set_file_with_kind(&path, text.to_string(), kind);
                } else {
                    self.index_file(&path, text, kind);
                }
            }
        }
        if stubs_changed && !self.stubs_loaded.is_empty() {
            self.load_stubs();
        }
    }

    /// Options used when extracting symbols (for indexing on other threads).
    pub fn extract_options(&self) -> ExtractOptions {
        self.opts()
    }

    fn opts(&self) -> ExtractOptions {
        ExtractOptions::new(self.config.php_version)
    }

    // ----------------------------------------------------------- files

    /// Adds or replaces an open document. `path` is any unique key, e.g. a URI.
    pub fn set_file(&mut self, path: &str, text: String) -> FileId {
        let kind =
            self.file_id(path).and_then(|id| self.file(id)).map(|f| f.kind).unwrap_or(FileKind::Workspace);
        self.set_file_with_kind(path, text, kind)
    }

    fn set_file_with_kind(&mut self, path: &str, text: String, kind: FileKind) -> FileId {
        let id = self.file_id_or_insert(path);
        self.revision += 1;
        let parse = indexer::parse_source(path, &text, self.config.php_version);
        let (scopes, symbols) = php_hir::analyze_file(&parse.tree(), self.opts());
        self.index.set_file(id, kind, Arc::new(symbols));
        let line_index = Arc::new(LineIndex::new(&text));
        self.files[id.0 as usize] = Some(SourceFile {
            path: path.into(),
            kind,
            text: text.into(),
            line_index,
            open: Some(OpenFile { parse, scopes: Arc::new(scopes) }),
        });
        id
    }

    /// Adds or replaces a file that is only indexed (not opened).
    pub fn index_file(&mut self, path: &str, text: Arc<str>, kind: FileKind) -> FileId {
        let symbols = indexer::extract_symbols(path, &text, self.opts());
        self.add_indexed(IndexedFile {
            path: path.into(),
            text,
            kind,
            symbols: Arc::new(symbols),
            line_index: None,
        })
    }

    /// Adds a file analysed elsewhere (e.g. in parallel with [`indexer::index_files`]).
    pub fn add_indexed(&mut self, file: IndexedFile) -> FileId {
        let id = self.file_id_or_insert(&file.path);
        // never replace an open document by its on-disk version
        if self.file(id).is_some_and(|f| f.is_open()) {
            return id;
        }
        self.index.set_file(id, file.kind, file.symbols);
        self.revision += 1;
        let line_index = file.line_index.unwrap_or_else(|| Arc::new(LineIndex::new(&file.text)));
        self.files[id.0 as usize] =
            Some(SourceFile { path: file.path, kind: file.kind, text: file.text, line_index, open: None });
        id
    }

    /// Analyses many files in parallel and adds them.
    /// Sets the on-disk cache [`Workspace::index_files`] reads from.
    pub fn set_index_cache(&mut self, cache: Option<Arc<IndexCache>>) {
        self.index_cache = cache;
    }

    pub fn index_files(&mut self, files: Vec<(String, Arc<str>, FileKind)>) {
        let indexed = indexer::index_files_cached(files, self.opts(), self.index_cache.as_deref());
        for f in indexed {
            self.add_indexed(f);
        }
    }

    /// Marks an open document as closed. It stays indexed with its current
    /// text (call [`Workspace::remove_file`] if it no longer exists).
    pub fn close_file(&mut self, id: FileId) {
        if let Some(Some(f)) = self.files.get_mut(id.0 as usize) {
            f.open = None;
        }
    }

    pub fn remove_file(&mut self, id: FileId) {
        self.revision += 1;
        if let Some(slot) = self.files.get_mut(id.0 as usize) {
            if let Some(file) = slot.take() {
                self.ids.remove(&file.path);
            }
        }
        self.index.remove_file(id);
    }

    /// Loads the configured stub extensions (replacing previously loaded stubs).
    pub fn load_stubs(&mut self) {
        let files = self.stub_files();
        self.index_files(files);
        self.mark_stubs_loaded();
    }

    /// Removes previously loaded stubs and returns the configured stub files
    /// as `(path, text, kind)`, e.g. for indexing on another thread.
    pub fn stub_files(&mut self) -> Vec<(String, Arc<str>, FileKind)> {
        let stale: Vec<FileId> =
            self.files().filter(|(_, f)| f.kind == FileKind::Stub).map(|(id, _)| id).collect();
        for id in stale {
            self.remove_file(id);
        }
        php_stubs::files_for(&self.config.stubs)
            .map(|f| (format!("{STUB_SCHEME}{}", f.path), Arc::<str>::from(f.text), FileKind::Stub))
            .chain(std::iter::once((
                format!("{STUB_SCHEME}php-lsp/blade.php"),
                Arc::<str>::from(blade::STUB),
                FileKind::Stub,
            )))
            .collect()
    }

    pub fn mark_stubs_loaded(&mut self) {
        self.stubs_loaded = self.config.stubs.clone();
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

    pub fn symbols(&self, id: FileId) -> Option<&Arc<FileSymbols>> {
        self.index.file_symbols(id)
    }

    /// The syntax tree of a file (cached for open documents).
    pub fn parse(&self, id: FileId) -> Option<php_syntax::Parse> {
        let f = self.file(id)?;
        Some(match &f.open {
            Some(o) => o.parse.clone(),
            None => indexer::parse_source(&f.path, &f.text, self.config.php_version),
        })
    }

    pub(crate) fn scopes(&self, id: FileId, parse: &php_syntax::Parse) -> Arc<FileScopes> {
        match self.file(id).and_then(|f| f.open.as_ref()) {
            Some(o) => o.scopes.clone(),
            None => Arc::new(FileScopes::build(&parse.tree())),
        }
    }

    // ----------------------------------------------------------- features

    pub fn diagnostics(&self, id: FileId) -> Vec<Diagnostic> {
        let Some(parse) = self.parse(id) else { return Vec::new() };
        let mut out = diagnostics::syntax_diagnostics(&self.config, &parse);
        // a template ending in a directive / echo without a final newline: its
        // virtual code has no room for the closing `;`
        if let Some(f) = self.file(id).filter(|f| blade::is_blade(&f.path)) {
            let end = TextSize::from(f.text.len() as u32);
            out.retain(|d| !(d.range.start() >= end && d.message == "expected `;`"));
        }
        if let Some(sema) = sema::Sema::new(self, id) {
            out.extend(diagnostics::semantic_diagnostics(&sema));
        }
        out
    }

    fn twig_file(&self, id: FileId) -> Option<&SourceFile> {
        self.file(id).filter(|f| twig::is_twig(&f.path))
    }

    pub fn document_symbols(&self, id: FileId) -> Vec<Symbol> {
        if let Some(f) = self.twig_file(id) {
            return twig::symbols(&f.text);
        }
        if let Some(f) = self.file(id).filter(|f| blade::is_blade(&f.path)) {
            return blade::symbols(&f.text, &blade::virtual_php(&f.text));
        }
        self.parse(id).map(|p| symbols::document_symbols(&p.tree())).unwrap_or_default()
    }

    pub fn folding_ranges(&self, id: FileId) -> Vec<Fold> {
        let (Some(p), Some(f)) = (self.parse(id), self.file(id)) else { return Vec::new() };
        if twig::is_twig(&f.path) {
            return twig::folds(&f.text);
        }
        if blade::is_blade(&f.path) {
            return blade::folds(&f.text, &blade::virtual_php(&f.text));
        }
        folding::folding_ranges(&p.syntax_node(), &f.text)
    }

    /// For each offset, the nested ranges from innermost to outermost.
    pub fn selection_ranges(&self, id: FileId, offsets: &[TextSize]) -> Vec<Vec<TextRange>> {
        if self.twig_file(id).is_some() {
            return Vec::new();
        }
        let Some(p) = self.parse(id) else { return Vec::new() };
        let root = p.syntax_node();
        offsets.iter().map(|&o| selection::selection_ranges(&root, o)).collect()
    }

    /// What the identifier at `offset` refers to.
    pub fn definition_at(&self, id: FileId, offset: TextSize) -> Option<Definition> {
        let sema = sema::Sema::new(self, id)?;
        classify::classify(&sema, offset).map(|(d, _)| d)
    }

    pub fn hover(&self, id: FileId, offset: TextSize) -> Option<Hover> {
        if self.twig_file(id).is_some() {
            return twig::hover(self, id, offset);
        }
        hover::hover(&sema::Sema::new(self, id)?, offset)
    }

    pub fn goto_definition(&self, id: FileId, offset: TextSize) -> Vec<Location> {
        if self.twig_file(id).is_some() {
            return twig::goto_definition(self, id, offset);
        }
        sema::Sema::new(self, id).map(|s| navigation::goto_definition(&s, offset)).unwrap_or_default()
    }

    /// Goes to the declaration of the type of the expression at `offset`.
    pub fn goto_type_definition(&self, id: FileId, offset: TextSize) -> Vec<Location> {
        sema::Sema::new(self, id).map(|s| navigation::goto_type_definition(&s, offset)).unwrap_or_default()
    }

    /// Implementations of an interface / abstract method, or subclasses.
    pub fn goto_implementation(&self, id: FileId, offset: TextSize) -> Vec<Location> {
        sema::Sema::new(self, id).map(|s| navigation::goto_implementation(&s, offset)).unwrap_or_default()
    }

    /// References like Intelephense reports them: names cover the whole
    /// (qualified) name, built-in declarations are left out.
    pub fn references(&self, id: FileId, offset: TextSize, include_declaration: bool) -> Vec<Reference> {
        let refs = sema::Sema::new(self, id)
            .map(|s| navigation::references(&s, offset, include_declaration))
            .unwrap_or_default();
        refs.into_iter()
            .filter(|r| self.index().file_kind(r.file) != Some(php_index::FileKind::Stub))
            .map(|mut r| {
                if let Some(f) = self.file(r.file) {
                    let start = usize::from(r.range.start());
                    let bytes = f.text.as_bytes();
                    let mut s = start;
                    // `\Foo\Bar`: back over the namespace segments
                    while s > 0
                        && (bytes[s - 1].is_ascii_alphanumeric()
                            || matches!(bytes[s - 1], b'_' | b'\\' | 0x80..))
                    {
                        s -= 1;
                    }
                    if s < start && f.text[s..start].contains('\\') && !f.text[..start].ends_with('$') {
                        r.range = TextRange::new(TextSize::from(s as u32), r.range.end());
                    }
                }
                r
            })
            .collect()
    }

    /// Opening and closing tag names of the HTML element at `offset`.
    pub fn linked_editing_ranges(&self, id: FileId, offset: TextSize) -> Option<Vec<TextRange>> {
        let f = self.file(id)?;
        let root = if blade::is_blade(&f.path) || twig::is_twig(&f.path) {
            php_syntax::parse(&f.text, self.config.php_version).syntax_node()
        } else {
            self.parse(id)?.syntax_node()
        };
        html::linked_editing_ranges(&root, offset)
    }

    /// Occurrences of the symbol at `offset` in the same file.
    pub fn document_highlights(&self, id: FileId, offset: TextSize) -> Vec<Reference> {
        sema::Sema::new(self, id).map(|s| navigation::document_highlights(&s, offset)).unwrap_or_default()
    }

    pub fn completions(&self, id: FileId, offset: TextSize) -> CompletionList {
        if let Some(f) = self.twig_file(id) {
            // outside of Twig tags: HTML
            return twig::completions(self, id, offset).unwrap_or_else(|| CompletionList {
                items: html::completions(
                    &php_syntax::parse(&f.text, self.config.php_version).syntax_node(),
                    offset,
                )
                .unwrap_or_default(),
                incomplete: false,
            });
        }
        if let Some(f) = self.file(id).filter(|f| blade::is_blade(&f.path)) {
            let b = blade::virtual_php(&f.text);
            if !blade::in_code(&b, offset) {
                let items = blade::directive_completions(&f.text, offset).unwrap_or_else(|| {
                    html::completions(
                        &php_syntax::parse(&f.text, self.config.php_version).syntax_node(),
                        offset,
                    )
                    .unwrap_or_default()
                });
                return CompletionList { items, incomplete: false };
            }
        }
        sema::Sema::new(self, id).map(|s| completion::completions(&s, offset)).unwrap_or_default()
    }

    pub fn signature_help(&self, id: FileId, offset: TextSize) -> Option<SignatureHelp> {
        signature_help::signature_help(&sema::Sema::new(self, id)?, offset)
    }

    pub fn workspace_symbols(&self, query: &str, limit: usize) -> Vec<WorkspaceSymbol> {
        workspace_symbols::workspace_symbols(self, query, limit)
    }

    /// Range and current text of the symbol at `offset`, if it can be renamed.
    pub fn prepare_rename(&self, id: FileId, offset: TextSize) -> Result<(TextRange, String), RenameError> {
        let sema = sema::Sema::new(self, id).ok_or_else(|| RenameError("unknown file".into()))?;
        rename::prepare_rename(&sema, offset)
    }

    pub fn rename(&self, id: FileId, offset: TextSize, new_name: &str) -> Result<SourceChange, RenameError> {
        let sema = sema::Sema::new(self, id).ok_or_else(|| RenameError("unknown file".into()))?;
        rename::rename(&sema, offset, new_name)
    }

    pub fn prepare_call_hierarchy(&self, id: FileId, offset: TextSize) -> Vec<HierarchyItem> {
        sema::Sema::new(self, id).map(|s| hierarchy::prepare_call_hierarchy(&s, offset)).unwrap_or_default()
    }

    pub fn incoming_calls(&self, item: &HierarchyItem) -> Vec<HierarchyCall> {
        hierarchy::incoming_calls(self, item)
    }

    pub fn outgoing_calls(&self, item: &HierarchyItem) -> Vec<HierarchyCall> {
        hierarchy::outgoing_calls(self, item)
    }

    pub fn prepare_type_hierarchy(&self, id: FileId, offset: TextSize) -> Vec<HierarchyItem> {
        sema::Sema::new(self, id).map(|s| hierarchy::prepare_type_hierarchy(&s, offset)).unwrap_or_default()
    }

    pub fn supertypes(&self, item: &HierarchyItem) -> Vec<HierarchyItem> {
        hierarchy::supertypes(self, item)
    }

    pub fn subtypes(&self, item: &HierarchyItem) -> Vec<HierarchyItem> {
        hierarchy::subtypes(self, item)
    }

    /// Code lenses of a file; titles are computed by [`Workspace::resolve_code_lens`].
    pub fn code_lenses(&self, id: FileId) -> Vec<CodeLens> {
        sema::Sema::new(self, id).map(|s| hierarchy::code_lenses(&s)).unwrap_or_default()
    }

    pub fn resolve_code_lens(&self, id: FileId, lens: &CodeLens) -> Option<ResolvedCodeLens> {
        hierarchy::resolve_code_lens(&sema::Sema::new(self, id)?, lens)
    }

    pub fn code_actions(&self, id: FileId, range: TextRange) -> Vec<CodeAction> {
        sema::Sema::new(self, id).map(|s| code_actions::code_actions(&s, range)).unwrap_or_default()
    }

    /// Whitespace edits formatting the file (`None` if it has syntax errors).
    pub fn format(
        &self,
        id: FileId,
        range: Option<TextRange>,
        options: &FormatOptions,
    ) -> Option<Vec<TextEdit>> {
        let file = self.file(id)?;
        if twig::is_twig(&file.path) {
            return Some(Vec::new());
        }
        let mut options = options.clone();
        options.php_version = self.config.php_version;
        if blade::is_blade(&file.path) {
            // only the code of `@php` blocks; the template stays as it is
            let edits = blade::format(&file.text, range, &options);
            return Some(edits.into_iter().map(|(range, new_text)| TextEdit { range, new_text }).collect());
        }
        let edits = match range {
            Some(r) => php_fmt::format_range(&file.text, r, &options),
            None => php_fmt::format(&file.text, &options),
        }
        .ok()?;
        Some(edits.into_iter().map(|e| TextEdit { range: e.range, new_text: e.new_text }).collect())
    }

    pub fn document_links(&self, id: FileId) -> Vec<DocumentLink> {
        if self.twig_file(id).is_some() {
            return twig::document_links(self, id);
        }
        sema::Sema::new(self, id).map(|s| links::document_links(&s)).unwrap_or_default()
    }

    /// Edits to apply when the file `id` is renamed / moved to `new_path`
    /// (a `file://` key): class name and PSR-4 namespace follow the file.
    pub fn file_rename_edits(&self, id: FileId, new_path: &str) -> SourceChange {
        sema::Sema::new(self, id).map(|s| links::file_rename_edits(&s, new_path)).unwrap_or_default()
    }

    pub fn inlay_hints(&self, id: FileId, range: TextRange) -> Vec<InlayHint> {
        sema::Sema::new(self, id).map(|s| inlay_hints::inlay_hints(&s, range)).unwrap_or_default()
    }

    /// Semantic tokens of a file (optionally limited to `range`).
    pub fn semantic_tokens(&self, id: FileId, range: Option<TextRange>) -> Vec<SemanticToken> {
        if let Some(f) = self.twig_file(id) {
            return twig::tokens(&f.text, range);
        }
        let mut tokens = sema::Sema::new(self, id)
            .map(|s| semantic_tokens::semantic_tokens(&s, range))
            .unwrap_or_default();
        if let Some(f) = self.file(id).filter(|f| blade::is_blade(&f.path)) {
            let b = blade::virtual_php(&f.text);
            tokens.extend(
                blade::tokens(&b).into_iter().filter(|t| range.is_none_or(|r| r.contains_range(t.range))),
            );
            tokens.sort_by_key(|t| t.range.start());
        }
        tokens
    }

    /// Definition targets of the string literal at `offset` contributed by
    /// plugins (config keys, views, routes, hooks, ...): `(path key, line)`.
    pub fn string_targets(&self, id: FileId, offset: TextSize) -> Vec<(String, u32)> {
        if self.twig_file(id).is_some() {
            return twig::string_targets(self, id, offset);
        }
        let Some(sema) = sema::Sema::new(self, id) else { return Vec::new() };
        let Some((arg, _)) = completion::string_arg_at(&sema, offset) else { return Vec::new() };
        let cx = plugins::PluginCx { ws: self, file: id };
        self.plugins
            .0
            .iter()
            .flat_map(|p| p.string_candidates(&cx, &arg))
            .filter(|c| c.value == arg.value)
            .filter_map(|c| c.location)
            .collect()
    }

    /// The inferred type of the expression at `offset`, rendered as text.
    pub fn type_at(&self, id: FileId, offset: TextSize) -> Option<String> {
        let sema = sema::Sema::new(self, id)?;
        let token = sema.token_at(offset)?;
        let node = token.parent()?;
        let expr = node.ancestors().find(|n| infer::is_expr(n.kind()))?;
        Some(sema.infer().expr_type(&expr).to_string())
    }

    /// Debug dump of the syntax tree.
    pub fn syntax_tree(&self, id: FileId) -> Option<String> {
        self.parse(id).map(|p| p.debug_tree())
    }
}
