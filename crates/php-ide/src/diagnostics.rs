//! Diagnostics: syntax errors plus semantic checks (undefined symbols,
//! argument counts, implementation errors, deprecations, unused code).

use std::collections::HashMap;

use php_hir::symbols::FunctionDef;
use php_hir::{Fqn, Type};
use php_syntax::ast::{self, AstNode, ClassLikeKind, UseKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::sema::{in_variable_scope, Sema};
use crate::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Stable diagnostic codes (shown to users and usable for suppression).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    SyntaxError,
    UnusedSymbol,
    ArgumentCount,
    Deprecated,
    UndefinedType,
    UndefinedFunction,
    UndefinedConstant,
    UndefinedVariable,
    UndefinedMethod,
    UndefinedProperty,
    UndefinedClassConstant,
    AbstractInstantiation,
    MissingImplementation,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::SyntaxError => "P1001",
            DiagnosticCode::UnusedSymbol => "P1003",
            DiagnosticCode::ArgumentCount => "P1005",
            DiagnosticCode::Deprecated => "P1007",
            DiagnosticCode::UndefinedVariable => "P1008",
            DiagnosticCode::UndefinedType => "P1009",
            DiagnosticCode::UndefinedFunction => "P1010",
            DiagnosticCode::UndefinedConstant => "P1011",
            DiagnosticCode::UndefinedMethod => "P1013",
            DiagnosticCode::UndefinedProperty => "P1014",
            DiagnosticCode::UndefinedClassConstant => "P1012",
            DiagnosticCode::AbstractInstantiation => "P1016",
            DiagnosticCode::MissingImplementation => "P1037",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
    pub code: DiagnosticCode,
    /// The code is unnecessary (rendered faded by editors).
    pub unnecessary: bool,
    /// The code is deprecated (rendered struck through by editors).
    pub deprecated: bool,
}

impl Diagnostic {
    fn new(
        range: TextRange,
        severity: Severity,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic { range, severity, message: message.into(), code, unnecessary: false, deprecated: false }
    }
}

/// Which diagnostics are reported (names follow Intelephense's
/// `diagnostics.*` settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsConfig {
    pub enable: bool,
    pub syntax: bool,
    pub undefined_types: bool,
    pub undefined_functions: bool,
    pub undefined_constants: bool,
    pub undefined_class_constants: bool,
    pub undefined_methods: bool,
    pub undefined_properties: bool,
    pub undefined_variables: bool,
    pub unused_symbols: bool,
    pub deprecated: bool,
    pub argument_count: bool,
    pub implementation_errors: bool,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        DiagnosticsConfig {
            enable: true,
            syntax: true,
            undefined_types: true,
            undefined_functions: true,
            undefined_constants: true,
            undefined_class_constants: true,
            undefined_methods: true,
            undefined_properties: true,
            undefined_variables: true,
            unused_symbols: true,
            deprecated: true,
            argument_count: true,
            implementation_errors: true,
        }
    }
}

pub(crate) fn syntax_diagnostics(config: &Config, parse: &php_syntax::Parse) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if config.diagnostics.enable && config.diagnostics.syntax {
        for e in parse.errors() {
            out.push(Diagnostic::new(
                e.range,
                Severity::Error,
                DiagnosticCode::SyntaxError,
                e.message.clone(),
            ));
        }
    }
    out
}

pub(crate) fn semantic_diagnostics(sema: &Sema) -> Vec<Diagnostic> {
    let config = &sema.ws.config().diagnostics;
    if !config.enable {
        return Vec::new();
    }
    let mut cx = Checker { sema, config, out: Vec::new() };
    cx.run();
    let mut out = cx.out;
    out.sort_by_key(|d| (d.range.start(), d.range.end()));
    out.dedup_by(|a, b| a.range == b.range && a.code == b.code);
    out
}

const BUILTIN_TYPES: &[&str] = &[
    "int", "integer", "float", "double", "string", "bool", "boolean", "array", "callable", "iterable",
    "object", "mixed", "void", "null", "never", "false", "true", "resource", "self", "static", "parent",
];

struct Checker<'s, 'a> {
    sema: &'s Sema<'a>,
    config: &'s DiagnosticsConfig,
    out: Vec<Diagnostic>,
}

impl Checker<'_, '_> {
    fn push(&mut self, d: Diagnostic) {
        self.out.push(d);
    }

    fn deprecated(&mut self, range: TextRange, what: &str, reason: Option<&String>) {
        if !self.config.deprecated {
            return;
        }
        let Some(reason) = reason else { return };
        let mut msg = format!("'{what}' is deprecated.");
        if !reason.is_empty() {
            msg.push(' ');
            msg.push_str(reason);
        }
        let mut d = Diagnostic::new(range, Severity::Hint, DiagnosticCode::Deprecated, msg);
        d.deprecated = true;
        self.push(d);
    }

    fn run(&mut self) {
        let root = self.sema.root.clone();
        let index_ready = self.sema.ws.index_ready();
        for node in root.descendants() {
            if index_ready {
                match node.kind() {
                    QUALIFIED_NAME => self.check_class_ref(&node),
                    CALL_EXPR => self.check_function_call(&node),
                    NAME_EXPR => self.check_constant(&node),
                    METHOD_CALL_EXPR | STATIC_CALL_EXPR => self.check_method_call(&node),
                    PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR => self.check_property(&node),
                    CLASS_CONST_FETCH_EXPR => self.check_class_constant(&node),
                    NEW_EXPR => self.check_new(&node),
                    CLASS_DECL | ENUM_DECL => self.check_implementation(&node),
                    _ => {}
                }
            }
            if matches!(node.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR) {
                self.check_variables(&node);
            }
        }
        if index_ready && self.config.undefined_types {
            self.check_doc_types();
        }
        if self.config.unused_symbols {
            self.check_unused_imports();
        }
    }

    // ------------------------------------------------------------------ types

    fn check_class_ref(&mut self, qn: &SyntaxNode) {
        let Some(ctx) = qn.parent() else { return };
        let is_class_ref = match ctx.kind() {
            NAMED_TYPE | EXTENDS_CLAUSE | IMPLEMENTS_CLAUSE | CATCH_TYPES | TRAIT_USE => true,
            NAME_EXPR => {
                let Some(grand) = ctx.parent() else { return };
                let first = grand.first_child().as_ref() == Some(&ctx);
                match grand.kind() {
                    NEW_EXPR | INSTANCEOF_EXPR => true,
                    STATIC_CALL_EXPR | CLASS_CONST_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR => first,
                    _ => false,
                }
            }
            _ => false,
        };
        if !is_class_ref {
            return;
        }
        let text = ast::text_without_trivia(qn);
        if BUILTIN_TYPES.contains(&text.to_ascii_lowercase().as_str()) {
            return;
        }
        let Some(fqn) = self.sema.resolve_class_name(&text, qn) else { return };
        match self.sema.index().class(&fqn) {
            Some((_, c)) => {
                let deprecated = c.deprecated.clone();
                self.deprecated(qn.text_range(), fqn.as_str(), deprecated.as_ref());
            }
            None => {
                // `catch (Throwable)` in a namespace etc. resolve fine; only
                // report names that are not declared anywhere
                if self.config.undefined_types {
                    self.push(Diagnostic::new(
                        qn.text_range(),
                        Severity::Error,
                        DiagnosticCode::UndefinedType,
                        format!("Undefined type '{fqn}'."),
                    ));
                }
            }
        }
    }

    // ------------------------------------------------------------------ calls

    fn args(call: &SyntaxNode) -> Option<SyntaxNode> {
        call.children().find(|c| c.kind() == ARG_LIST)
    }

    fn check_arg_count(&mut self, call: &SyntaxNode, f: &FunctionDef, range: TextRange) {
        self.check_arg_count_overloads(call, &[f], range);
    }

    /// Reports an argument count error unless some overload accepts the call.
    fn check_arg_count_overloads(&mut self, call: &SyntaxNode, overloads: &[&FunctionDef], range: TextRange) {
        if !self.config.argument_count
            || overloads.is_empty()
            || overloads.iter().any(|f| f.dynamic_args || f.is_magic)
        {
            return;
        }
        let Some(args) = Self::args(call) else { return };
        let mut count = 0;
        for arg in args.children().filter(|c| c.kind() == ARG) {
            // spread, named arguments and first-class callables are not counted
            if arg.children_with_tokens().any(|e| e.kind() == DOT3)
                || arg.children().any(|c| c.kind() == NAME)
            {
                return;
            }
            count += 1;
        }
        let accepts = |f: &FunctionDef| {
            let required =
                f.params.iter().rposition(|p| p.default.is_none() && !p.variadic).map_or(0, |i| i + 1);
            let variadic = f.params.iter().any(|p| p.variadic);
            count >= required && (variadic || count <= f.params.len())
        };
        if overloads.iter().any(|f| accepts(f)) {
            return;
        }
        let f = overloads[0];
        let required = f.params.iter().rposition(|p| p.default.is_none() && !p.variadic).map_or(0, |i| i + 1);
        let variadic = f.params.iter().any(|p| p.variadic);
        let max = f.params.len();
        if count < required {
            self.push(Diagnostic::new(
                range,
                Severity::Error,
                DiagnosticCode::ArgumentCount,
                format!(
                    "Expected {required} argument{}. Found {count}.",
                    if required == 1 { "" } else { "s" }
                ),
            ));
        } else if !variadic && count > max {
            self.push(Diagnostic::new(
                range,
                Severity::Error,
                DiagnosticCode::ArgumentCount,
                format!("Expected {max} argument{}. Found {count}.", if max == 1 { "" } else { "s" }),
            ));
        }
    }

    fn check_function_call(&mut self, call: &SyntaxNode) {
        let Some(callee) = call.first_child().filter(|c| c.kind() == NAME_EXPR) else { return };
        let text = ast::text_without_trivia(&callee);
        match self.sema.resolve_function(&text, callee.text_range().start()) {
            Some((_, f)) => {
                let overloads: Vec<&FunctionDef> =
                    self.sema.index().functions(&f.fqn).into_iter().map(|(_, f)| f).collect();
                self.check_arg_count_overloads(call, &overloads, callee.text_range());
                self.deprecated(callee.text_range(), f.fqn.as_str(), f.deprecated.as_ref());
            }
            None => {
                if self.config.undefined_functions {
                    let name = self.sema.scope_at(callee.text_range().start()).resolve_function(&text);
                    let shown = name.fallback.unwrap_or(name.name);
                    self.push(Diagnostic::new(
                        callee.text_range(),
                        Severity::Error,
                        DiagnosticCode::UndefinedFunction,
                        format!("Call to unknown function: '{shown}'"),
                    ));
                }
            }
        }
    }

    fn check_constant(&mut self, node: &SyntaxNode) {
        let Some(parent) = node.parent() else { return };
        // callees and class references are checked elsewhere
        if matches!(
            parent.kind(),
            CALL_EXPR
                | NEW_EXPR
                | INSTANCEOF_EXPR
                | STATIC_CALL_EXPR
                | CLASS_CONST_FETCH_EXPR
                | STATIC_PROPERTY_FETCH_EXPR
        ) && parent.first_child().as_ref() == Some(node)
            || parent.kind() == INSTANCEOF_EXPR
            || parent.kind() == NEW_EXPR
        {
            return;
        }
        let text = ast::text_without_trivia(node);
        if matches!(text.to_ascii_lowercase().trim_start_matches('\\'), "true" | "false" | "null") {
            return;
        }
        match self.sema.resolve_constant(&text, node.text_range().start()) {
            Some((_, c)) => {
                let deprecated = c.deprecated.clone();
                self.deprecated(node.text_range(), c.fqn.as_str(), deprecated.as_ref());
            }
            None => {
                if self.config.undefined_constants {
                    let r = self.sema.scope_at(node.text_range().start()).resolve_constant(&text);
                    self.push(Diagnostic::new(
                        node.text_range(),
                        Severity::Error,
                        DiagnosticCode::UndefinedConstant,
                        format!("Undefined constant '{}'.", r.fallback.unwrap_or(r.name)),
                    ));
                }
            }
        }
    }

    /// Known classes of the receiver; `None` if the type is not fully known.
    fn receiver_classes(&self, node: &SyntaxNode) -> Option<Vec<Fqn>> {
        let lhs = node.first_child()?;
        if matches!(node.kind(), STATIC_CALL_EXPR | STATIC_PROPERTY_FETCH_EXPR | CLASS_CONST_FETCH_EXPR) {
            if lhs.kind() == NAME_EXPR {
                let fqn = self.sema.resolve_class_name(&ast::text_without_trivia(&lhs), &lhs)?;
                let (_, def) = self.sema.index().class(&fqn)?;
                if def.kind == ClassLikeKind::Trait || self.sema.index().has_unknown_ancestor(&fqn) {
                    return None;
                }
                return Some(vec![fqn]);
            }
            return None;
        }
        let ty = self.sema.infer().expr_type(&lhs);
        // only check when every union member is a known class (or null)
        let mut classes = Vec::new();
        for m in ty.members() {
            match m {
                Type::Named(n, _) => {
                    let (_, def) = self.sema.index().class(n)?;
                    // inside traits `$this` refers to the (unknown) using class
                    if def.kind == ClassLikeKind::Trait || self.sema.index().has_unknown_ancestor(n) {
                        return None;
                    }
                    classes.push(n.clone());
                }
                Type::Null => {}
                _ => return None,
            }
        }
        if classes.is_empty() {
            None
        } else {
            Some(classes)
        }
    }

    fn has_magic(&self, classes: &[Fqn], name: &str) -> bool {
        classes.iter().any(|c| self.sema.index().find_method(c, name).is_some())
    }

    fn member_name(node: &SyntaxNode) -> Option<(String, TextRange)> {
        let m = node.children().find_map(ast::MemberName::cast)?;
        // dynamic names (`$a->$b`, `$a->{...}`) are not checked
        if m.syntax().children().next().is_some()
            || m.syntax().children_with_tokens().any(|e| e.kind() == L_CURLY)
        {
            return None;
        }
        Some((m.text()?, m.syntax().text_range()))
    }

    fn check_method_call(&mut self, node: &SyntaxNode) {
        let Some((name, range)) = Self::member_name(node) else { return };
        let Some(classes) = self.receiver_classes(node) else { return };
        let index = self.sema.index();
        let mut found = false;
        for c in &classes {
            if let Some(m) = index.find_method(c, &name) {
                found = true;
                self.check_arg_count(node, m.def, range);
                let what = format!("{}::{}", m.owner.fqn, m.def.name);
                self.deprecated(range, &what, m.def.deprecated.as_ref());
                break;
            }
        }
        if found || !self.config.undefined_methods {
            return;
        }
        let magic = if node.kind() == STATIC_CALL_EXPR { "__callStatic" } else { "__call" };
        if self.has_magic(&classes, magic)
            || node.kind() == STATIC_CALL_EXPR && self.has_magic(&classes, "__call")
        {
            return;
        }
        self.push(Diagnostic::new(
            range,
            Severity::Error,
            DiagnosticCode::UndefinedMethod,
            format!("Undefined method '{name}'."),
        ));
    }

    fn check_property(&mut self, node: &SyntaxNode) {
        let Some((name, range)) = Self::member_name(node) else { return };
        if is_write_target(node) {
            return;
        }
        let Some(classes) = self.receiver_classes(node) else { return };
        let index = self.sema.index();
        for c in &classes {
            if let Some(p) = index.find_property(c, &name) {
                let what = format!("{}::${}", p.owner.fqn, p.def.name);
                self.deprecated(range, &what, p.def.deprecated.as_ref());
                return;
            }
            if let Some((_, def)) = index.class(c) {
                if def.kind == ClassLikeKind::Enum && (name == "name" || name == "value") {
                    return;
                }
            }
        }
        if !self.config.undefined_properties || self.has_magic(&classes, "__get") {
            return;
        }
        // classes allowing dynamic properties
        let dynamic = classes.iter().any(|c| {
            c.as_str().eq_ignore_ascii_case("stdClass")
                || index.ancestors(c).iter().any(|(_, a)| {
                    a.fqn.as_str().eq_ignore_ascii_case("stdClass")
                        || a.attributes
                            .iter()
                            .any(|at| at.short().eq_ignore_ascii_case("AllowDynamicProperties"))
                })
        });
        if dynamic {
            return;
        }
        self.push(Diagnostic::new(
            range,
            Severity::Error,
            DiagnosticCode::UndefinedProperty,
            format!("Undefined property '${name}'."),
        ));
    }

    fn check_class_constant(&mut self, node: &SyntaxNode) {
        let Some((name, range)) = Self::member_name(node) else { return };
        if name.eq_ignore_ascii_case("class") {
            return;
        }
        let Some(classes) = self.receiver_classes(node) else { return };
        let index = self.sema.index();
        for c in &classes {
            if index.find_case(c, &name).is_some() {
                return;
            }
            if let Some(k) = index.find_class_constant(c, &name) {
                let what = format!("{}::{}", k.owner.fqn, k.def.name);
                self.deprecated(range, &what, k.def.deprecated.as_ref());
                return;
            }
        }
        if self.config.undefined_class_constants {
            self.push(Diagnostic::new(
                range,
                Severity::Error,
                DiagnosticCode::UndefinedClassConstant,
                format!("Undefined class constant '{name}'."),
            ));
        }
    }

    fn check_new(&mut self, node: &SyntaxNode) {
        let Some(class) = node.children().find(|c| c.kind() == NAME_EXPR) else { return };
        let text = ast::text_without_trivia(&class);
        let lower = text.to_ascii_lowercase();
        let Some(fqn) = self.sema.resolve_class_name(&text, &class) else { return };
        let Some((_, def)) = self.sema.index().class(&fqn) else { return };
        if self.sema.index().has_unknown_ancestor(&fqn) {
            return;
        }
        if self.config.implementation_errors && !matches!(lower.as_str(), "static") {
            let what = match def.kind {
                ClassLikeKind::Interface => Some("interface"),
                ClassLikeKind::Trait => Some("trait"),
                ClassLikeKind::Enum => Some("enum"),
                ClassLikeKind::Class if def.is_abstract => Some("abstract class"),
                _ => None,
            };
            if let Some(what) = what {
                self.push(Diagnostic::new(
                    class.text_range(),
                    Severity::Error,
                    DiagnosticCode::AbstractInstantiation,
                    format!("Cannot instantiate {what} '{fqn}'."),
                ));
                return;
            }
        }
        if let Some(ctor) = self.sema.index().find_method(&fqn, "__construct") {
            self.check_arg_count(node, ctor.def, class.text_range());
        } else if self.config.argument_count {
            // no constructor: no arguments allowed
            if let Some(args) = Self::args(node) {
                let n = args.children().filter(|c| c.kind() == ARG).count();
                if n > 0 && !args.children().any(|a| a.children_with_tokens().any(|e| e.kind() == DOT3)) {
                    self.push(Diagnostic::new(
                        class.text_range(),
                        Severity::Error,
                        DiagnosticCode::ArgumentCount,
                        format!("Expected 0 arguments. Found {n}."),
                    ));
                }
            }
        }
    }

    fn check_implementation(&mut self, node: &SyntaxNode) {
        if !self.config.implementation_errors {
            return;
        }
        let Some(fqn) = self.sema.class_fqn(node) else { return };
        let Some((_, def)) = self.sema.index().class(&fqn) else { return };
        if def.is_abstract || self.sema.index().has_unknown_ancestor(&fqn) {
            return;
        }
        let Some(name_node) = node.children().find(|c| c.kind() == NAME) else { return };
        let members = self.sema.index().members_with(&fqn, false);
        let mut missing: Vec<String> = members
            .methods
            .iter()
            .filter(|m| m.def.is_abstract && !m.def.is_magic && !m.owner.fqn.eq_ignore_case(&fqn))
            // enums implement cases(), from() and tryFrom() implicitly
            .filter(|m| {
                !(def.kind == ClassLikeKind::Enum
                    && matches!(m.def.name.to_ascii_lowercase().as_str(), "cases" | "from" | "tryfrom"))
            })
            .map(|m| format!("{}::{}", m.owner.fqn, m.def.name))
            .collect();
        missing.sort();
        if missing.is_empty() {
            return;
        }
        let shown: Vec<&str> = missing.iter().take(3).map(String::as_str).collect();
        let more = if missing.len() > 3 { format!(" and {} more", missing.len() - 3) } else { String::new() };
        self.push(Diagnostic::new(
            name_node.text_range(),
            Severity::Error,
            DiagnosticCode::MissingImplementation,
            format!("'{}' does not implement methods {}{}.", fqn, shown.join(", "), more),
        ));
    }

    fn check_doc_types(&mut self) {
        let templates = crate::doc_types::template_names(&self.sema.root);
        let docs: Vec<SyntaxToken> = self
            .sema
            .root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == DOC_COMMENT && t.text().contains('@'))
            .collect();
        for tok in docs {
            for r in crate::doc_types::doc_type_refs(&tok) {
                if templates.iter().any(|t| *t == r.name) {
                    continue;
                }
                let Some(anchor) = tok.parent() else { continue };
                let Some(fqn) = self.sema.resolve_class_name(&r.name, &anchor) else { continue };
                if self.sema.index().class(&fqn).is_none() {
                    self.push(Diagnostic::new(
                        r.range,
                        Severity::Error,
                        DiagnosticCode::UndefinedType,
                        format!("Undefined type '{fqn}'."),
                    ));
                }
            }
        }
    }

    // ------------------------------------------------------------------ imports

    fn check_unused_imports(&mut self) {
        let root = self.sema.root.clone();
        // all identifiers used as first name segments, plus doc comment words
        let mut used: HashMap<String, ()> = HashMap::new();
        let mut used_fns: HashMap<String, ()> = HashMap::new();
        let mut used_consts: HashMap<String, ()> = HashMap::new();
        for node in root.descendants() {
            if node.kind() == QUALIFIED_NAME && !node.ancestors().any(|a| a.kind() == USE_DECL) {
                let text = ast::text_without_trivia(&node);
                if text.starts_with('\\') || text.to_ascii_lowercase().starts_with("namespace\\") {
                    continue;
                }
                let first = text.split('\\').next().unwrap_or(&text).to_ascii_lowercase();
                let parent = node.parent();
                let is_call = parent.as_ref().is_some_and(|p| {
                    p.kind() == NAME_EXPR
                        && p.parent()
                            .is_some_and(|g| g.kind() == CALL_EXPR && g.first_child().as_ref() == Some(p))
                });
                let is_const = parent.as_ref().is_some_and(|p| p.kind() == NAME_EXPR) && !is_call;
                if is_call && !text.contains('\\') {
                    used_fns.insert(first.clone(), ());
                } else if is_const && !text.contains('\\') {
                    used_consts.insert(text.clone(), ());
                }
                used.insert(first, ());
            }
        }
        let mut doc_words: HashMap<String, ()> = HashMap::new();
        for tok in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if tok.kind() == DOC_COMMENT {
                for word in tok.text().split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\\')) {
                    let first = word.trim_start_matches('\\').split('\\').next().unwrap_or("");
                    if !first.is_empty() {
                        doc_words.insert(first.to_ascii_lowercase(), ());
                    }
                }
            }
        }
        for scope in &self.sema.scopes.scopes {
            for import in &scope.imports {
                let alias = import.alias.to_ascii_lowercase();
                let is_used = match import.kind {
                    UseKind::Class => used.contains_key(&alias) || doc_words.contains_key(&alias),
                    UseKind::Function => used_fns.contains_key(&alias),
                    UseKind::Const => used_consts.contains_key(&import.alias),
                };
                if !is_used {
                    let mut d = Diagnostic::new(
                        import.range,
                        Severity::Hint,
                        DiagnosticCode::UnusedSymbol,
                        format!("Symbol '{}' is declared but not used.", import.target),
                    );
                    d.unnecessary = true;
                    self.push(d);
                }
            }
        }
    }

    // ------------------------------------------------------------------ variables

    fn check_variables(&mut self, scope: &SyntaxNode) {
        if !self.config.undefined_variables && !self.config.unused_symbols {
            return;
        }
        // dynamic features make variable analysis unreliable
        let dynamic = scope.descendants().any(|n| {
            matches!(n.kind(), DYNAMIC_VARIABLE_EXPR | EVAL_EXPR | INCLUDE_EXPR)
                || n.kind() == CALL_EXPR
                    && n.first_child().is_some_and(|c| {
                        c.kind() == NAME_EXPR
                            && matches!(
                                ast::text_without_trivia(&c)
                                    .trim_start_matches('\\')
                                    .to_ascii_lowercase()
                                    .as_str(),
                                "extract" | "compact" | "get_defined_vars" | "parse_str" | "func_get_args"
                            )
                    })
        });
        if dynamic {
            return;
        }
        let mut vars: HashMap<String, VarInfo> = HashMap::new();
        for tok in scope.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if tok.kind() != VARIABLE {
                continue;
            }
            let name = tok.text()[1..].to_string();
            if name == "this" || is_superglobal(&name) {
                continue;
            }
            let Some(parent) = tok.parent() else { continue };
            // property declarations and static property names
            if matches!(parent.kind(), PROPERTY_ELEMENT | MEMBER_NAME) {
                continue;
            }
            let own = in_variable_scope(&tok, scope) || in_arrow_fn(&tok, scope);
            if !own {
                // `use ($x)` of a directly nested closure reads (or with `&` defines) $x here
                if parent.kind() == CLOSURE_USE_VAR {
                    let closure = parent.ancestors().find(|n| n.kind() == CLOSURE_EXPR);
                    if closure.and_then(|c| c.parent()).is_some_and(|p| {
                        p.ancestors()
                            .find(|n| {
                                matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | SOURCE_FILE)
                            })
                            .as_ref()
                            == Some(scope)
                    }) {
                        let by_ref = parent.children_with_tokens().any(|e| e.kind() == AMP);
                        let v = vars.entry(name).or_default();
                        if by_ref {
                            // the closure may write and read it
                            v.defs.push(tok.text_range().start());
                            v.reads.push((tok.text_range(), true));
                        } else {
                            v.reads.push((tok.text_range(), false));
                        }
                    }
                }
                continue;
            }
            let v = vars.entry(name).or_default();
            match self.occurrence(&tok, &parent, scope) {
                Occ::Def => {
                    v.defs.push(tok.text_range().start());
                    v.def_ranges.push(tok.text_range());
                }
                Occ::Param => {
                    v.defs.push(scope.text_range().start());
                    v.param = true;
                }
                Occ::DefAndRead => {
                    v.defs.push(tok.text_range().start());
                    v.reads.push((tok.text_range(), true));
                }
                Occ::Read { guarded } => v.reads.push((tok.text_range(), guarded)),
            }
        }
        let loops: Vec<TextRange> = scope
            .descendants()
            .filter(|n| matches!(n.kind(), WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT))
            .map(|n| n.text_range())
            .collect();
        let mut names: Vec<(&String, &VarInfo)> = vars.iter().collect();
        names.sort_by_key(|(n, _)| n.as_str());
        let mut diags = Vec::new();
        for (name, v) in names {
            if self.config.undefined_variables && scope.kind() != SOURCE_FILE {
                for (range, guarded) in &v.reads {
                    if *guarded {
                        continue;
                    }
                    let pos = range.start();
                    let defined_before = v.defs.iter().any(|d| *d < pos);
                    let defined_in_loop =
                        loops.iter().any(|l| l.contains(pos) && v.defs.iter().any(|d| l.contains(*d)));
                    if !defined_before && !defined_in_loop {
                        diags.push(Diagnostic::new(
                            *range,
                            Severity::Error,
                            DiagnosticCode::UndefinedVariable,
                            format!("Undefined variable '${name}'."),
                        ));
                    }
                }
            }
            if self.config.unused_symbols && v.reads.is_empty() && !v.param {
                for r in &v.def_ranges {
                    let mut d = Diagnostic::new(
                        *r,
                        Severity::Hint,
                        DiagnosticCode::UnusedSymbol,
                        format!("Symbol '${name}' is declared but not used."),
                    );
                    d.unnecessary = true;
                    diags.push(d);
                }
            }
        }
        self.out.extend(diags);
    }

    fn occurrence(&self, tok: &SyntaxToken, parent: &SyntaxNode, scope: &SyntaxNode) -> Occ {
        match parent.kind() {
            PARAM => {
                // parameters of the scope itself (closures nested in arrow functions excluded)
                Occ::Param
            }
            STATIC_VAR | CATCH_CLAUSE => Occ::Def,
            // by-reference `use` variables escape the closure
            CLOSURE_USE_VAR if parent.children_with_tokens().any(|e| e.kind() == AMP) => Occ::DefAndRead,
            CLOSURE_USE_VAR => Occ::Def,
            VARIABLE_EXPR => {
                let expr = parent.clone();
                let Some(p) = expr.parent() else { return Occ::Read { guarded: false } };
                let _ = scope;
                match p.kind() {
                    ASSIGN_EXPR if p.first_child().as_ref() == Some(&expr) => {
                        let op = p
                            .children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .find(|t| t.kind().is_assign_op())
                            .map(|t| t.kind());
                        match op {
                            Some(EQ) => Occ::Def,
                            Some(QUESTION2_EQ) => Occ::DefAndRead,
                            _ => Occ::DefAndRead,
                        }
                    }
                    FOREACH_VALUE | FOREACH_KEY | GLOBAL_STMT | STATIC_VAR => Occ::Def,
                    ARRAY_ELEMENT if is_destructuring(&p) => Occ::Def,
                    INDEX_EXPR if is_write_target(&p) && p.first_child().as_ref() == Some(&expr) => {
                        Occ::DefAndRead
                    }
                    ISSET_EXPR | EMPTY_EXPR => Occ::Read { guarded: true },
                    UNSET_STMT => Occ::Read { guarded: true },
                    BIN_EXPR
                        if p.first_child().as_ref() == Some(&expr)
                            && p.children_with_tokens().any(|e| e.kind() == QUESTION2) =>
                    {
                        Occ::Read { guarded: true }
                    }
                    PREFIX_EXPR if p.first_token().is_some_and(|t| t.kind() == AT) => {
                        Occ::Read { guarded: true }
                    }
                    ARG => {
                        if self.is_by_ref_arg(&p) {
                            Occ::DefAndRead
                        } else {
                            Occ::Read { guarded: false }
                        }
                    }
                    _ => {
                        // reads nested in isset(), e.g. isset($a['x'])
                        let guarded =
                            expr.ancestors().take(6).any(|a| matches!(a.kind(), ISSET_EXPR | EMPTY_EXPR));
                        let _ = tok;
                        Occ::Read { guarded }
                    }
                }
            }
            _ => Occ::Read { guarded: false },
        }
    }

    fn is_by_ref_arg(&self, arg: &SyntaxNode) -> bool {
        let Some(list) = arg.parent() else { return false };
        let Some(call) = list.parent() else { return false };
        let index = list.children().filter(|c| c.kind() == ARG).position(|a| &a == arg);
        let Some(index) = index else { return false };
        match crate::signature_help::callee_params(self.sema, &call) {
            Some(params) => {
                params.get(index).or_else(|| params.last().filter(|p| p.variadic)).is_some_and(|p| p.by_ref)
            }
            // unknown callee: assume it may define the variable
            None => true,
        }
    }
}

#[derive(Default)]
struct VarInfo {
    defs: Vec<TextSize>,
    def_ranges: Vec<TextRange>,
    /// (range, guarded by isset/empty/??/@)
    reads: Vec<(TextRange, bool)>,
    param: bool,
}

enum Occ {
    Def,
    Param,
    DefAndRead,
    Read { guarded: bool },
}

fn is_destructuring(element: &SyntaxNode) -> bool {
    let Some(array) = element.parent() else { return false };
    if !matches!(array.kind(), ARRAY_EXPR | LIST_EXPR) {
        return false;
    }
    // [$a, $b] = ... or foreach ($x as [$a, $b])
    let Some(p) = array.parent() else { return false };
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(&array),
        FOREACH_VALUE => true,
        ARRAY_ELEMENT => is_destructuring(&p),
        _ => false,
    }
}

fn is_write_target(expr: &SyntaxNode) -> bool {
    let Some(p) = expr.parent() else { return false };
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(expr),
        INDEX_EXPR | PROPERTY_FETCH_EXPR if p.first_child().as_ref() == Some(expr) => is_write_target(&p),
        FOREACH_VALUE | FOREACH_KEY => true,
        ARRAY_ELEMENT => is_destructuring(&p),
        UNSET_STMT => true,
        _ => false,
    }
}

fn in_arrow_fn(token: &SyntaxToken, scope: &SyntaxNode) -> bool {
    token.parent_ancestors().any(|a| a.kind() == ARROW_FN_EXPR)
        && token
            .parent_ancestors()
            .find(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | SOURCE_FILE))
            .as_ref()
            == Some(scope)
}

fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "_GET"
            | "_POST"
            | "_SERVER"
            | "_REQUEST"
            | "_COOKIE"
            | "_FILES"
            | "_ENV"
            | "_SESSION"
            | "GLOBALS"
            | "http_response_header"
            | "argc"
            | "argv"
    )
}
