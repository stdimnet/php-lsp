//! Class-level checks from Intelephense's catalog: method compatibility
//! (P1038), final overrides (P1039), attributes (P1086 / P1088), colliding
//! trait methods (P1093), enum rules (P1050 / P1052 / P1096 / P1097 / P1103)
//! and readonly hooked properties (P1079).

use std::collections::HashMap;

use php_hir::symbols::{ClassDef, FunctionDef, Visibility};
use php_hir::{Fqn, Type};
use php_index::FileKind;
use php_syntax::ast;
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange};

use crate::diagnostics::{Checker, Diagnostic, DiagnosticCode as Code, Severity};

const ENUM_MAGIC_ALLOWED: &[&str] = &["__call", "__callstatic", "__invoke"];

const ALL_TARGETS: u32 = 127;

/// The `Attribute::TARGET_*` flag of what `owner` declares.
fn target_of(owner: &SyntaxNode) -> u32 {
    match owner.kind() {
        CLASS_DECL | ENUM_DECL | INTERFACE_DECL | TRAIT_DECL | ANON_CLASS => 1,
        FUNCTION_DECL | CLOSURE_EXPR | ARROW_FN_EXPR => 2,
        METHOD_DECL => 4,
        PROPERTY_DECL => 8,
        CLASS_CONST_DECL | ENUM_CASE => 16,
        // promoted parameters are properties too
        PARAM if owner.children().any(|c| c.kind() == MODIFIER_LIST) => 32 | 8,
        PARAM => 32,
        CONST_DECL => 64,
        _ => ALL_TARGETS,
    }
}

/// Names reserved for types, with the PHP version reserving them.
const RESERVED_CLASS_NAMES: &[(&str, (u8, u8))] = &[
    ("int", (7, 0)),
    ("float", (7, 0)),
    ("bool", (7, 0)),
    ("string", (7, 0)),
    ("true", (7, 0)),
    ("false", (7, 0)),
    ("null", (7, 0)),
    ("void", (7, 1)),
    ("iterable", (7, 1)),
    ("object", (7, 2)),
    ("mixed", (8, 0)),
    ("never", (8, 1)),
];

/// P1127: `#[\NoDiscard]` on `void` / `never` functions and magic methods.
fn no_discard_misuse(owner: &SyntaxNode) -> Option<String> {
    if !matches!(owner.kind(), FUNCTION_DECL | METHOD_DECL) {
        return None;
    }
    if owner.kind() == METHOD_DECL {
        let name =
            owner.children().find(|c| c.kind() == MEMBER_NAME).map(|n| ast::text_without_trivia(&n))?;
        if name.starts_with("__") && !name.eq_ignore_ascii_case("__invoke") {
            return Some(format!("Attribute 'NoDiscard' cannot be used on magic method '{name}'."));
        }
    }
    let ret = owner.children().find(|c| c.kind() == RETURN_TYPE)?;
    let t = ast::text_without_trivia(&ret).trim_start_matches(':').trim().to_ascii_lowercase();
    (t == "void" || t == "never").then(|| {
        "Attribute 'NoDiscard' cannot be used on functions or methods with a 'void' or 'never' return type.".to_string()
    })
}

/// Union members as Intelephense prints them (`?int` is `null|int`).
fn intelephense_type(t: &Type) -> String {
    let members = t.members();
    let mut parts: Vec<String> = Vec::new();
    if members.iter().any(|m| matches!(m, Type::Null)) && members.len() > 1 {
        parts.push("null".into());
    }
    parts.extend(
        members.iter().filter(|m| !matches!(m, Type::Null) || members.len() == 1).map(|m| m.to_string()),
    );
    parts.join("|")
}

/// A type for equality checks (member order and case do not matter).
fn type_key(t: &Type) -> Vec<String> {
    let mut v: Vec<String> = t.members().iter().map(|m| m.to_string().to_ascii_lowercase()).collect();
    v.sort();
    v
}

/// The class header including `extends` and `implements`.
fn header_full(node: &SyntaxNode) -> TextRange {
    let start = header_range(node).start();
    let end = node
        .children()
        .filter(|c| matches!(c.kind(), EXTENDS_CLAUSE | IMPLEMENTS_CLAUSE | NAME))
        .last()
        .map_or(node.text_range().end(), |c| c.text_range().end());
    TextRange::new(start, end.max(start))
}

fn visibility_rank(v: Visibility) -> u8 {
    match v {
        Visibility::Public => 0,
        Visibility::Protected => 1,
        Visibility::Private => 2,
    }
}

/// `public function f(int $x): int` of the method declared at `range`
/// (without attributes, doc comment and body).
fn method_header(class: &SyntaxNode, range: TextRange) -> Option<TextRange> {
    let m = class.descendants().find(|n| n.kind() == METHOD_DECL && n.text_range() == range)?;
    Some(decl_header(&m))
}

/// A declaration from its first keyword to the end of its return type /
/// parameter list.
fn decl_header(decl: &SyntaxNode) -> TextRange {
    let start = decl
        .children_with_tokens()
        .find(|e| !e.kind().is_trivia() && e.kind() != ATTRIBUTE_GROUP)
        .map_or(decl.text_range().start(), |e| e.text_range().start());
    let end = decl
        .children()
        .filter(|c| matches!(c.kind(), RETURN_TYPE | PARAM_LIST))
        .last()
        .map_or(decl.text_range().end(), |c| c.text_range().end());
    TextRange::new(start, end.max(start))
}

/// A declaration without leading attributes and doc comment.
fn without_attributes(decl: &SyntaxNode) -> TextRange {
    let start = decl
        .children_with_tokens()
        .find(|e| !e.kind().is_trivia() && e.kind() != ATTRIBUTE_GROUP)
        .map_or(decl.text_range().start(), |e| e.text_range().start());
    TextRange::new(start, decl.text_range().end())
}

/// A modifier keyword of a declaration.
fn modifier(node: &SyntaxNode, kind: php_syntax::SyntaxKind) -> Option<TextRange> {
    node.children()
        .filter(|c| c.kind() == MODIFIER_LIST)
        .flat_map(|m| m.children_with_tokens().filter_map(|e| e.into_token()).collect::<Vec<_>>())
        .chain(node.children_with_tokens().filter_map(|e| e.into_token()))
        .find(|t| t.kind() == kind)
        .map(|t| t.text_range())
}

/// `readonly class A extends B` (without attributes and doc comments).
fn header_range(node: &SyntaxNode) -> TextRange {
    let start = node
        .children_with_tokens()
        .find(|e| !e.kind().is_trivia() && e.kind() != ATTRIBUTE_GROUP)
        .map_or(node.text_range().start(), |e| e.text_range().start());
    let end = node
        .children()
        .find(|c| c.kind() == EXTENDS_CLAUSE)
        .or_else(|| node.children().find(|c| c.kind() == NAME))
        .map_or(node.text_range().end(), |c| c.text_range().end());
    TextRange::new(start, end.max(start))
}

fn const_element_range(class: &SyntaxNode, name: &str) -> Option<TextRange> {
    class
        .descendants()
        .filter(|n| n.kind() == CONST_ELEMENT)
        .find(|e| {
            e.children()
                .find(|c| matches!(c.kind(), NAME | MEMBER_NAME))
                .is_some_and(|n| ast::text_without_trivia(&n) == name)
        })
        .map(|e| e.text_range())
}

fn property_element_range(class: &SyntaxNode, name: &str) -> Option<TextRange> {
    let var = format!("${name}");
    class
        .descendants()
        .filter(|n| n.kind() == PROPERTY_ELEMENT)
        .find(|e| {
            e.children_with_tokens()
                .any(|t| t.kind() == VARIABLE && t.as_token().is_some_and(|t| t.text() == var))
        })
        .map(|e| e.text_range())
}

impl Checker<'_, '_> {
    fn error(&mut self, range: TextRange, code: Code, msg: String) {
        self.push(Diagnostic::new(range, Severity::Error, code, msg));
    }

    /// The index entry of the class declared by `node` in this file.
    fn own_class(&self, node: &SyntaxNode) -> Option<(Fqn, &ClassDef)> {
        let fqn = self.sema.class_fqn(node)?;
        let (file, def) = self.sema.index().classes(&fqn).into_iter().find(|(f, _)| *f == self.sema.file)?;
        let _ = file;
        Some((fqn, def))
    }

    pub(crate) fn lint_class(&mut self, node: &SyntaxNode) {
        if self.config.implementation_errors {
            self.lint_overrides(node);
            self.lint_trait_collisions(node);
        }
        if self.config.language_constraints {
            if node.kind() == ENUM_DECL {
                self.lint_enum(node);
            }
            self.lint_hooked_readonly(node);
            self.lint_declaration_rules(node);
        }
        if self.config.implementation_errors {
            self.lint_inheritance_rules(node);
        }
    }

    // ------------------------------------------------------------------ declarations

    /// Reserved names (P1023), `readonly` methods (P1045), enum cases outside
    /// enums (P1049), backing types (P1051), trait constants before PHP 8.2
    /// (P1055), static properties of readonly classes (P1058).
    fn lint_declaration_rules(&mut self, node: &SyntaxNode) {
        let v = self.sema.ws.config().php_version;
        let mut out: Vec<(TextRange, &'static str, String)> = Vec::new();
        if let Some(name) = node.children().find(|c| c.kind() == NAME) {
            let text = ast::text_without_trivia(&name);
            let since =
                RESERVED_CLASS_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(&text)).map(|(_, s)| *s);
            if since.is_some_and(|(major, minor)| v >= php_syntax::PhpVersion::new(major, minor)) {
                out.push((
                    name.text_range(),
                    "P1023",
                    format!("'{text}' is a reserved word and cannot be used to name a class, interface, trait or namespace."),
                ));
            }
        }
        let members: Vec<SyntaxNode> = node
            .children()
            .filter(|c| c.kind() == MEMBER_LIST)
            .flat_map(|l| l.children().collect::<Vec<_>>())
            .collect();
        let class_readonly = modifier(node, READONLY_KW).is_some();
        for m in &members {
            match m.kind() {
                METHOD_DECL if v >= php_syntax::PhpVersion::PHP_8_1 => {
                    if let Some(r) = modifier(m, READONLY_KW) {
                        out.push((r, "P1045", "Cannot use 'readonly' as method modifier.".into()));
                    }
                }
                ENUM_CASE if node.kind() != ENUM_DECL => {
                    out.push((m.text_range(), "P1049", "Enum case can only be used in an enum.".into()));
                }
                CLASS_CONST_DECL if node.kind() == TRAIT_DECL && v < php_syntax::PhpVersion::PHP_8_2 => {
                    out.push((m.text_range(), "P1055", "Traits cannot have constants.".into()));
                }
                PROPERTY_DECL
                    if class_readonly
                        && node.kind() == CLASS_DECL
                        && v >= php_syntax::PhpVersion::PHP_8_2 =>
                {
                    if let Some(r) = modifier(m, STATIC_KW) {
                        out.push((r, "P1058", "Readonly class cannot declare static properties.".into()));
                    }
                }
                _ => {}
            }
        }
        // P1095: `implements A, A`, `use T, T`
        let lists = node.children().filter(|c| matches!(c.kind(), IMPLEMENTS_CLAUSE | EXTENDS_CLAUSE)).chain(
            node.children()
                .filter(|c| c.kind() == MEMBER_LIST)
                .flat_map(|l| l.children().filter(|m| m.kind() == TRAIT_USE).collect::<Vec<_>>()),
        );
        for list in lists {
            let what = match list.kind() {
                IMPLEMENTS_CLAUSE => "implements",
                EXTENDS_CLAUSE => "extends",
                _ => "use",
            };
            let mut seen = std::collections::HashSet::new();
            for q in list.children().filter(|c| c.kind() == QUALIFIED_NAME) {
                let text = ast::text_without_trivia(&q);
                let key =
                    self.sema.resolve_class_name(&text, &q).map_or(text.to_ascii_lowercase(), |f| f.key());
                if !seen.insert(key) {
                    out.push((q.text_range(), "P1095", format!("Duplicate type {text} in {what} list.")));
                }
            }
        }
        if let Some(backing) = node
            .children()
            .find(|c| c.kind() == ENUM_BACKING_TYPE)
            .filter(|_| v >= php_syntax::PhpVersion::PHP_8_1)
        {
            if let Some(ty) = backing.children().next() {
                let t = ast::text_without_trivia(&ty).to_ascii_lowercase();
                if t != "int" && t != "string" {
                    out.push((ty.text_range(), "P1051", "Enum backing type must be int or string.".into()));
                }
            }
        }
        for (r, code, msg) in out {
            self.error(r, Code::Catalog(code), msg);
        }
    }

    // ------------------------------------------------------------------ inheritance

    /// Readonly classes and properties (P1059 - P1062), circular inheritance
    /// (P1134), typed class constants (P1102), trait property conflicts (P1110).
    fn lint_inheritance_rules(&mut self, node: &SyntaxNode) {
        let Some((fqn, def)) = self.own_class(node) else { return };
        let index = self.sema.index();
        let v = self.sema.ws.config().php_version;
        let mut out: Vec<(TextRange, &'static str, String)> = Vec::new();
        let header = header_range(node);
        if self.circular(&fqn) {
            out.push((header, "P1134", format!("'{fqn}' has a circular reference.")));
            for (r, code, msg) in out {
                self.error(r, Code::Catalog(code), msg);
            }
            return;
        }
        if def.kind == ast::ClassLikeKind::Class && v >= php_syntax::PhpVersion::PHP_8_2 {
            if let Some((_, parent)) = def.parent.as_ref().and_then(|p| index.class(p)) {
                if parent.is_readonly && !def.is_readonly {
                    out.push((header, "P1059", "Non-readonly class cannot extend readonly class.".into()));
                } else if def.is_readonly && !parent.is_readonly && parent.kind == ast::ClassLikeKind::Class {
                    out.push((header, "P1060", "Readonly class cannot extend non-readonly class.".into()));
                }
            }
        }
        // stub signatures vary between PHP versions
        let ancestors: Vec<&ClassDef> = index
            .ancestors_with(&fqn, false)
            .into_iter()
            .filter(|(f, _)| index.file_kind(*f) != Some(FileKind::Stub))
            .map(|(_, c)| c)
            .filter(|c| c.fqn != fqn && !def.traits.contains(&c.fqn) && c.kind != ast::ClassLikeKind::Trait)
            .collect();
        for p in def.properties.iter().filter(|p| p.magic.is_none() && v >= php_syntax::PhpVersion::PHP_8_1) {
            let Some(pp) = ancestors.iter().find_map(|c| c.property(&p.name)) else { continue };
            if pp.visibility == Visibility::Private || pp.magic.is_some() {
                continue;
            }
            let readonly = p.is_readonly || def.is_readonly;
            if pp.is_readonly && !readonly {
                out.push((
                    p.name_range,
                    "P1061",
                    "Cannot redeclare readonly property as non-readonly.".into(),
                ));
            } else if readonly && !pp.is_readonly {
                out.push((
                    p.name_range,
                    "P1062",
                    "Cannot redeclare non-readonly property as readonly.".into(),
                ));
            }
        }
        for c in def.constants.iter().filter(|_| v >= php_syntax::PhpVersion::PHP_8_3) {
            let Some(ct) = &c.ty else { continue };
            let Some((owner, pc)) = ancestors.iter().find_map(|a| a.constant(&c.name).map(|pc| (a, pc)))
            else {
                continue;
            };
            let Some(pt) = &pc.ty else { continue };
            if !ct.members().into_iter().all(|m| self.accepts(pt, m, true)) {
                let range = const_element_range(node, &c.name).unwrap_or(c.name_range);
                out.push((
                    range,
                    "P1102",
                    format!(
                        "Class constant {fqn}::{} is not compatible with {}::{}.",
                        c.name, owner.fqn, pc.name
                    ),
                ));
            }
        }
        for t in &def.traits {
            let Some((_, tdef)) = index.class(t) else { continue };
            for tp in tdef.properties.iter().filter(|p| p.magic.is_none()) {
                let Some(p) = def.property(&tp.name).filter(|p| p.magic.is_none()) else { continue };
                let same = p.visibility == tp.visibility
                    && p.is_static == tp.is_static
                    && p.is_readonly == tp.is_readonly
                    && p.ty.as_ref().map(|t| t.to_string()) == tp.ty.as_ref().map(|t| t.to_string())
                    && p.default == tp.default;
                if !same {
                    let range = property_element_range(node, &p.name).unwrap_or(p.name_range);
                    out.push((
                        range,
                        "P1110",
                        format!(
                            "In the composition of type '{fqn}', properties '{fqn}::${}' and '{t}::${}' are not compatible.",
                            p.name, tp.name
                        ),
                    ));
                }
            }
        }
        self.lint_property_redeclarations(&fqn, def, &mut out);
        self.lint_composition(node, &fqn, def, &mut out);
        for (r, code, msg) in out {
            self.error(r, Code::Catalog(code), msg);
        }
    }

    /// P1070 visibility and P1077 type of properties redeclared from a parent
    /// class or interface.
    fn lint_property_redeclarations(
        &self,
        fqn: &Fqn,
        def: &ClassDef,
        out: &mut Vec<(TextRange, &'static str, String)>,
    ) {
        let index = self.sema.index();
        let parents: Vec<&ClassDef> = index
            .ancestors_with(fqn, false)
            .into_iter()
            .filter(|(f, _)| index.file_kind(*f) != Some(FileKind::Stub))
            .map(|(_, c)| c)
            .filter(|c| c.fqn != *fqn && !def.traits.contains(&c.fqn) && c.kind != ast::ClassLikeKind::Trait)
            .collect();
        for p in def.properties.iter().filter(|p| p.magic.is_none() && !p.promoted) {
            let Some((owner, pp)) = parents.iter().find_map(|c| c.property(&p.name).map(|pp| (c, pp))) else {
                continue;
            };
            if pp.visibility == Visibility::Private || pp.magic.is_some() {
                continue;
            }
            let set_rank = |v: Option<Visibility>, get: Visibility| visibility_rank(v.unwrap_or(get));
            if visibility_rank(p.visibility) > visibility_rank(pp.visibility)
                || set_rank(p.set_visibility, p.visibility) > set_rank(pp.set_visibility, pp.visibility)
            {
                out.push((
                    p.range,
                    "P1070",
                    format!(
                        "Visibility of '{fqn}::${}' must be the same or less restrictive than '{}::${}'.",
                        p.name, owner.fqn, pp.name
                    ),
                ));
            }
            let (Some(ct), Some(pt)) = (&p.ty, &pp.ty) else { continue };
            let decl = self
                .sema
                .root
                .covering_element(p.range)
                .ancestors()
                .find(|n| n.kind() == PROPERTY_DECL)
                .map_or(p.range, |d| without_attributes(&d));
            let (ok, suffix) = match pp.hooks.as_slice() {
                // only read: the type may be narrower
                [h] if h == "get" => (self.strict_subtype(ct, pt), " or narrower"),
                // only written: the type may be wider
                [h] if h == "set" => (self.strict_subtype(pt, ct), " or wider"),
                _ => (type_key(ct) == type_key(pt), "."),
            };
            if !ok {
                out.push((
                    decl,
                    "P1077",
                    format!("Property {fqn}::${} must have type {}{suffix}", p.name, intelephense_type(pt)),
                ));
            }
        }
    }

    /// Every value of `sub` is a value of `sup` (no scalar conversions).
    fn strict_subtype(&self, sub: &Type, sup: &Type) -> bool {
        let index = self.sema.index();
        let sup_members = sup.members();
        if sup_members.iter().any(|m| matches!(m, Type::Mixed)) {
            return true;
        }
        let keys = type_key(sup);
        sub.members().into_iter().all(|m| {
            keys.contains(&m.to_string().to_ascii_lowercase())
                || matches!(m, Type::Named(c, _) if sup_members.iter().any(|s| match s {
                    Type::Named(p, _) => index.is_subtype(c, p),
                    Type::Object => true,
                    _ => false,
                }))
                || matches!(m, Type::Array(..) | Type::List(_) | Type::Shape(_))
                    && sup_members.iter().any(|s| matches!(s, Type::Iterable(..) | Type::Array(..)))
                || matches!(m, Type::True | Type::False)
                    && sup_members.iter().any(|s| matches!(s, Type::Bool))
        })
    }

    /// P1111 / P1112: constants and methods that traits, interfaces and the
    /// parent class bring together incompatibly.
    fn lint_composition(
        &self,
        node: &SyntaxNode,
        fqn: &Fqn,
        def: &ClassDef,
        out: &mut Vec<(TextRange, &'static str, String)>,
    ) {
        let index = self.sema.index();
        let header = header_full(node);
        let same_const = |a: &php_hir::symbols::ClassConstDef, b: &php_hir::symbols::ClassConstDef| {
            a.visibility == b.visibility && a.is_final == b.is_final && a.value == b.value
        };
        // a constant of the class and of one of its traits
        for t in &def.traits {
            let Some((_, tdef)) = index.class(t) else { continue };
            for tc in &tdef.constants {
                let Some(c) = def.constant(&tc.name) else { continue };
                if !same_const(c, tc) {
                    out.push((
                        c.range,
                        "P1111",
                        format!(
                            "In the composition of type '{fqn}', class constants '{fqn}::{}' and '{t}::{}' are not compatible.",
                            c.name, tc.name
                        ),
                    ));
                }
            }
        }
        // a trait constant against the parent's (accepted from PHP 8.5 on)
        if self.sema.ws.config().php_version < php_syntax::PhpVersion::PHP_8_5 {
            if let Some((_, parent)) = def.parent.as_ref().and_then(|p| index.class(p)) {
                for t in &def.traits {
                    let Some((_, tdef)) = index.class(t) else { continue };
                    for tc in &tdef.constants {
                        let Some(pc) = parent.constant(&tc.name) else { continue };
                        if def.constant(&tc.name).is_none() && !same_const(tc, pc) {
                            out.push((
                                header,
                                "P1111",
                                format!(
                                    "In the composition of type '{fqn}', class constants '{t}::{}' and '{}::{}' are not compatible.",
                                    tc.name, parent.fqn, pc.name
                                ),
                            ));
                        }
                    }
                }
            }
        }
        // the same constant from two interfaces
        let ifaces: Vec<&ClassDef> =
            def.interfaces.iter().filter_map(|i| index.class(i).map(|(_, c)| c)).collect();
        for (i, a) in ifaces.iter().enumerate() {
            for b in &ifaces[i + 1..] {
                for ac in &a.constants {
                    let Some(bc) = b.constant(&ac.name) else { continue };
                    if def.constant(&ac.name).is_none() && !same_const(ac, bc) {
                        out.push((
                            header,
                            "P1111",
                            format!(
                                "In the composition of type '{fqn}', class constants '{}::{}' and '{}::{}' are not compatible.",
                                a.fqn, ac.name, b.fqn, bc.name
                            ),
                        ));
                    }
                }
            }
        }
        // a trait method against an interface or parent method the class does not override
        let others: Vec<&ClassDef> = index
            .ancestors_with(fqn, false)
            .into_iter()
            .filter(|(f, _)| index.file_kind(*f) != Some(FileKind::Stub))
            .map(|(_, c)| c)
            .filter(|c| c.fqn != *fqn && c.kind != ast::ClassLikeKind::Trait && !def.traits.contains(&c.fqn))
            .collect();
        for t in &def.traits {
            let Some((_, tdef)) = index.class(t) else { continue };
            for tm in tdef.methods.iter().filter(|m| !m.is_abstract && !m.is_magic) {
                if def.methods.iter().any(|m| m.name.eq_ignore_ascii_case(&tm.name)) {
                    continue;
                }
                let Some((owner, om)) = others.iter().find_map(|c| c.method(&tm.name).map(|m| (c, m))) else {
                    continue;
                };
                if om.visibility == Visibility::Private
                    || om.is_magic
                    || tm.name.eq_ignore_ascii_case("__construct")
                {
                    continue;
                }
                if !self.signature_compatible(tm, om) {
                    out.push((
                        header,
                        "P1112",
                        format!(
                            "In the composition of type '{fqn}', methods '{t}::{}()' and '{}::{}()' are not compatible.",
                            tm.name, owner.fqn, om.name
                        ),
                    ));
                }
            }
        }
    }

    /// `fqn` reaches itself through `extends`.
    fn circular(&self, fqn: &Fqn) -> bool {
        let index = self.sema.index();
        let mut stack: Vec<Fqn> = index
            .class(fqn)
            .map(|(_, d)| d.parent.iter().chain(&d.interfaces).cloned().collect())
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        while let Some(f) = stack.pop() {
            if f.key() == fqn.key() {
                return true;
            }
            if !seen.insert(f.key()) || seen.len() > 200 {
                continue;
            }
            if let Some((_, d)) = index.class(&f) {
                stack.extend(d.parent.iter().chain(&d.interfaces).cloned());
            }
        }
        false
    }

    // ------------------------------------------------------------------ overrides

    fn lint_overrides(&mut self, node: &SyntaxNode) {
        let Some((fqn, def)) = self.own_class(node) else { return };
        let index = self.sema.index();
        if index.has_unknown_ancestor(&fqn) {
            return;
        }
        let own_traits = def.traits.clone();
        let mut out: Vec<(TextRange, Code, String)> = Vec::new();
        // `class A extends FinalClass`
        if let Some(parent) = &def.parent {
            if let Some((_, p)) = index.class(parent) {
                if p.is_final {
                    let range = node
                        .children()
                        .find(|c| c.kind() == EXTENDS_CLAUSE)
                        .map_or(def.name_range, |e| e.text_range());
                    out.push((
                        range,
                        Code::FinalExtend,
                        format!("Class {fqn} cannot extend final class {}.", p.fqn),
                    ));
                }
            }
        }
        let ancestors: Vec<(php_index::FileId, &ClassDef)> = index
            .ancestors_with(&fqn, false)
            .into_iter()
            .filter(|(_, c)| c.fqn != fqn && !own_traits.contains(&c.fqn))
            .collect();
        for m in def.methods.iter().filter(|m| !m.is_magic) {
            let Some((file, owner, pm)) =
                ancestors.iter().find_map(|(f, c)| c.method(&m.name).map(|pm| (*f, *c, pm)))
            else {
                continue;
            };
            if pm.visibility == Visibility::Private || pm.is_magic {
                continue;
            }
            let header = method_header(node, m.range).unwrap_or(m.name_range);
            if pm.is_final && owner.kind != ast::ClassLikeKind::Trait {
                out.push((
                    header,
                    Code::FinalOverride,
                    format!(
                        "Method '{fqn}::{}()' cannot override final method '{}::{}()'.",
                        m.name, owner.fqn, pm.name
                    ),
                ));
                continue;
            }
            // constructors are only checked against interfaces / abstract ones;
            // stub signatures vary between PHP versions
            let ctor = m.name.eq_ignore_ascii_case("__construct");
            if ctor && !(pm.is_abstract || owner.kind == ast::ClassLikeKind::Interface) {
                continue;
            }
            if index.file_kind(file) == Some(FileKind::Stub) {
                continue;
            }
            if !self.signature_compatible(m, pm)
                || visibility_rank(m.visibility) > visibility_rank(pm.visibility)
            {
                out.push((
                    header,
                    Code::IncompatibleMethod,
                    format!(
                        "Method '{fqn}::{}()' is not compatible with method '{}::{}()'.",
                        m.name, owner.fqn, pm.name
                    ),
                ));
            }
        }
        // abstract methods of the class's own traits
        for t in &own_traits {
            let Some((_, tdef)) = index.class(t) else { continue };
            for tm in tdef.methods.iter().filter(|tm| tm.is_abstract && !tm.is_magic) {
                let Some(m) = def.method(&tm.name) else { continue };
                if !self.signature_compatible(m, tm) {
                    out.push((
                        method_header(node, m.range).unwrap_or(m.name_range),
                        Code::IncompatibleMethod,
                        format!(
                            "Method '{fqn}::{}()' is not compatible with method '{t}::{}()'.",
                            m.name, tm.name
                        ),
                    ));
                }
            }
        }
        for (r, c, msg) in out {
            self.error(r, c, msg);
        }
    }

    /// PHP's rules for overriding `parent` with `child`: no more required
    /// parameters, all parent parameters accepted (contravariant types), a
    /// covariant return type.
    fn signature_compatible(&self, child: &FunctionDef, parent: &FunctionDef) -> bool {
        let required =
            |f: &FunctionDef| f.params.iter().filter(|p| p.default.is_none() && !p.variadic).count();
        let child_variadic = child.params.iter().any(|p| p.variadic);
        if required(child) > required(parent) {
            return false;
        }
        if !child_variadic && child.params.len() < parent.params.len() {
            return false;
        }
        for (i, pp) in parent.params.iter().enumerate() {
            let cp = child.params.get(i).or_else(|| child.params.last().filter(|p| p.variadic));
            let Some(cp) = cp else { return false };
            match (&cp.ty, &pp.ty) {
                (None, _) => {}
                (Some(_), None) => return false,
                (Some(ct), Some(pt)) => {
                    // every value the parent accepts must be accepted
                    if !pt.members().into_iter().all(|m| self.accepts(ct, m, true)) {
                        return false;
                    }
                }
            }
        }
        match (&child.return_type, &parent.return_type) {
            (_, None) => true,
            (None, Some(pt)) => matches!(pt, Type::Mixed),
            (Some(ct), Some(pt)) => ct.members().into_iter().all(|m| self.accepts(pt, m, true)),
        }
    }

    // ------------------------------------------------------------------ traits

    fn lint_trait_collisions(&mut self, node: &SyntaxNode) {
        let Some((fqn, def)) = self.own_class(node) else { return };
        if def.traits.len() < 2 {
            return;
        }
        let index = self.sema.index();
        // methods resolved explicitly with `insteadof` / declared by the class
        let resolved: Vec<String> = node
            .descendants()
            .filter(|n| n.kind() == TRAIT_PRECEDENCE)
            .filter_map(|n| n.children().find(|c| c.kind() == MEMBER_NAME))
            .map(|m| ast::text_without_trivia(&m).to_ascii_lowercase())
            .collect();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for t in &def.traits {
            let Some((_, tdef)) = index.class(t) else { return };
            if index.has_unknown_ancestor(t) {
                return;
            }
            for m in tdef.methods.iter().filter(|m| !m.is_abstract && !m.is_magic) {
                by_name.entry(m.name.to_ascii_lowercase()).or_default().push(format!("{}::{}()", t, m.name));
            }
        }
        let mut colliding: Vec<String> = by_name
            .into_iter()
            .filter(|(name, owners)| {
                owners.len() > 1 && !resolved.contains(name) && def.method(name).is_none()
            })
            .flat_map(|(_, owners)| owners)
            .collect();
        if colliding.is_empty() {
            return;
        }
        colliding.sort();
        let range = header_range(node);
        self.error(
            range,
            Code::CollidingTraitMethods,
            format!("Type {fqn} has colliding trait methods {}.", colliding.join(", ")),
        );
    }

    // ------------------------------------------------------------------ enums

    fn lint_enum(&mut self, node: &SyntaxNode) {
        let Some((fqn, def)) = self.own_class(node) else { return };
        let backed = def.backing_type.is_some();
        let mut out: Vec<(TextRange, Code, String)> = Vec::new();
        // the syntax of each case: whole case, `= value`, value
        let case_nodes: Vec<SyntaxNode> = node.descendants().filter(|n| n.kind() == ENUM_CASE).collect();
        let parts = |name: &str| {
            let n = case_nodes.iter().find(|c| {
                c.children()
                    .find(|x| matches!(x.kind(), NAME | MEMBER_NAME))
                    .is_some_and(|x| ast::text_without_trivia(&x) == name)
            })?;
            let eq = n.children_with_tokens().find(|e| e.kind() == EQ).map(|e| e.text_range());
            let value = n.children().find(|c| crate::infer::is_expr(c.kind())).map(|c| c.text_range());
            Some((without_attributes(n), eq, value))
        };
        let mut values: HashMap<String, Vec<TextRange>> = HashMap::new();
        for case in &def.cases {
            let (whole, eq, value) = parts(&case.name).unwrap_or((case.name_range, None, None));
            match (&case.value, backed) {
                (None, true) => out.push((
                    whole,
                    Code::EnumCaseValue,
                    format!("Case {} of backed enum {fqn} must have a value.", case.name),
                )),
                (Some(_), false) => out.push((
                    match (eq, value) {
                        (Some(e), Some(v)) => TextRange::new(e.start(), v.end()),
                        _ => case.name_range,
                    },
                    Code::EnumCaseUnexpectedValue,
                    format!("Case {} of non-backed enum {fqn} must not have a value.", case.name),
                )),
                (Some(v), true) => {
                    values.entry(v.clone()).or_default().push(value.unwrap_or(case.name_range))
                }
                (None, false) => {}
            }
        }
        // every case sharing a value
        let mut dups: Vec<(String, TextRange)> = values
            .into_iter()
            .filter(|(_, r)| r.len() > 1)
            .flat_map(|(v, rs)| rs.into_iter().map(move |r| (v.clone(), r)))
            .collect();
        dups.sort_by_key(|(_, r)| r.start());
        for (v, r) in dups {
            out.push((r, Code::DuplicateEnumValue, format!("Duplicate enum case value {v}.")));
        }
        for m in &def.methods {
            let lower = m.name.to_ascii_lowercase();
            if lower.starts_with("__") && !ENUM_MAGIC_ALLOWED.contains(&lower.as_str()) && !m.is_magic {
                let range = node
                    .descendants()
                    .find(|n| n.kind() == METHOD_DECL && n.text_range() == m.range)
                    .map_or(m.name_range, |n| without_attributes(&n));
                out.push((
                    range,
                    Code::EnumMagicMethod,
                    format!("Enum {fqn} cannot implement magic method {}.", m.name),
                ));
            }
        }
        for p in node.descendants().filter(|n| n.kind() == PROPERTY_DECL) {
            if p.ancestors()
                .skip(1)
                .find(|a| matches!(a.kind(), ENUM_DECL | CLASS_DECL | ANON_CLASS | TRAIT_DECL))
                .as_ref()
                == Some(node)
            {
                out.push((
                    p.text_range(),
                    Code::EnumProperty,
                    format!("Enum {fqn} cannot include properties."),
                ));
            }
        }
        for (r, c, msg) in out {
            self.error(r, c, msg);
        }
    }

    // ------------------------------------------------------------------ hooks

    fn lint_hooked_readonly(&mut self, node: &SyntaxNode) {
        let props: Vec<SyntaxNode> = node
            .descendants()
            .filter(|n| n.kind() == PROPERTY_DECL && n.descendants().any(|h| h.kind() == PROPERTY_HOOK_LIST))
            .collect();
        for p in props {
            let readonly = p
                .children()
                .find(|c| c.kind() == MODIFIER_LIST)
                .is_some_and(|m| m.children_with_tokens().any(|e| e.kind() == READONLY_KW));
            if readonly {
                self.error(
                    p.text_range(),
                    Code::HookedReadonly,
                    "Hooked properties cannot be readonly.".into(),
                );
            }
        }
    }

    // ------------------------------------------------------------------ attributes

    /// P1086 non-attribute classes used as attributes, P1088 repeated
    /// attributes that are not `Attribute::IS_REPEATABLE`.
    pub(crate) fn lint_attributes(&mut self, owner: &SyntaxNode) {
        if !self.config.language_constraints {
            return;
        }
        let attrs: Vec<SyntaxNode> = owner
            .children()
            .filter(|c| c.kind() == ATTRIBUTE_GROUP)
            .flat_map(|g| g.children().filter(|a| a.kind() == ATTRIBUTE).collect::<Vec<_>>())
            .collect();
        if attrs.is_empty() {
            return;
        }
        let index = self.sema.index();
        let mut seen: HashMap<String, ()> = HashMap::new();
        let mut out: Vec<(TextRange, Code, String)> = Vec::new();
        for a in attrs {
            let Some(q) = a.children().find(|c| c.kind() == QUALIFIED_NAME) else { continue };
            let Some(fqn) = self.sema.resolve_class_name(&ast::text_without_trivia(&q), &q) else { continue };
            let Some((file, def)) = index.class(&fqn) else { continue };
            if def.kind != ast::ClassLikeKind::Class {
                continue;
            }
            let is_attribute = def.attributes.iter().any(|x| x.as_str().eq_ignore_ascii_case("Attribute"))
                || index
                    .ancestors_with(&fqn, false)
                    .iter()
                    .any(|(_, c)| c.attributes.iter().any(|x| x.as_str().eq_ignore_ascii_case("Attribute")));
            if !is_attribute {
                if !index.has_unknown_ancestor(&fqn) {
                    out.push((
                        a.text_range(),
                        Code::NotAnAttribute,
                        format!("Attempting to use non-attribute \"{fqn}\" as attribute."),
                    ));
                }
                continue;
            }
            // Intelephense's stub of `Override` allows every target
            let unchecked = index.file_kind(file) == Some(FileKind::Stub)
                && fqn.as_str().eq_ignore_ascii_case("Override");
            if fqn.as_str().eq_ignore_ascii_case("NoDiscard") {
                if let Some(msg) = no_discard_misuse(owner) {
                    out.push((q.text_range(), Code::Catalog("P1127"), msg));
                }
            }
            if let Some(allowed) = self.attribute_targets(file, def).filter(|_| !unchecked) {
                if allowed & target_of(owner) == 0 {
                    out.push((
                        q.text_range(),
                        Code::Catalog("P1087"),
                        format!("Attribute \"{fqn}\" has invalid target."),
                    ));
                }
            }
            if seen.insert(fqn.as_str().to_ascii_lowercase(), ()).is_some() && !self.repeatable(file, def) {
                out.push((
                    q.text_range(),
                    Code::AttributeRepeated,
                    format!("Attribute \"{fqn}\" cannot be repeated."),
                ));
            }
        }
        for (r, c, msg) in out {
            self.error(r, c, msg);
        }
    }

    /// The `Attribute::TARGET_*` flags of an attribute class; `None` when
    /// they cannot be read.
    fn attribute_targets(&self, file: php_index::FileId, def: &ClassDef) -> Option<u32> {
        let parse = self.sema.ws.parse(file)?;
        let root = parse.syntax_node();
        let decl = root.descendants().find(|n| n.kind() == CLASS_DECL && n.text_range() == def.range)?;
        let attr = decl
            .children()
            .filter(|c| c.kind() == ATTRIBUTE_GROUP)
            .flat_map(|g| g.children().filter(|a| a.kind() == ATTRIBUTE).collect::<Vec<_>>())
            .find(|a| {
                a.children().find(|c| c.kind() == QUALIFIED_NAME).is_some_and(|q| {
                    ast::text_without_trivia(&q).trim_start_matches('\\').eq_ignore_ascii_case("Attribute")
                })
            })?;
        let Some(args) = attr.children().find(|c| c.kind() == ARG_LIST) else { return Some(ALL_TARGETS) };
        let Some(first) = args.children().find(|c| c.kind() == ARG) else { return Some(ALL_TARGETS) };
        let text = ast::text_without_trivia(&first);
        let mut flags = 0;
        for part in text.split('|') {
            let name = part.trim().rsplit("::").next()?.to_ascii_uppercase();
            flags |= match name.as_str() {
                "TARGET_CLASS" => 1,
                "TARGET_FUNCTION" => 2,
                "TARGET_METHOD" => 4,
                "TARGET_PROPERTY" => 8,
                "TARGET_CLASS_CONSTANT" => 16,
                "TARGET_PARAMETER" => 32,
                "TARGET_CONSTANT" => 64,
                "TARGET_ALL" => ALL_TARGETS,
                "IS_REPEATABLE" => 0,
                _ => return None,
            };
        }
        // only `IS_REPEATABLE`: all targets
        Some(if flags == 0 { ALL_TARGETS } else { flags })
    }

    /// `#[Attribute(Attribute::IS_REPEATABLE | ...)]` on the declaration.
    fn repeatable(&self, file: php_index::FileId, def: &ClassDef) -> bool {
        let Some(parse) = self.sema.ws.parse(file) else { return true };
        let root = parse.syntax_node();
        let Some(decl) = root.descendants().find(|n| n.kind() == CLASS_DECL && n.text_range() == def.range)
        else {
            return true;
        };
        decl.children()
            .filter(|c| c.kind() == ATTRIBUTE_GROUP)
            .any(|g| g.text().to_string().contains("IS_REPEATABLE"))
    }
}
