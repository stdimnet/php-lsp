//! Semantic model of PHP source files.
//!
//! * [`names`]: namespace scopes and `use` imports
//! * [`phpdoc`]: doc blocks, tags and the PHPDoc type language
//! * [`types`]: the type model
//! * [`symbols`]: declared classes, functions and constants

pub mod names;
pub mod phpdoc;
pub mod symbols;
pub mod types;

pub use names::{FileScopes, Import, NamespaceScope, ResolvedName};
pub use phpdoc::DocBlock;
pub use symbols::{
    extract, ClassConstDef, ClassDef, ConstDef, EnumCaseDef, ExtractOptions, FileSymbols, FunctionDef,
    ParamDef, PropertyDef, TypeResolver, Visibility,
};
pub use types::{Fqn, Type};

/// Parses nothing itself: computes scopes and symbols for an already parsed file.
pub fn analyze_file(file: &php_syntax::ast::SourceFile, opts: ExtractOptions) -> (FileScopes, FileSymbols) {
    let scopes = FileScopes::build(file);
    let symbols = extract(file, &scopes, opts);
    (scopes, symbols)
}
