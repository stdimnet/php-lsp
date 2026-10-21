//! Call hierarchy, type hierarchy and code lenses.

use php_index::FileId;
use php_syntax::ast::ClassLikeKind;
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::classify::{classify, classify_token, Definition};
use crate::navigation::{references, ReferenceKind};
use crate::sema::Sema;
use crate::{SymbolKind, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyItem {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    pub file: FileId,
    pub range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyCall {
    pub item: HierarchyItem,
    /// Ranges of the calls (in the caller for incoming, in `from` for outgoing).
    pub ranges: Vec<TextRange>,
}

fn item_for(ws: &Workspace, def: &Definition) -> Option<HierarchyItem> {
    let index = ws.index();
    match def {
        Definition::Function(fqn) => {
            let (file, f) = index.function(fqn)?;
            Some(HierarchyItem {
                name: f.name.clone(),
                kind: SymbolKind::Function,
                detail: f.fqn.namespace().map(str::to_string),
                file,
                range: f.range,
                selection_range: f.name_range,
            })
        }
        Definition::Method { class, name } => {
            let m = index.find_method(class, name)?;
            Some(HierarchyItem {
                name: m.def.name.clone(),
                kind: if m.def.name.eq_ignore_ascii_case("__construct") {
                    SymbolKind::Constructor
                } else {
                    SymbolKind::Method
                },
                detail: Some(m.owner.fqn.to_string()),
                file: m.file,
                range: m.def.range,
                selection_range: m.def.name_range,
            })
        }
        Definition::Class(fqn) => {
            let (file, c) = index.class(fqn)?;
            Some(HierarchyItem {
                name: c.fqn.short().to_string(),
                kind: match c.kind {
                    ClassLikeKind::Class => SymbolKind::Class,
                    ClassLikeKind::Interface => SymbolKind::Interface,
                    ClassLikeKind::Trait => SymbolKind::Trait,
                    ClassLikeKind::Enum => SymbolKind::Enum,
                },
                detail: c.fqn.namespace().map(str::to_string),
                file,
                range: c.range,
                selection_range: c.name_range,
            })
        }
        _ => None,
    }
}

/// The definition an item stands for (re-classified at its name).
fn item_definition(ws: &Workspace, item: &HierarchyItem) -> Option<Definition> {
    let sema = Sema::new(ws, item.file)?;
    classify(&sema, item.selection_range.start()).map(|(d, _)| d)
}

pub(crate) fn prepare_call_hierarchy(sema: &Sema, offset: TextSize) -> Vec<HierarchyItem> {
    match classify(sema, offset) {
        Some((d @ (Definition::Function(_) | Definition::Method { .. }), _)) => {
            item_for(sema.ws, &d).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// The function / method declaration enclosing `node`.
fn enclosing_callable(sema: &Sema, node: &SyntaxNode) -> Option<Definition> {
    let decl = node.ancestors().find(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL))?;
    let name = decl.children().find(|c| matches!(c.kind(), NAME | MEMBER_NAME))?;
    let token = name.first_token()?;
    classify_token(sema, &token)
}

pub(crate) fn incoming_calls(ws: &Workspace, item: &HierarchyItem) -> Vec<HierarchyCall> {
    let Some(sema) = Sema::new(ws, item.file) else { return Vec::new() };
    let refs = references(&sema, item.selection_range.start(), false);
    let mut out: Vec<HierarchyCall> = Vec::new();
    for r in refs.into_iter().filter(|r| r.kind != ReferenceKind::Declaration) {
        let Some(s) = Sema::new(ws, r.file) else { continue };
        let Some(token) = s.token_at(r.range.start()) else { continue };
        let Some(node) = token.parent() else { continue };
        let caller = match enclosing_callable(&s, &node).and_then(|d| item_for(ws, &d)) {
            Some(c) => c,
            // top level code: the file itself
            None => HierarchyItem {
                name: ws
                    .file(r.file)
                    .map(|f| f.path.rsplit('/').next().unwrap_or("").to_string())
                    .unwrap_or_default(),
                kind: SymbolKind::Namespace,
                detail: None,
                file: r.file,
                range: s.root.text_range(),
                selection_range: TextRange::empty(TextSize::from(0)),
            },
        };
        match out.iter_mut().find(|c| c.item == caller) {
            Some(c) => c.ranges.push(r.range),
            None => out.push(HierarchyCall { item: caller, ranges: vec![r.range] }),
        }
    }
    out
}

pub(crate) fn outgoing_calls(ws: &Workspace, item: &HierarchyItem) -> Vec<HierarchyCall> {
    let Some(sema) = Sema::new(ws, item.file) else { return Vec::new() };
    let Some(decl) = sema.root.descendants().find(|n| n.text_range() == item.range) else {
        return Vec::new();
    };
    let mut out: Vec<HierarchyCall> = Vec::new();
    for node in decl.descendants() {
        let name_token = match node.kind() {
            CALL_EXPR => node.first_child().filter(|c| c.kind() == NAME_EXPR).and_then(|c| c.last_token()),
            METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
                node.children().find(|c| c.kind() == MEMBER_NAME).and_then(|c| c.first_token())
            }
            NEW_EXPR => node.children().find(|c| c.kind() == NAME_EXPR).and_then(|c| c.last_token()),
            _ => None,
        };
        let Some(token) = name_token else { continue };
        let def = match classify_token(&sema, &token) {
            Some(Definition::Class(fqn)) => Definition::Method { class: fqn, name: "__construct".into() },
            Some(d) => d,
            None => continue,
        };
        let Some(callee) = item_for(ws, &def) else { continue };
        match out.iter_mut().find(|c| c.item == callee) {
            Some(c) => c.ranges.push(token.text_range()),
            None => out.push(HierarchyCall { item: callee, ranges: vec![token.text_range()] }),
        }
    }
    out
}

pub(crate) fn prepare_type_hierarchy(sema: &Sema, offset: TextSize) -> Vec<HierarchyItem> {
    match classify(sema, offset) {
        Some((d @ Definition::Class(_), _)) => item_for(sema.ws, &d).into_iter().collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn supertypes(ws: &Workspace, item: &HierarchyItem) -> Vec<HierarchyItem> {
    let Some(Definition::Class(fqn)) = item_definition(ws, item) else { return Vec::new() };
    let Some((_, c)) = ws.index().class(&fqn) else { return Vec::new() };
    c.supertypes().filter_map(|s| item_for(ws, &Definition::Class(s.clone()))).collect()
}

pub(crate) fn subtypes(ws: &Workspace, item: &HierarchyItem) -> Vec<HierarchyItem> {
    let Some(Definition::Class(fqn)) = item_definition(ws, item) else { return Vec::new() };
    ws.index()
        .direct_subtypes(&fqn)
        .into_iter()
        .filter_map(|(_, c)| item_for(ws, &Definition::Class(c.fqn.clone())))
        .collect()
}

// ------------------------------------------------------------------ code lens

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeLensKind {
    References,
    Implementations,
    /// The method overrides / implements a parent method.
    Parent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLens {
    pub range: TextRange,
    pub kind: CodeLensKind,
}

/// The resolved title and targets of a lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCodeLens {
    pub title: String,
    pub targets: Vec<crate::Location>,
}

pub(crate) fn code_lenses(sema: &Sema) -> Vec<CodeLens> {
    let mut out = Vec::new();
    for node in sema.root.descendants() {
        let kind = node.kind();
        if !matches!(kind, CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | FUNCTION_DECL | METHOD_DECL)
        {
            continue;
        }
        let Some(name) = node.children().find(|c| matches!(c.kind(), NAME | MEMBER_NAME)) else { continue };
        let range = name.text_range();
        out.push(CodeLens { range, kind: CodeLensKind::References });
        if matches!(kind, INTERFACE_DECL)
            || kind == CLASS_DECL
                && node.children().any(|c| {
                    c.kind() == MODIFIER_LIST && c.children_with_tokens().any(|t| t.kind() == ABSTRACT_KW)
                })
        {
            out.push(CodeLens { range, kind: CodeLensKind::Implementations });
        }
        if kind == METHOD_DECL {
            out.push(CodeLens { range, kind: CodeLensKind::Parent });
        }
    }
    out
}

pub(crate) fn resolve_code_lens(sema: &Sema, lens: &CodeLens) -> Option<ResolvedCodeLens> {
    let offset = lens.range.start();
    match lens.kind {
        CodeLensKind::References => {
            let refs = references(sema, offset, false);
            let n = refs.len();
            Some(ResolvedCodeLens {
                title: format!("{n} reference{}", if n == 1 { "" } else { "s" }),
                targets: refs
                    .into_iter()
                    .map(|r| crate::Location { file: r.file, range: r.range, focus: r.range })
                    .collect(),
            })
        }
        CodeLensKind::Implementations => {
            let impls = crate::navigation::goto_implementation(sema, offset);
            let n = impls.len();
            Some(ResolvedCodeLens {
                title: format!("{n} implementation{}", if n == 1 { "" } else { "s" }),
                targets: impls,
            })
        }
        CodeLensKind::Parent => {
            let (Definition::Method { class, name }, _) = classify(sema, offset)? else { return None };
            let index = sema.index();
            let (_, c) = index.class(&class)?;
            // the first ancestor (after the class itself) declaring the method
            let parent = index
                .ancestors_with(&class, false)
                .into_iter()
                .skip(1)
                .find(|(_, a)| a.method(&name).is_some() && !a.fqn.eq_ignore_case(&c.fqn))?;
            let m = parent.1.method(&name)?;
            let verb = if m.is_abstract || parent.1.kind == ClassLikeKind::Interface {
                "implements"
            } else {
                "overrides"
            };
            Some(ResolvedCodeLens {
                title: format!("{verb} {}::{}", parent.1.fqn.short(), m.name),
                targets: vec![crate::Location { file: parent.0, range: m.range, focus: m.name_range }],
            })
        }
    }
}
