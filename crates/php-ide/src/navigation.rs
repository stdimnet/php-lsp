//! Go to definition / type definition / implementation, find references and
//! document highlights.

use php_index::{FileId, FileKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::classify::{classify, classify_token, Definition};
use crate::infer::is_expr;
use crate::sema::Sema;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: FileId,
    /// The whole declaration.
    pub range: TextRange,
    /// The name of the declaration.
    pub focus: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceKind {
    Declaration,
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    pub file: FileId,
    pub range: TextRange,
    pub kind: ReferenceKind,
}

/// `$_GET` etc.: their declaration in the superglobals stub.
fn superglobal_declaration(sema: &Sema, name: &str) -> Option<Location> {
    let (id, _) = sema.ws.files().find(|(_, f)| f.path.ends_with("superglobals/_superglobals.php"))?;
    let root = sema.ws.parse(id)?.syntax_node();
    let tok = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == VARIABLE && &t.text()[1..] == name)?;
    let stmt =
        tok.parent_ancestors().find(|n| n.kind() == EXPR_STMT).map_or(tok.text_range(), |n| n.text_range());
    Some(Location { file: id, range: stmt, focus: tok.text_range() })
}

/// A variable token that (re)defines the variable: assignment target,
/// parameter, `foreach` / `catch` / `global` / `static` binding.
fn is_definition_token(t: &php_syntax::SyntaxToken) -> bool {
    let Some(var) = t.parent() else { return false };
    let Some(p) = var.parent() else { return false };
    // `[$a, $b] = ...`
    if p.kind() == ARRAY_ELEMENT {
        let mut cur = p.clone();
        while let Some(up) = cur.parent() {
            match up.kind() {
                ARRAY_EXPR | LIST_EXPR | ARRAY_ELEMENT => cur = up,
                ASSIGN_EXPR => return up.first_child().as_ref() == Some(&cur),
                FOREACH_VALUE => return true,
                _ => return false,
            }
        }
        return false;
    }
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(&var),
        PARAM | FOREACH_VALUE | FOREACH_KEY | CATCH_CLAUSE | GLOBAL_STMT | STATIC_VAR | CLOSURE_USE_VAR => {
            true
        }
        _ => matches!(var.kind(), PARAM | CATCH_CLAUSE | STATIC_VAR),
    }
}

pub(crate) fn declarations(sema: &Sema, def: &Definition, at: Option<TextSize>) -> Vec<Location> {
    let index = sema.index();
    let loc = |file, range, focus| Location { file, range, focus };
    match def {
        Definition::Class(fqn) => {
            index.classes(fqn).into_iter().map(|(f, c)| loc(f, c.range, c.name_range)).collect()
        }
        Definition::Function(fqn) => {
            index.functions(fqn).into_iter().map(|(f, d)| loc(f, d.range, d.name_range)).collect()
        }
        Definition::Constant(fqn) => {
            index.constants(fqn).into_iter().map(|(f, d)| loc(f, d.range, d.name_range)).collect()
        }
        Definition::Method { class, name } => index
            .find_method(class, name)
            .map(|m| loc(m.file, m.def.range, m.def.name_range))
            .into_iter()
            .collect(),
        Definition::Property { class, name } => index
            .find_property(class, name)
            .map(|p| loc(p.file, p.def.range, p.def.name_range))
            .into_iter()
            .collect(),
        Definition::ClassConstant { class, name } => index
            .find_class_constant(class, name)
            .map(|c| loc(c.file, c.def.range, c.def.name_range))
            .into_iter()
            .collect(),
        Definition::EnumCase { class, name } => index
            .find_case(class, name)
            .map(|c| loc(c.file, c.def.range, c.def.name_range))
            .into_iter()
            .collect(),
        Definition::Variable { name, scope } => {
            let Some(scope_node) = node_with_range(&sema.root, *scope) else { return Vec::new() };
            if crate::infer::is_superglobal(name) {
                return superglobal_declaration(sema, name).into_iter().collect();
            }
            // like Intelephense: the first definition in the scope
            let same = |t: &SyntaxToken| {
                classify_token(sema, t).is_some_and(|d| d.same_as(def))
                    // promoted parameters are properties, but also the variable
                    || t.parent().is_some_and(|p| p.kind() == PARAM)
            };
            let candidates: Vec<SyntaxToken> = variable_tokens(&scope_node, name)
                .into_iter()
                .filter(|t| at.is_none_or(|a| t.text_range().start() <= a))
                .filter(|t| same(t))
                .filter(|t| at.is_none() || is_definition_token(t))
                .collect();
            let first = candidates.first().cloned();
            // `function () use ($x)`: the definition outside
            if let Some(closure) = first
                .as_ref()
                .filter(|t| t.parent().is_some_and(|p| p.kind() == CLOSURE_USE_VAR))
                .and_then(|t| t.parent_ancestors().find(|n| n.kind() == CLOSURE_EXPR))
            {
                if let Some(outer) = closure.parent().map(|p| sema.variable_scope(&p)) {
                    let outer_def = Definition::Variable { name: name.clone(), scope: outer.text_range() };
                    let found = declarations(sema, &outer_def, Some(closure.text_range().start()));
                    if !found.is_empty() {
                        return found;
                    }
                }
            }
            let first = first.or_else(|| {
                variable_tokens(&scope_node, name)
                    .into_iter()
                    .filter(|t| at.is_none_or(|a| t.text_range().start() <= a))
                    .find(|t| classify_token(sema, t).is_some_and(|d| d.same_as(def)))
            });
            first
                .map(|t| {
                    let range = t
                        .parent()
                        .and_then(|p| {
                            p.ancestors().find(|a| {
                                matches!(
                                    a.kind(),
                                    ASSIGN_EXPR | PARAM | FOREACH_STMT | CATCH_CLAUSE | STATIC_VAR
                                )
                            })
                        })
                        .map(|n| {
                            if n.kind() == FOREACH_STMT || n.kind() == CATCH_CLAUSE {
                                t.text_range()
                            } else {
                                n.text_range()
                            }
                        })
                        .unwrap_or(t.text_range());
                    loc(sema.file, range, t.text_range())
                })
                .into_iter()
                .collect()
        }
        Definition::Namespace(_) => Vec::new(),
    }
}

/// `@see foo()`, `@see Foo::bar()`, `@see Foo::$baz`, `@see Foo::BAR` and the
/// name in `@method T name()` (its own declaration).
fn doc_reference(sema: &Sema, token: &SyntaxToken, offset: TextSize) -> Option<Vec<Location>> {
    let text = token.text();
    let base = token.text_range().start();
    let rel = usize::from(offset - base).min(text.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\\' || c == '$';
    let start =
        rel - text[..rel].chars().rev().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
    let end = rel + text[rel..].chars().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
    let word = &text[start..end];
    if word.is_empty() {
        return None;
    }
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..];
    let tag = line.trim_start_matches([' ', '\t', '*', '/']).split_whitespace().next().unwrap_or("");
    let call = text[end..].starts_with('(');
    let range = TextRange::new(base + TextSize::from(start as u32), base + TextSize::from(end as u32));
    if tag == "@method" && call {
        return Some(vec![Location { file: sema.file, range, focus: range }]);
    }
    if tag != "@see" && tag != "@uses" {
        return None;
    }
    let index = sema.index();
    let anchor = token.parent()?;
    let loc = |f: FileId, r: TextRange, n: TextRange| Location { file: f, range: r, focus: n };
    if let Some(class_end) = text[..start].strip_suffix("::").map(|h| h.len()) {
        let cstart = class_end
            - text[..class_end].chars().rev().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
        let class = sema.resolve_class_name(&text[cstart..class_end], &anchor)?;
        let found = if let Some(prop) = word.strip_prefix('$') {
            index.find_property(&class, prop).map(|p| loc(p.file, p.def.range, p.def.name_range))
        } else if call {
            index.find_method(&class, word).map(|m| loc(m.file, m.def.range, m.def.name_range))
        } else {
            index
                .find_class_constant(&class, word)
                .map(|c| loc(c.file, c.def.range, c.def.name_range))
                .or_else(|| index.find_case(&class, word).map(|c| loc(c.file, c.def.range, c.def.name_range)))
        };
        return Some(found.into_iter().collect());
    }
    if call {
        let (file, f) = sema.resolve_function(word, anchor.text_range().start())?;
        return Some(vec![loc(file, f.range, f.name_range)]);
    }
    None
}

fn node_with_range(root: &SyntaxNode, range: TextRange) -> Option<SyntaxNode> {
    root.descendants().find(|n| {
        n.text_range() == range
            && matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | PROPERTY_HOOK | SOURCE_FILE)
    })
}

fn variable_tokens(scope: &SyntaxNode, name: &str) -> Vec<SyntaxToken> {
    scope
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| {
            (t.kind() == VARIABLE && &t.text()[1..] == name)
                || (t.kind() == IDENT
                    && t.text() == name
                    && t.parent().is_some_and(|p| p.kind() == VARIABLE_EXPR))
        })
        .collect()
}

pub(crate) fn goto_definition(sema: &Sema, offset: TextSize) -> Vec<Location> {
    if let Some(t) = sema.token_at(offset) {
        // like Intelephense: no target for `$this` and the `class` of `Foo::class`
        if t.kind() == VARIABLE && t.text() == "$this" {
            return Vec::new();
        }
        if t.text().eq_ignore_ascii_case("class")
            && t.parent().and_then(|p| p.parent()).is_some_and(|f| f.kind() == CLASS_CONST_FETCH_EXPR)
        {
            return Vec::new();
        }
        if t.kind() == DOC_COMMENT {
            if let Some(locs) = doc_reference(sema, &t, offset) {
                return locs;
            }
        }
    }
    match classify(sema, offset) {
        Some((def, _)) => {
            // `$x` in a doc comment documents a parameter declared after it
            let in_doc = sema.token_at(offset).is_some_and(|t| t.kind() == DOC_COMMENT);
            let mut locs = declarations(sema, &def, if in_doc { None } else { Some(offset) });
            // for variables at their own declaration, still return it
            locs.dedup();
            locs
        }
        None => Vec::new(),
    }
}

pub(crate) fn goto_type_definition(sema: &Sema, offset: TextSize) -> Vec<Location> {
    let Some(token) = sema.token_at(offset) else { return Vec::new() };
    let Some(node) = token.parent() else { return Vec::new() };
    let ty = match node.ancestors().find(|n| is_expr(n.kind())) {
        Some(expr) => sema.infer().expr_type(&expr),
        None => match classify(sema, offset) {
            Some((Definition::Variable { name, .. }, _)) => sema.infer().variable_type(&name, &node),
            Some((d @ Definition::Class(_), _)) => return declarations(sema, &d, None),
            _ => return Vec::new(),
        },
    };
    ty.class_names().into_iter().flat_map(|c| declarations(sema, &Definition::Class(c), None)).collect()
}

pub(crate) fn goto_implementation(sema: &Sema, offset: TextSize) -> Vec<Location> {
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let index = sema.index();
    match def {
        Definition::Class(fqn) => index
            .subtypes(&fqn)
            .into_iter()
            .map(|(f, c)| Location { file: f, range: c.range, focus: c.name_range })
            .collect(),
        Definition::Method { class, name } => index
            .subtypes(&class)
            .into_iter()
            .filter_map(|(f, c)| {
                c.method(&name).map(|m| Location { file: f, range: m.range, focus: m.name_range })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn reference_kind(token: &SyntaxToken) -> ReferenceKind {
    let Some(parent) = token.parent() else { return ReferenceKind::Read };
    match parent.kind() {
        NAME => match parent.parent().map(|p| p.kind()) {
            Some(CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | FUNCTION_DECL | CONST_ELEMENT) => {
                ReferenceKind::Declaration
            }
            _ => ReferenceKind::Read,
        },
        MEMBER_NAME => match parent.parent().map(|p| p.kind()) {
            Some(METHOD_DECL | CONST_ELEMENT | ENUM_CASE) => ReferenceKind::Declaration,
            Some(PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR) => {
                if is_write_target(&parent.parent().unwrap()) {
                    ReferenceKind::Write
                } else {
                    ReferenceKind::Read
                }
            }
            _ => ReferenceKind::Read,
        },
        PROPERTY_ELEMENT | PARAM => ReferenceKind::Declaration,
        VARIABLE_EXPR => {
            if is_write_target(&parent) {
                ReferenceKind::Write
            } else {
                ReferenceKind::Read
            }
        }
        _ => ReferenceKind::Read,
    }
}

fn is_write_target(expr: &SyntaxNode) -> bool {
    let Some(p) = expr.parent() else { return false };
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(expr),
        FOREACH_VALUE | FOREACH_KEY | STATIC_VAR => true,
        ARRAY_ELEMENT => p
            .parent()
            .and_then(|a| a.parent())
            .is_some_and(|a| a.kind() == ASSIGN_EXPR && a.first_child() == p.parent()),
        _ => false,
    }
}

fn token_matches(token: &SyntaxToken, name: &str, case_insensitive: bool) -> bool {
    let text = match token.kind() {
        VARIABLE => &token.text()[1..],
        IDENT => token.text(),
        k if k.is_keyword() => token.text(),
        _ => return false,
    };
    if case_insensitive {
        text.eq_ignore_ascii_case(name)
    } else {
        text == name
    }
}

fn file_references(sema: &Sema, def: &Definition, name: &str, include_declaration: bool) -> Vec<Reference> {
    file_references_of(sema, std::slice::from_ref(def), name, include_declaration)
}

/// References in this file to any of `defs` (which share `name`).
fn file_references_of(
    sema: &Sema,
    defs: &[Definition],
    name: &str,
    include_declaration: bool,
) -> Vec<Reference> {
    let Some(def) = defs.first() else { return Vec::new() };
    let ci = matches!(
        def,
        Definition::Class(_) | Definition::Function(_) | Definition::Method { .. } | Definition::Namespace(_)
    );
    let root = match def {
        Definition::Variable { scope, .. } => match node_with_range(&sema.root, *scope) {
            Some(n) => n,
            None => return Vec::new(),
        },
        _ => sema.root.clone(),
    };
    let mut out = Vec::new();
    let is_class = matches!(def, Definition::Class(_));
    let variable = matches!(def, Definition::Variable { .. });
    for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        // `/** @var Foo $name */` in the scope
        if variable && token.kind() == DOC_COMMENT && crate::sema::in_variable_scope(&token, &root) {
            out.extend(doc_tag_variables(sema, &token, "@var", name));
            continue;
        }
        // class names in doc comments
        if is_class
            && token.kind() == DOC_COMMENT
            && token.text().to_ascii_lowercase().contains(&name.to_ascii_lowercase())
        {
            for r in crate::doc_types::doc_type_refs(&token).into_iter().chain(see_refs(&token)) {
                let last = r.name.rsplit('\\').next().unwrap_or(&r.name);
                if !last.eq_ignore_ascii_case(name) {
                    continue;
                }
                let Some(fqn) = crate::classify::resolve_doc_name(sema, &token, &r.name) else { continue };
                if Definition::Class(fqn).same_as(def) {
                    // only the last segment is the name itself
                    let len = php_syntax::TextSize::from(last.len() as u32);
                    out.push(Reference {
                        file: sema.file,
                        range: TextRange::new(r.range.end() - len, r.range.end()),
                        kind: ReferenceKind::Read,
                    });
                }
            }
            continue;
        }
        if !token_matches(&token, name, ci) {
            continue;
        }
        // `\Foo\Foo`: only the last segment names the class
        if !matches!(def, Definition::Namespace(_))
            && token.parent().is_some_and(|p| {
                p.kind() == QUALIFIED_NAME
                    && p.last_token().is_some_and(|l| l.text_range().end() > token.text_range().end())
            })
        {
            continue;
        }
        let Some(d) = classify_token(sema, &token) else { continue };
        if !defs.iter().any(|def| d.same_as(def)) {
            continue;
        }
        let kind = reference_kind(&token);
        if kind == ReferenceKind::Declaration && !include_declaration {
            continue;
        }
        out.push(Reference { file: sema.file, range: token.text_range(), kind });
    }
    out
}

pub(crate) fn references(sema: &Sema, offset: TextSize, include_declaration: bool) -> Vec<Reference> {
    if let Some(refs) = this_references(sema, offset) {
        return refs;
    }
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let name = def.search_name();
    if let Definition::Variable { scope, .. } = &def {
        let mut out = file_references(sema, &def, &name, include_declaration);
        out.extend(param_doc_references(sema, *scope, &name));
        // the same parameter of overridden / overriding methods (named
        // arguments)
        out.extend(related_param_references(sema, *scope, &name, include_declaration));
        // a global variable: the file scope and the functions declaring it;
        // closures importing the variable with `use`
        let mut others = global_scopes(sema, *scope, &name);
        others.extend(closure_scopes(sema, *scope, &name));
        for other in others {
            let def = Definition::Variable { name: name.clone(), scope: other };
            out.extend(file_references(sema, &def, &name, include_declaration));
            out.extend(param_doc_references(sema, other, &name));
        }
        out.sort_by_key(|r| (r.file, r.range.start()));
        out.dedup();
        return out;
    }
    // a method together with the methods it overrides and that override it
    let defs = match &def {
        Definition::Method { class, name } => method_family(sema.index(), class, name)
            .into_iter()
            .map(|class| Definition::Method { class, name: name.clone() })
            .collect(),
        Definition::Property { class, name } => property_family(sema.index(), class, name)
            .into_iter()
            .map(|class| Definition::Property { class, name: name.clone() })
            .collect(),
        _ => vec![def.clone()],
    };
    // `new Foo(...)` calls the constructor
    let constructor_of = match &def {
        Definition::Method { class, name } if name.eq_ignore_ascii_case("__construct") => Some(class.clone()),
        _ => None,
    };
    let lower = name.to_ascii_lowercase();
    let class_lower = constructor_of.as_ref().map(|c| c.short().to_ascii_lowercase());
    let ws = sema.ws;
    let current = sema.file;
    let candidates: Vec<FileId> = ws
        .files()
        .filter(|(id, f)| *id == current || f.kind == FileKind::Workspace)
        .filter(|(_, f)| {
            let text = f.text.to_ascii_lowercase();
            text.contains(&lower) || class_lower.as_ref().is_some_and(|c| text.contains(c.as_str()))
        })
        .map(|(id, _)| id)
        .collect();
    let mut out: Vec<Reference> = crate::par::map(candidates, |id| match Sema::new(ws, id) {
        Some(s) => {
            let mut refs = file_references_of(&s, &defs, &name, include_declaration);
            if let Some(class) = &constructor_of {
                refs.extend(new_expressions(&s, class));
            }
            refs
        }
        None => Vec::new(),
    })
    .into_iter()
    .flatten()
    .collect();
    // declarations outside the workspace (vendor / stubs)
    if include_declaration {
        for def in &defs {
            for loc in declarations(sema, def, None) {
                if !out.iter().any(|r| r.file == loc.file && r.range == loc.focus) {
                    out.push(Reference {
                        file: loc.file,
                        range: loc.focus,
                        kind: ReferenceKind::Declaration,
                    });
                }
            }
        }
    }
    // promoted constructor parameters: their `@param` tags
    if let Definition::Property { name, .. } = &def {
        for d in &defs {
            for loc in declarations(sema, d, None) {
                let Some(s) = Sema::new(ws, loc.file) else { continue };
                let Some(token) = s.root.token_at_offset(loc.focus.start()).right_biased() else { continue };
                let promoted = token.parent().filter(|p| p.kind() == PARAM);
                if let Some(method) = promoted.and_then(|p| p.ancestors().find(|a| a.kind() == METHOD_DECL)) {
                    out.extend(param_doc_references(&s, method.text_range(), name));
                }
            }
        }
    }
    out.sort_by_key(|r| (r.file, r.range.start()));
    out.dedup();
    out
}

/// The classes whose own method `name` is `class::name`, one it overrides
/// or one overriding it (with the traits providing it).
pub(crate) fn method_family(index: &php_index::Index, class: &php_hir::Fqn, name: &str) -> Vec<php_hir::Fqn> {
    if name.eq_ignore_ascii_case("__construct") {
        return vec![class.clone()];
    }
    member_family(index, class, |c| {
        index
            .find_method(c, name)
            .filter(|m| m.def.visibility != php_hir::Visibility::Private)
            .map(|m| m.owner.fqn.clone())
    })
}

/// Like [`method_family`] for properties.
pub(crate) fn property_family(
    index: &php_index::Index,
    class: &php_hir::Fqn,
    name: &str,
) -> Vec<php_hir::Fqn> {
    member_family(index, class, |c| {
        index
            .find_property(c, name)
            .filter(|p| p.def.visibility != php_hir::Visibility::Private)
            .map(|p| p.owner.fqn.clone())
    })
}

/// `owner(c)`: the class declaring the member as seen from `c`.
fn member_family(
    index: &php_index::Index,
    class: &php_hir::Fqn,
    owner: impl Fn(&php_hir::Fqn) -> Option<php_hir::Fqn>,
) -> Vec<php_hir::Fqn> {
    let mut out = vec![class.clone()];
    let add = |out: &mut Vec<php_hir::Fqn>, f: php_hir::Fqn| {
        if !out.iter().any(|c| c.eq_ignore_case(&f)) {
            out.push(f);
        }
    };
    // the topmost declarations: the ancestors declaring the member, also
    // those of classes inheriting it (a trait's users, a subclass
    // implementing an interface with it)
    let mut from = vec![class.clone()];
    for (_, c) in index.subtypes(class) {
        if owner(&c.fqn).is_some_and(|o| o.eq_ignore_case(class)) {
            from.push(c.fqn.clone());
        }
    }
    let mut roots: Vec<php_hir::Fqn> = Vec::new();
    for start in &from {
        for (_, c) in index.ancestors_with(start, false) {
            if owner(&c.fqn).is_some_and(|o| o.eq_ignore_case(&c.fqn)) {
                add(&mut roots, c.fqn.clone());
            }
        }
    }
    for root in &roots {
        add(&mut out, root.clone());
        for (_, c) in index.subtypes(root) {
            if let Some(o) = owner(&c.fqn) {
                add(&mut out, o);
            }
        }
    }
    out
}

/// For a global variable (used at file level or declared `global`), the
/// other scopes of this file sharing it.
fn global_scopes(sema: &Sema, scope: TextRange, name: &str) -> Vec<TextRange> {
    let declares = |n: &SyntaxNode| {
        n.descendants().filter(|g| g.kind() == GLOBAL_STMT).any(|g| {
            g.descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == VARIABLE && &t.text()[1..] == name))
        })
    };
    let functions: Vec<SyntaxNode> = sema
        .root
        .descendants()
        .filter(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR))
        .filter(|n| declares(n))
        .collect();
    let file = sema.root.text_range();
    let is_global = scope == file || functions.iter().any(|f| f.text_range() == scope);
    if !is_global {
        return Vec::new();
    }
    std::iter::once(file).chain(functions.iter().map(|f| f.text_range())).filter(|r| *r != scope).collect()
}

/// The scopes sharing a variable through closure `use ($name)` clauses:
/// closures inside `scope` importing it and, for a closure, the scope it
/// imports it from.
fn closure_scopes(sema: &Sema, scope: TextRange, name: &str) -> Vec<TextRange> {
    let imports = |closure: &SyntaxNode| {
        closure.children().filter(|c| c.kind() == CLOSURE_USE).any(|u| {
            u.descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == VARIABLE && &t.text()[1..] == name))
        })
    };
    let mut done = vec![scope];
    let mut todo = vec![scope];
    while let Some(current) = todo.pop() {
        let Some(node) = node_with_range(&sema.root, current) else { continue };
        let mut linked = Vec::new();
        // inner closures
        for closure in node.descendants().filter(|n| n.kind() == CLOSURE_EXPR && *n != node) {
            let outer = closure.parent().map(|p| sema.variable_scope(&p));
            if outer.is_some_and(|o| o == node) && imports(&closure) {
                linked.push(closure.text_range());
            }
        }
        // the enclosing scope of an importing closure
        if node.kind() == CLOSURE_EXPR && imports(&node) {
            if let Some(p) = node.parent() {
                linked.push(sema.variable_scope(&p).text_range());
            }
        }
        for l in linked {
            if !done.contains(&l) {
                done.push(l);
                todo.push(l);
            }
        }
    }
    done.retain(|r| *r != scope);
    done
}

/// Class names after `@see` / `@uses` (`@see Foo`, `{@see Foo::bar()}`).
fn see_refs(token: &SyntaxToken) -> Vec<crate::doc_types::DocTypeRef> {
    let text = token.text();
    let base = token.text_range().start();
    let mut out = Vec::new();
    for tag in ["@see ", "@uses "] {
        let mut from = 0;
        while let Some(i) = text[from..].find(tag) {
            let start = from + i + tag.len();
            let rest = &text[start..];
            let lead = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            let name: String =
                rest[lead..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '\\').collect();
            if !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) {
                let at = base + TextSize::from((start + lead) as u32);
                out.push(crate::doc_types::DocTypeRef {
                    range: TextRange::at(at, TextSize::from(name.len() as u32)),
                    name,
                });
            }
            from = start;
        }
    }
    out
}

/// `$this` in the enclosing method (with its closures).
fn this_references(sema: &Sema, offset: TextSize) -> Option<Vec<Reference>> {
    let token = sema.root.token_at_offset(offset).find(|t| t.kind() == VARIABLE && t.text() == "$this")?;
    let scope = token
        .parent_ancestors()
        .find(|n| {
            matches!(n.kind(), METHOD_DECL | FUNCTION_DECL | CLASS_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS)
        })
        .unwrap_or_else(|| sema.root.clone());
    let mut out = Vec::new();
    let mut stack = vec![scope.clone()];
    while let Some(node) = stack.pop() {
        for e in node.children_with_tokens() {
            match e {
                php_syntax::NodeOrToken::Token(t) if t.kind() == VARIABLE && t.text() == "$this" => {
                    out.push(Reference { file: sema.file, range: t.text_range(), kind: ReferenceKind::Read });
                }
                php_syntax::NodeOrToken::Node(n)
                    if n != scope && matches!(n.kind(), ANON_CLASS | CLASS_DECL | FUNCTION_DECL) => {}
                php_syntax::NodeOrToken::Node(n) => stack.push(n),
                _ => {}
            }
        }
    }
    out.sort_by_key(|r| r.range.start());
    Some(out)
}

/// `$name` in the `@param` tags of the function whose range is `scope`.
fn param_doc_references(sema: &Sema, scope: TextRange, name: &str) -> Vec<Reference> {
    let Some(func) =
        node_with_range(&sema.root, scope).filter(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL))
    else {
        return Vec::new();
    };
    let mut prev = func.prev_sibling_or_token();
    while let Some(php_syntax::NodeOrToken::Token(t)) = &prev {
        if t.kind() != WHITESPACE {
            break;
        }
        prev = t.prev_sibling_or_token();
    }
    // the doc comment may also be the first token of the declaration
    let doc = match prev {
        Some(php_syntax::NodeOrToken::Token(t)) if t.kind() == DOC_COMMENT => Some(t),
        _ => func.first_token().filter(|t| t.kind() == DOC_COMMENT),
    };
    let Some(doc) = doc else { return Vec::new() };
    doc_tag_variables(sema, &doc, "param", name)
}

/// `$name` on the lines of `doc` containing `tag`.
fn doc_tag_variables(sema: &Sema, doc: &SyntaxToken, tag: &str, name: &str) -> Vec<Reference> {
    let needle = format!("${name}");
    let text = doc.text();
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        if line.contains(tag) {
            let mut from = 0;
            while let Some(i) = line[from..].find(&needle) {
                let at = from + i;
                let end = at + needle.len();
                let boundary = line[end..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
                if boundary {
                    out.push(Reference {
                        file: sema.file,
                        range: TextRange::at(
                            doc.text_range().start() + TextSize::from((line_start + at) as u32),
                            TextSize::from(needle.len() as u32),
                        ),
                        kind: ReferenceKind::Read,
                    });
                }
                from = end;
            }
        }
        line_start += line.len();
    }
    out
}

/// The parameter `$name` of the methods related to the method `scope`.
fn related_param_references(
    sema: &Sema,
    scope: TextRange,
    name: &str,
    include_declaration: bool,
) -> Vec<Reference> {
    let Some(method) = node_with_range(&sema.root, scope).filter(|n| n.kind() == METHOD_DECL) else {
        return Vec::new();
    };
    let is_param = method.descendants().filter(|n| n.kind() == PARAM).any(|p| {
        p.children_with_tokens()
            .any(|e| e.as_token().is_some_and(|t| t.kind() == VARIABLE && &t.text()[1..] == name))
    });
    if !is_param {
        return Vec::new();
    }
    let Some(class) = sema.enclosing_class(&method) else { return Vec::new() };
    let Some(method_name) = method
        .children()
        .find(|c| c.kind() == MEMBER_NAME)
        .and_then(|n| n.first_token())
        .map(|t| t.text().to_string())
    else {
        return Vec::new();
    };
    let index = sema.index();
    let mut out = Vec::new();
    for other in method_family(index, &class, &method_name) {
        if other.eq_ignore_case(&class) {
            continue;
        }
        let Some(found) = index.find_method(&other, &method_name) else { continue };
        if !found.def.params.iter().any(|p| p.name == name) {
            continue;
        }
        if sema.index().file_kind(found.file) != Some(FileKind::Workspace) {
            continue;
        }
        let Some(s) = Sema::new(sema.ws, found.file) else { continue };
        let Some(node) = node_with_range(&s.root, found.def.range) else { continue };
        for scope in std::iter::once(node.text_range()).chain(closure_scopes(&s, node.text_range(), name)) {
            let def = Definition::Variable { name: name.to_string(), scope };
            out.extend(file_references(&s, &def, name, include_declaration));
        }
        out.extend(param_doc_references(&s, node.text_range(), name));
    }
    out
}

/// The class names of `new class(...)` expressions.
fn new_expressions(sema: &Sema, class: &php_hir::Fqn) -> Vec<Reference> {
    let short = class.short();
    let mut out = Vec::new();
    for token in sema.root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if token.kind() != IDENT || !token.text().eq_ignore_ascii_case(short) {
            continue;
        }
        let Some(qn) = token.parent().filter(|p| p.kind() == QUALIFIED_NAME) else { continue };
        let new_expr = qn.parent().filter(|p| p.kind() == NAME_EXPR).and_then(|p| p.parent()).or(qn.parent());
        if !new_expr.is_some_and(|p| p.kind() == NEW_EXPR) {
            continue;
        }
        if classify_token(sema, &token).is_some_and(|d| d.same_as(&Definition::Class(class.clone()))) {
            out.push(Reference { file: sema.file, range: token.text_range(), kind: ReferenceKind::Read });
        }
    }
    out
}

pub(crate) fn document_highlights(sema: &Sema, offset: TextSize) -> Vec<Reference> {
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let name = def.search_name();
    file_references(sema, &def, &name, true)
}
