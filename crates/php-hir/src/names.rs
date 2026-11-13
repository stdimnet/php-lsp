//! Name resolution: namespaces and `use` imports.

use std::collections::HashMap;

use php_syntax::ast::{self, AstNode, UseKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::types::{builtin_type, Fqn, Type};

/// An import introduced by a `use` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub kind: UseKind,
    /// The local alias (`Bar` in `use Foo\Bar` or `use Foo as Bar`).
    pub alias: String,
    pub target: Fqn,
    /// Range of the whole `use` clause.
    pub range: TextRange,
}

/// One namespace region of a file with its imports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceScope {
    /// `None` for the global namespace.
    pub namespace: Option<Fqn>,
    pub range: TextRange,
    pub imports: Vec<Import>,
    classes: HashMap<String, Fqn>,
    functions: HashMap<String, Fqn>,
    constants: HashMap<String, Fqn>,
    /// Where new `use` statements should be inserted.
    pub use_insert_offset: TextSize,
}

/// Result of resolving a function or constant name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedName {
    pub name: Fqn,
    /// Global fallback for unqualified names inside a namespace.
    pub fallback: Option<Fqn>,
}

impl NamespaceScope {
    pub fn namespace_str(&self) -> &str {
        self.namespace.as_ref().map(|n| n.as_str()).unwrap_or("")
    }

    /// Resolves a class name as written in source (`Foo`, `\Foo`, `Foo\Bar`,
    /// `namespace\Foo`). Does not handle `self`/`static`/`parent`.
    pub fn resolve_class(&self, name: &str) -> Fqn {
        if let Some(rest) = name.strip_prefix('\\') {
            return Fqn::new(rest);
        }
        if let Some(rest) = strip_namespace_prefix(name) {
            return Fqn::join(self.namespace.as_ref(), rest);
        }
        let (first, rest) = match name.split_once('\\') {
            Some((f, r)) => (f, Some(r)),
            None => (name, None),
        };
        if let Some(target) = self.classes.get(&first.to_ascii_lowercase()) {
            return match rest {
                Some(r) => Fqn::new(&format!("{}\\{}", target, r)),
                None => target.clone(),
            };
        }
        Fqn::join(self.namespace.as_ref(), name)
    }

    pub fn resolve_function(&self, name: &str) -> ResolvedName {
        self.resolve_fn_or_const(name, &self.functions, true)
    }

    pub fn resolve_constant(&self, name: &str) -> ResolvedName {
        self.resolve_fn_or_const(name, &self.constants, false)
    }

    fn resolve_fn_or_const(
        &self,
        name: &str,
        map: &HashMap<String, Fqn>,
        case_insensitive: bool,
    ) -> ResolvedName {
        if let Some(rest) = name.strip_prefix('\\') {
            return ResolvedName { name: Fqn::new(rest), fallback: None };
        }
        if let Some(rest) = strip_namespace_prefix(name) {
            return ResolvedName { name: Fqn::join(self.namespace.as_ref(), rest), fallback: None };
        }
        if name.contains('\\') {
            // qualified: first segment is resolved via class imports
            let (first, rest) = name.split_once('\\').unwrap();
            if let Some(target) = self.classes.get(&first.to_ascii_lowercase()) {
                return ResolvedName { name: Fqn::new(&format!("{target}\\{rest}")), fallback: None };
            }
            return ResolvedName { name: Fqn::join(self.namespace.as_ref(), name), fallback: None };
        }
        let key = if case_insensitive { name.to_ascii_lowercase() } else { name.to_string() };
        if let Some(target) = map.get(&key) {
            return ResolvedName { name: target.clone(), fallback: None };
        }
        match &self.namespace {
            Some(ns) => ResolvedName { name: Fqn::join(Some(ns), name), fallback: Some(Fqn::new(name)) },
            None => ResolvedName { name: Fqn::new(name), fallback: None },
        }
    }

    /// Resolves a type name used in a declaration or PHPDoc.
    pub fn resolve_type_name(&self, name: &str) -> Type {
        match builtin_type(name) {
            Some(t) => t,
            None => Type::Named(self.resolve_class(name), Vec::new()),
        }
    }

    /// The shortest name under which `fqn` can be referenced in this scope,
    /// if it is reachable without a new import.
    pub fn shortest_class_name(&self, fqn: &Fqn) -> Option<String> {
        for import in &self.imports {
            if import.kind == UseKind::Class && import.target.eq_ignore_case(fqn) {
                return Some(import.alias.clone());
            }
        }
        let ns = self.namespace_str();
        match fqn.namespace() {
            Some(n) if n.eq_ignore_ascii_case(ns) => {
                // same namespace, but an import with the same alias would shadow it
                let short = fqn.short();
                if self.classes.contains_key(&short.to_ascii_lowercase()) {
                    None
                } else {
                    Some(short.to_string())
                }
            }
            None if ns.is_empty() => Some(fqn.as_str().to_string()),
            _ => None,
        }
    }

    pub fn alias_taken(&self, alias: &str) -> bool {
        self.classes.contains_key(&alias.to_ascii_lowercase())
    }
}

fn strip_namespace_prefix(name: &str) -> Option<&str> {
    if name.len() > 10 && name[..10].eq_ignore_ascii_case("namespace\\") {
        Some(&name[10..])
    } else {
        None
    }
}

/// All namespace scopes of a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileScopes {
    pub scopes: Vec<NamespaceScope>,
}

impl FileScopes {
    pub fn build(file: &ast::SourceFile) -> FileScopes {
        let root = file.syntax();
        let mut scopes = Vec::new();
        let mut current = NamespaceScope {
            namespace: None,
            range: root.text_range(),
            use_insert_offset: after_open_tag(root),
            ..Default::default()
        };
        let mut has_namespace = false;
        for stmt in root.children() {
            match stmt.kind() {
                NAMESPACE_DECL => {
                    let ns = ast::NamespaceDecl::cast(stmt.clone()).unwrap();
                    let name = ns.name().map(|n| Fqn::new(&n.text())).filter(|n| !n.as_str().is_empty());
                    if let Some(body) = ns.body() {
                        // braced namespace: self-contained scope
                        let mut scope = NamespaceScope {
                            namespace: name,
                            range: stmt.text_range(),
                            use_insert_offset: body
                                .syntax()
                                .first_token()
                                .map(|t| t.text_range().end())
                                .unwrap_or(stmt.text_range().end()),
                            ..Default::default()
                        };
                        for s in body.syntax().children() {
                            if s.kind() == USE_DECL {
                                add_uses(&mut scope, &s);
                            }
                        }
                        scopes.push(scope);
                        has_namespace = true;
                        continue;
                    }
                    if has_namespace || !current.imports.is_empty() || current.namespace.is_some() {
                        let mut prev = std::mem::take(&mut current);
                        prev.range = TextRange::new(prev.range.start(), stmt.text_range().start());
                        scopes.push(prev);
                    }
                    has_namespace = true;
                    current = NamespaceScope {
                        namespace: name,
                        range: TextRange::new(stmt.text_range().start(), root.text_range().end()),
                        use_insert_offset: stmt.text_range().end(),
                        ..Default::default()
                    };
                }
                USE_DECL => {
                    add_uses(&mut current, &stmt);
                    current.use_insert_offset = current.use_insert_offset.max(stmt.text_range().end());
                }
                _ => {}
            }
        }
        if !scopes.iter().any(|s| s.range == current.range) {
            scopes.push(current);
        }
        FileScopes { scopes }
    }

    /// The scope containing `offset` (falls back to the last scope).
    pub fn at(&self, offset: TextSize) -> &NamespaceScope {
        self.scopes
            .iter()
            .filter(|s| s.range.contains_inclusive(offset))
            .min_by_key(|s| s.range.len())
            .or(self.scopes.last())
            .expect("there is always at least one scope")
    }
}

fn after_open_tag(root: &SyntaxNode) -> TextSize {
    root.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == OPEN_TAG)
        .map(|t| t.text_range().end())
        .unwrap_or_default()
}

fn add_uses(scope: &mut NamespaceScope, node: &SyntaxNode) {
    let decl = ast::UseDecl::cast(node.clone()).unwrap();
    let kind = decl.kind();
    for clause in decl.clauses() {
        let Some(name) = clause.name() else { continue };
        let prefix = name.text();
        if let Some(group) = clause.group() {
            for inner in group.clauses() {
                let Some(n) = inner.name() else { continue };
                let full = format!("{}\\{}", prefix.trim_start_matches('\\'), n.text());
                let kind = inner.own_kind().unwrap_or(kind);
                add_import(scope, kind, &full, inner.alias().map(|a| a.text()), inner.syntax().text_range());
            }
        } else {
            add_import(scope, kind, &prefix, clause.alias().map(|a| a.text()), clause.syntax().text_range());
        }
    }
}

fn add_import(
    scope: &mut NamespaceScope,
    kind: UseKind,
    target: &str,
    alias: Option<String>,
    range: TextRange,
) {
    let target = Fqn::new(target);
    let alias = alias.unwrap_or_else(|| target.short().to_string());
    match kind {
        UseKind::Class => scope.classes.insert(alias.to_ascii_lowercase(), target.clone()),
        UseKind::Function => scope.functions.insert(alias.to_ascii_lowercase(), target.clone()),
        UseKind::Const => scope.constants.insert(alias.clone(), target.clone()),
    };
    scope.imports.push(Import { kind, alias, target, range });
}

#[cfg(test)]
mod tests {
    use super::*;
    use php_syntax::{parse, PhpVersion};

    #[test]
    fn resolution() {
        let text = "<?php namespace App\\Http; use Foo\\Bar, Baz\\Qux as Q; use function str\\fmt; use Grp\\{A, B as BB};";
        let file = parse(text, PhpVersion::LATEST).tree();
        let scopes = FileScopes::build(&file);
        let s = scopes.at(TextSize::from(text.len() as u32));
        assert_eq!(s.namespace_str(), "App\\Http");
        assert_eq!(s.resolve_class("Bar").as_str(), "Foo\\Bar");
        assert_eq!(s.resolve_class("q\\Inner").as_str(), "Baz\\Qux\\Inner");
        assert_eq!(s.resolve_class("Local").as_str(), "App\\Http\\Local");
        assert_eq!(s.resolve_class("\\Root").as_str(), "Root");
        assert_eq!(s.resolve_class("namespace\\Sub\\X").as_str(), "App\\Http\\Sub\\X");
        assert_eq!(s.resolve_class("BB").as_str(), "Grp\\B");
        let f = s.resolve_function("strlen");
        assert_eq!(f.name.as_str(), "App\\Http\\strlen");
        assert_eq!(f.fallback.unwrap().as_str(), "strlen");
        assert_eq!(s.resolve_function("fmt").name.as_str(), "str\\fmt");
        assert_eq!(s.shortest_class_name(&Fqn::new("Foo\\Bar")).as_deref(), Some("Bar"));
        assert_eq!(s.shortest_class_name(&Fqn::new("App\\Http\\Ctl")).as_deref(), Some("Ctl"));
        assert_eq!(s.shortest_class_name(&Fqn::new("Other\\Ctl")), None);
    }

    #[test]
    fn multiple_namespaces() {
        let text = "<?php namespace A; use X\\Y; class C {} namespace B; class D {}";
        let file = parse(text, PhpVersion::LATEST).tree();
        let scopes = FileScopes::build(&file);
        assert_eq!(scopes.scopes.len(), 2);
        let c = TextSize::from(text.find("class C").unwrap() as u32);
        let d = TextSize::from(text.find("class D").unwrap() as u32);
        assert_eq!(scopes.at(c).namespace_str(), "A");
        assert_eq!(scopes.at(d).namespace_str(), "B");
        assert_eq!(scopes.at(d).resolve_class("Y").as_str(), "B\\Y");
    }
}
