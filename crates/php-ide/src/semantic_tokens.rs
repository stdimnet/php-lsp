//! Semantic tokens (semantic highlighting).

use php_index::FileKind;
use php_syntax::ast::ClassLikeKind;
use php_syntax::{SyntaxKind::*, SyntaxToken, TextRange};

use crate::classify::{classify_token, Definition};
use crate::sema::Sema;

/// Token types, in the order of [`TOKEN_TYPES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SemanticTokenType {
    Namespace,
    Class,
    Interface,
    Enum,
    Type,
    Function,
    Method,
    Property,
    EnumMember,
    Variable,
    Parameter,
    Decorator,
}

/// LSP names of the token types.
pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "class",
    "interface",
    "enum",
    "type",
    "function",
    "method",
    "property",
    "enumMember",
    "variable",
    "parameter",
    "decorator",
];

/// LSP names of the modifiers (bit index = position).
pub const TOKEN_MODIFIERS: &[&str] =
    &["declaration", "static", "readonly", "deprecated", "abstract", "defaultLibrary"];

pub const MOD_DECLARATION: u32 = 1 << 0;
pub const MOD_STATIC: u32 = 1 << 1;
pub const MOD_READONLY: u32 = 1 << 2;
pub const MOD_DEPRECATED: u32 = 1 << 3;
pub const MOD_ABSTRACT: u32 = 1 << 4;
pub const MOD_DEFAULT_LIBRARY: u32 = 1 << 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub range: TextRange,
    pub token_type: SemanticTokenType,
    pub modifiers: u32,
}

fn is_declaration(token: &SyntaxToken) -> bool {
    let Some(parent) = token.parent() else { return false };
    match parent.kind() {
        NAME | MEMBER_NAME => parent.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                CLASS_DECL
                    | INTERFACE_DECL
                    | TRAIT_DECL
                    | ENUM_DECL
                    | FUNCTION_DECL
                    | METHOD_DECL
                    | CONST_ELEMENT
                    | ENUM_CASE
            )
        }),
        PROPERTY_ELEMENT | PARAM => true,
        _ => false,
    }
}

pub(crate) fn semantic_tokens(sema: &Sema, range: Option<TextRange>) -> Vec<SemanticToken> {
    let index = sema.index();
    let mut out = Vec::new();
    for token in sema.root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if !matches!(token.kind(), IDENT | VARIABLE) {
            continue;
        }
        if range.is_some_and(|r| !r.contains_range(token.text_range())) {
            continue;
        }
        let Some(def) = classify_token(sema, &token) else { continue };
        let mut mods = if is_declaration(&token) { MOD_DECLARATION } else { 0 };
        let stub = |file| index.file_kind(file) == Some(FileKind::Stub);
        let in_attribute = token.parent_ancestors().any(|a| a.kind() == ATTRIBUTE);
        let ty = match &def {
            Definition::Namespace(_) => SemanticTokenType::Namespace,
            Definition::Class(fqn) => {
                if token.text().eq_ignore_ascii_case("self")
                    || token.text().eq_ignore_ascii_case("static")
                    || token.text().eq_ignore_ascii_case("parent")
                {
                    continue; // keywords
                }
                if token.kind() == VARIABLE {
                    continue; // $this
                }
                match index.class(fqn) {
                    Some((file, c)) => {
                        if stub(file) {
                            mods |= MOD_DEFAULT_LIBRARY;
                        }
                        if c.deprecated.is_some() {
                            mods |= MOD_DEPRECATED;
                        }
                        if c.is_abstract {
                            mods |= MOD_ABSTRACT;
                        }
                        if in_attribute {
                            SemanticTokenType::Decorator
                        } else {
                            match c.kind {
                                ClassLikeKind::Class => SemanticTokenType::Class,
                                ClassLikeKind::Interface => SemanticTokenType::Interface,
                                ClassLikeKind::Enum => SemanticTokenType::Enum,
                                ClassLikeKind::Trait => SemanticTokenType::Type,
                            }
                        }
                    }
                    None => SemanticTokenType::Class,
                }
            }
            Definition::Function(fqn) => {
                if let Some((file, f)) = index.function(fqn) {
                    if stub(file) {
                        mods |= MOD_DEFAULT_LIBRARY;
                    }
                    if f.deprecated.is_some() {
                        mods |= MOD_DEPRECATED;
                    }
                }
                SemanticTokenType::Function
            }
            Definition::Constant(fqn) => {
                mods |= MOD_READONLY;
                if let Some((file, _)) = index.constant(fqn) {
                    if stub(file) {
                        mods |= MOD_DEFAULT_LIBRARY;
                    }
                }
                SemanticTokenType::Variable
            }
            Definition::Method { class, name } => {
                if let Some(m) = index.find_method(class, name) {
                    if m.def.is_static {
                        mods |= MOD_STATIC;
                    }
                    if m.def.is_abstract {
                        mods |= MOD_ABSTRACT;
                    }
                    if m.def.deprecated.is_some() {
                        mods |= MOD_DEPRECATED;
                    }
                    if stub(m.file) {
                        mods |= MOD_DEFAULT_LIBRARY;
                    }
                }
                SemanticTokenType::Method
            }
            Definition::Property { class, name } => {
                if let Some(p) = index.find_property(class, name) {
                    if p.def.is_static {
                        mods |= MOD_STATIC;
                    }
                    if p.def.is_readonly {
                        mods |= MOD_READONLY;
                    }
                    if p.def.deprecated.is_some() {
                        mods |= MOD_DEPRECATED;
                    }
                }
                SemanticTokenType::Property
            }
            Definition::ClassConstant { class, name } => {
                mods |= MOD_READONLY | MOD_STATIC;
                if index.find_class_constant(class, name).is_some_and(|c| c.def.deprecated.is_some()) {
                    mods |= MOD_DEPRECATED;
                }
                SemanticTokenType::Property
            }
            Definition::EnumCase { .. } => SemanticTokenType::EnumMember,
            Definition::Variable { .. } => {
                let is_param = token.parent().is_some_and(|p| p.kind() == PARAM);
                if is_param {
                    SemanticTokenType::Parameter
                } else {
                    SemanticTokenType::Variable
                }
            }
        };
        out.push(SemanticToken { range: token.text_range(), token_type: ty, modifiers: mods });
    }
    out
}
