//! Workspace symbol search.

use php_index::{FileId, FileKind};
use php_syntax::ast::ClassLikeKind;
use php_syntax::TextRange;

use crate::{SymbolKind, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub file: FileId,
    pub range: TextRange,
    pub deprecated: bool,
}

/// Fuzzy score: `None` if `query` is not a subsequence of `name`.
fn score(name: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let name_l = name.to_ascii_lowercase();
    let query_l = query.to_ascii_lowercase();
    if name_l == query_l {
        return Some(1000);
    }
    if name_l.starts_with(&query_l) {
        return Some(800 - name.len().min(200) as u32);
    }
    if name_l.contains(&query_l) {
        return Some(500 - name.len().min(200) as u32);
    }
    let mut it = name_l.chars();
    for q in query_l.chars() {
        it.find(|c| *c == q)?;
    }
    Some(100)
}

pub(crate) fn workspace_symbols(ws: &Workspace, query: &str, limit: usize) -> Vec<WorkspaceSymbol> {
    let mut out: Vec<(u32, WorkspaceSymbol)> = Vec::new();
    let index = ws.index();
    // members are only searched with `Class::member` or a non-empty query
    let (class_query, member_query) = match query.split_once("::") {
        Some((c, m)) => (c, Some(m)),
        None => (query, None),
    };
    for (file, kind, symbols) in index.files() {
        if kind == FileKind::Stub && query.len() < 3 {
            continue;
        }
        for c in symbols.classes.iter().filter(|c| !php_hir::symbols::is_anon_class(&c.fqn)) {
            let class_score = score(c.fqn.short(), class_query);
            if member_query.is_none() {
                if let Some(s) = class_score {
                    out.push((
                        s + rank(kind),
                        WorkspaceSymbol {
                            name: c.fqn.short().to_string(),
                            kind: match c.kind {
                                ClassLikeKind::Class => SymbolKind::Class,
                                ClassLikeKind::Interface => SymbolKind::Interface,
                                ClassLikeKind::Trait => SymbolKind::Trait,
                                ClassLikeKind::Enum => SymbolKind::Enum,
                            },
                            container: c.fqn.namespace().map(str::to_string),
                            file,
                            range: c.name_range,
                            deprecated: c.deprecated.is_some(),
                        },
                    ));
                }
            }
            if query.len() < 2 || kind == FileKind::Stub {
                continue;
            }
            let mq = member_query.unwrap_or(query);
            if member_query.is_some() && class_score.is_none() {
                continue;
            }
            let mut push = |name: &str, kind_: SymbolKind, range: TextRange, deprecated: bool| {
                if let Some(s) = score(name, mq) {
                    out.push((
                        s / 2 + rank(kind),
                        WorkspaceSymbol {
                            name: name.to_string(),
                            kind: kind_,
                            container: Some(c.fqn.to_string()),
                            file,
                            range,
                            deprecated,
                        },
                    ));
                }
            };
            for m in c.methods.iter().filter(|m| !m.is_magic) {
                push(&m.name, SymbolKind::Method, m.name_range, m.deprecated.is_some());
            }
            for p in c.properties.iter().filter(|p| p.magic.is_none()) {
                push(&format!("${}", p.name), SymbolKind::Property, p.name_range, p.deprecated.is_some());
            }
            for k in &c.constants {
                push(&k.name, SymbolKind::Constant, k.name_range, k.deprecated.is_some());
            }
            for e in &c.cases {
                push(&e.name, SymbolKind::EnumMember, e.name_range, false);
            }
        }
        if member_query.is_some() {
            continue;
        }
        for f in &symbols.functions {
            if let Some(s) = score(f.fqn.short(), query) {
                out.push((
                    s + rank(kind),
                    WorkspaceSymbol {
                        name: f.fqn.short().to_string(),
                        kind: SymbolKind::Function,
                        container: f.fqn.namespace().map(str::to_string),
                        file,
                        range: f.name_range,
                        deprecated: f.deprecated.is_some(),
                    },
                ));
            }
        }
        for c in &symbols.constants {
            if let Some(s) = score(c.fqn.short(), query) {
                out.push((
                    s + rank(kind),
                    WorkspaceSymbol {
                        name: c.fqn.short().to_string(),
                        kind: SymbolKind::Constant,
                        container: c.fqn.namespace().map(str::to_string),
                        file,
                        range: c.name_range,
                        deprecated: c.deprecated.is_some(),
                    },
                ));
            }
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    out.truncate(limit);
    out.into_iter().map(|(_, s)| s).collect()
}

fn rank(kind: FileKind) -> u32 {
    match kind {
        FileKind::Workspace => 50,
        FileKind::Vendor => 20,
        FileKind::Stub => 0,
    }
}
