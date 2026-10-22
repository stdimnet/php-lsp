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
        }
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
            if pm.is_final && owner.kind != ast::ClassLikeKind::Trait {
                out.push((
                    m.name_range,
                    Code::FinalOverride,
                    format!(
                        "Method '{fqn}::{}' cannot override final method '{}::{}'.",
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
            if !self.signature_compatible(m, pm) {
                out.push((
                    m.name_range,
                    Code::IncompatibleMethod,
                    format!(
                        "Method '{fqn}::{}' is not compatible with method '{}::{}'.",
                        m.name, owner.fqn, pm.name
                    ),
                ));
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
                by_name.entry(m.name.to_ascii_lowercase()).or_default().push(format!("{}::{}", t, m.name));
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
        let range = def.name_range;
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
        let mut values: HashMap<String, ()> = HashMap::new();
        for case in &def.cases {
            match (&case.value, backed) {
                (None, true) => out.push((
                    case.name_range,
                    Code::EnumCaseValue,
                    format!("Case {} of backed enum {fqn} must have a value.", case.name),
                )),
                (Some(_), false) => out.push((
                    case.name_range,
                    Code::EnumCaseUnexpectedValue,
                    format!("Case {} of non-backed enum {fqn} must not have a value.", case.name),
                )),
                (Some(v), true) => {
                    if values.insert(v.clone(), ()).is_some() {
                        out.push((
                            case.name_range,
                            Code::DuplicateEnumValue,
                            format!("Duplicate enum case value {v}."),
                        ));
                    }
                }
                (None, false) => {}
            }
        }
        for m in &def.methods {
            let lower = m.name.to_ascii_lowercase();
            if lower.starts_with("__") && !ENUM_MAGIC_ALLOWED.contains(&lower.as_str()) && !m.is_magic {
                out.push((
                    m.name_range,
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
                        q.text_range(),
                        Code::NotAnAttribute,
                        format!("Attempting to use non-attribute \"{fqn}\" as attribute."),
                    ));
                }
                continue;
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
