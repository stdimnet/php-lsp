//! Statement- and expression-level checks from Intelephense's catalog that
//! depend on the targeted PHP version (deprecated casts and syntax) or on
//! PHP's declaration rules (namespaces, constant expressions, types, property
//! hooks and asymmetric visibility).

use std::collections::HashSet;

use php_syntax::ast::{self, AstNode};
use php_syntax::{PhpVersion, SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::diagnostics::{Checker, Diagnostic, DiagnosticCode as Code, Severity};

/// Names functions may not have (from PHP 8.0 on for `assert`).
const FORBIDDEN_FUNCTIONS: &[&str] = &["exit", "die"];

/// `(integer)` etc. and their canonical spelling (deprecated in PHP 8.5).
const NON_CANONICAL_CASTS: &[(&str, &str)] =
    &[("integer", "int"), ("boolean", "bool"), ("double", "float"), ("binary", "string")];

const TYPE_NODES: &[php_syntax::SyntaxKind] =
    &[NAMED_TYPE, NULLABLE_TYPE, UNION_TYPE, INTERSECTION_TYPE, PAREN_TYPE];

fn token(node: &SyntaxNode, kind: php_syntax::SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == kind)
}

/// The class-like declaration directly owning a member.
fn owner(member: &SyntaxNode) -> Option<SyntaxNode> {
    member
        .ancestors()
        .skip(1)
        .find(|a| matches!(a.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS))
}

impl Checker<'_, '_> {
    fn version(&self) -> PhpVersion {
        self.sema.ws.config().php_version
    }

    fn report(&mut self, range: TextRange, severity: Severity, code: &'static str, msg: String) {
        self.push(Diagnostic::new(range, severity, Code::Catalog(code), msg));
    }

    /// A deprecation hint (rendered struck through).
    fn deprecation(&mut self, range: TextRange, code: &'static str, msg: String) {
        if !self.config.deprecated {
            return;
        }
        let mut d = Diagnostic::new(range, Severity::Hint, Code::Catalog(code), msg);
        d.deprecated = true;
        self.push(d);
    }

    pub(crate) fn lint_compat(&mut self, node: &SyntaxNode, index_ready: bool) {
        let lc = self.config.language_constraints;
        match node.kind() {
            CAST_EXPR => self.lint_cast(node),
            TERNARY_EXPR => self.lint_nested_ternary(node),
            INDEX_EXPR if token(node, L_CURLY).is_some() => {
                if self.version() >= PhpVersion::PHP_7_4 && self.config.deprecated {
                    let v = self.version();
                    self.report(
                        node.text_range(),
                        Severity::Information,
                        "P1034",
                        format!("Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP {v}."),
                    );
                }
            }
            INTERPOLATION if token(node, DOLLAR_CURLY).is_some() && self.version() >= PhpVersion::PHP_8_2 => {
                self.deprecation(node.text_range(), "P1063", "Using ${} in strings is deprecated.".into());
            }
            CASE_CLAUSE | DEFAULT_CLAUSE if self.version() >= PhpVersion::PHP_8_5 => {
                if let Some(semi) = token(node, SEMICOLON) {
                    self.deprecation(
                        semi.text_range(),
                        "P1083",
                        "Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead.".into(),
                    );
                }
            }
            SHELL_EXEC if self.version() >= PhpVersion::PHP_8_5 => {
                self.deprecation(
                    node.text_range(),
                    "P1085",
                    "The backtick (`) operator is deprecated, use shell_exec() instead".into(),
                );
            }
            FUNCTION_DECL => self.lint_function_name(node),
            ASSIGN_EXPR if lc => {
                self.lint_assign_target(node);
                self.lint_writable(node);
                if index_ready {
                    self.lint_reference_to_hooked(node);
                }
            }
            PREFIX_EXPR | POSTFIX_EXPR | FOREACH_VALUE | FOREACH_KEY if lc => self.lint_writable(node),
            INDEX_EXPR if lc => self.lint_empty_index_read(node),
            HEREDOC if self.version() >= PhpVersion::PHP_7_3 => self.lint_heredoc_indentation(node),
            NEW_EXPR if lc && is_first_class_callable(node) && self.version() >= PhpVersion::PHP_8_1 => {
                if let Some(dots) = dots_of(node) {
                    self.report(
                        dots,
                        Severity::Error,
                        "P1053",
                        "Cannot create closure for new expression.".into(),
                    );
                }
            }
            METHOD_CALL_EXPR
                if lc
                    && token(node, NULLSAFE_ARROW).is_some()
                    && is_first_class_callable(node)
                    && self.version() >= PhpVersion::PHP_8_1 =>
            {
                if let Some(dots) = dots_of(node) {
                    self.report(
                        dots,
                        Severity::Error,
                        "P1054",
                        "Cannot combine nullsafe operator with closure creation.".into(),
                    );
                }
            }
            METHOD_DECL | CLASS_CONST_DECL if lc && self.version() >= PhpVersion::PHP_8_4 => {
                if let Some(r) = set_visibility(node) {
                    self.report(
                        r,
                        Severity::Error,
                        "P1066",
                        "Cannot use private(set), protected(set), public(set) on non-property.".into(),
                    );
                }
            }
            BIN_EXPR if lc && token(node, PIPE_GT).is_some() && self.version() >= PhpVersion::PHP_8_5 => {
                if let Some(rhs) = node.children().filter(|c| crate::infer::is_expr(c.kind())).nth(1) {
                    if rhs.kind() == ARROW_FN_EXPR {
                        self.report(
                            rhs.text_range(),
                            Severity::Error,
                            "P1092",
                            "Arrow functions on the right hand side of |> must be parenthesized.".into(),
                        );
                    }
                }
            }
            NAMESPACE_DECL if lc && self.version() < PhpVersion::PHP_8_0 => {
                self.lint_namespace_keywords(node)
            }
            SOURCE_FILE if lc => self.lint_namespaces(node),
            CONST_ELEMENT | PROPERTY_ELEMENT | PARAM | ENUM_CASE | ATTRIBUTE if lc => {
                if node.kind() == PARAM {
                    self.lint_static_param(node);
                }
                self.lint_const_expr(node)
            }
            NAMED_TYPE if lc => self.lint_type_context(node),
            UNION_TYPE if lc && self.version() >= PhpVersion::PHP_8_0 => self.lint_redundant_types(node),
            INTERSECTION_TYPE if lc && self.version() >= PhpVersion::PHP_8_1 => {
                self.lint_redundant_types(node)
            }
            PROPERTY_DECL if lc => self.lint_property_rules(node),
            EXPR_STMT if index_ready && self.version() >= PhpVersion::PHP_8_5 => self.lint_no_discard(node),
            UNSET_STMT if lc && index_ready => self.lint_unset_hooked(node),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ casts

    fn lint_cast(&mut self, node: &SyntaxNode) {
        let Some(cast) = token(node, CAST) else { return };
        let name: String =
            cast.text().chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase();
        let v = self.version();
        match name.as_str() {
            "real" if v >= PhpVersion::PHP_7_4 => self.deprecation(
                cast.text_range(),
                "P1031",
                format!("'(real)' cast deprecated in PHP 7.4. Targeting PHP {v}."),
            ),
            "unset" if v >= PhpVersion::PHP_7_2 => self.deprecation(
                cast.text_range(),
                "P1032",
                format!("'(unset)' cast deprecated in PHP 7.2. Targeting PHP {v}."),
            ),
            "void" if v >= PhpVersion::PHP_8_5 && self.config.language_constraints => {
                // only as a statement of its own
                if !node.parent().is_some_and(|p| matches!(p.kind(), EXPR_STMT | FOR_INIT | FOR_STEP)) {
                    self.report(
                        cast.text_range(),
                        Severity::Error,
                        "P1117",
                        "(void) cast cannot be used in this context.".into(),
                    );
                }
            }
            _ => {
                if v >= PhpVersion::PHP_8_5 {
                    if let Some((_, canonical)) = NON_CANONICAL_CASTS.iter().find(|(n, _)| *n == name) {
                        self.deprecation(
                            cast.text_range(),
                            "P1084",
                            format!("Non-canonical cast ({name}) is deprecated. Use ({canonical}) instead."),
                        );
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ ternaries

    /// `a ? b : c ? d : e` without parentheses (except `?:` chains).
    fn lint_nested_ternary(&mut self, node: &SyntaxNode) {
        if self.version() < PhpVersion::PHP_7_4 || !self.config.deprecated {
            return;
        }
        let Some(parent) = node.parent().filter(|p| p.kind() == TERNARY_EXPR) else { return };
        let short = |t: &SyntaxNode| t.children().filter(|c| crate::infer::is_expr(c.kind())).count() < 3;
        if short(node) && short(&parent) {
            return;
        }
        let v = self.version();
        self.report(
            node.text_range(),
            Severity::Information,
            "P1033",
            format!(
                "Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP {v}."
            ),
        );
    }

    // ------------------------------------------------------------------ functions

    fn lint_function_name(&mut self, node: &SyntaxNode) {
        let Some(name) = node.children().find(|c| c.kind() == NAME) else { return };
        let text = ast::text_without_trivia(&name);
        let lower = text.to_ascii_lowercase();
        if lower == "assert" && self.version() < PhpVersion::PHP_8_0 {
            self.deprecation(
                name.text_range(),
                "P1099",
                "Defining a custom assert() function is deprecated.".into(),
            );
        } else if (lower == "assert" || FORBIDDEN_FUNCTIONS.contains(&lower.as_str()))
            && self.config.language_constraints
        {
            self.report(
                name.text_range(),
                Severity::Error,
                "P1100",
                format!("Defining a function named \"{text}\" is not allowed."),
            );
        }
    }

    // ------------------------------------------------------------------ assignments

    /// P1025 `[] = $x`, P1115 `$string[] = ...`.
    fn lint_assign_target(&mut self, node: &SyntaxNode) {
        let Some(lhs) = node.first_child() else { return };
        if !node.children_with_tokens().any(|e| e.kind() == EQ) {
            return;
        }
        match lhs.kind() {
            ARRAY_EXPR | LIST_EXPR => {
                let empty = !lhs
                    .children()
                    .filter(|c| c.kind() == ARRAY_ELEMENT)
                    .any(|e| e.children().next().is_some());
                if empty {
                    self.report(
                        lhs.text_range(),
                        Severity::Error,
                        "P1025",
                        "Empty array destructure.".into(),
                    );
                }
            }
            INDEX_EXPR => {
                let mut parts = lhs.children().filter(|c| crate::infer::is_expr(c.kind()));
                let Some(base) = parts.next() else { return };
                if parts.next().is_some() || base.kind() != VARIABLE_EXPR {
                    return;
                }
                let ty = self.sema.infer().expr_type(&base);
                let string = match &ty {
                    php_hir::Type::String => true,
                    php_hir::Type::Literal(l) => l.starts_with(['\'', '"']),
                    _ => false,
                };
                if string {
                    self.report(
                        lhs.text_range(),
                        Severity::Error,
                        "P1115",
                        "Cannot use [] operator on string.".into(),
                    );
                }
            }
            _ => {}
        }
    }

    /// P1114: a by-reference parameter receiving a value that is not writable.
    pub(crate) fn check_by_ref_args(&mut self, call: &SyntaxNode, params: &[php_hir::symbols::ParamDef]) {
        if !self.config.language_constraints {
            return;
        }
        let Some(list) = call.children().find(|c| c.kind() == ARG_LIST) else { return };
        for (i, arg) in list.children().filter(|c| c.kind() == ARG).enumerate() {
            if arg.children_with_tokens().any(|e| matches!(e.kind(), DOT3 | COLON)) {
                return; // spread / named: positions unknown
            }
            let param = params.get(i).or_else(|| params.last().filter(|p| p.variadic));
            if !param.is_some_and(|p| p.by_ref) {
                continue;
            }
            let Some(expr) = arg.children().find(|c| crate::infer::is_expr(c.kind())) else { continue };
            // literals and operators; calls are left alone (a notice only)
            let not_writable = matches!(
                expr.kind(),
                LITERAL | INTERPOLATED_STRING | BIN_EXPR | PREFIX_EXPR | CAST_EXPR | CLASS_CONST_FETCH_EXPR
            ) || expr.kind() == ARRAY_EXPR;
            if not_writable {
                self.report(
                    expr.text_range(),
                    Severity::Error,
                    "P1114",
                    "Only variables should be passed by reference.".into(),
                );
            }
        }
    }

    // ------------------------------------------------------------------ namespaces

    /// P1105 / P1106 / P1107: placement and style of namespace declarations.
    fn lint_namespaces(&mut self, root: &SyntaxNode) {
        let stmts: Vec<SyntaxNode> = root.children().collect();
        let Some(first) = stmts.iter().position(|s| s.kind() == NAMESPACE_DECL) else { return };
        let kw = |n: &SyntaxNode| token(n, NAMESPACE_KW).map_or(n.text_range(), |t| t.text_range());
        let ignorable = |s: &SyntaxNode| matches!(s.kind(), DECLARE_STMT | INLINE_HTML_STMT | EMPTY_STMT);
        let mut out: Vec<(TextRange, &'static str, &'static str)> = Vec::new();
        if stmts[..first].iter().any(|s| !ignorable(s)) {
            out.push((
                kw(&stmts[first]),
                "P1105",
                "Namespace declaration must be the first statement in script or immediately after declare statement.",
            ));
        }
        let bracketed = |n: &SyntaxNode| n.children().any(|c| c.kind() == BLOCK);
        let style = bracketed(&stmts[first]);
        for s in &stmts[first + 1..] {
            if s.kind() == NAMESPACE_DECL && bracketed(s) != style {
                out.push((kw(s), "P1106", "Cannot mix bracketed and unbracketed namespace declarations."));
            } else if style && s.kind() != NAMESPACE_DECL && !ignorable(s) && s.kind() != HALT_COMPILER_STMT {
                out.push((s.text_range(), "P1107", "No code may exist outside of bracketed namespace."));
            }
        }
        for (r, code, msg) in out {
            self.report(r, Severity::Error, code, msg.into());
        }
    }

    // ------------------------------------------------------------------ constant expressions

    /// P1089 / P1090: calls, variables and closures in constant expressions
    /// (constants, defaults, enum case values, attribute arguments).
    fn lint_const_expr(&mut self, node: &SyntaxNode) {
        let values: Vec<SyntaxNode> = match node.kind() {
            ATTRIBUTE => node
                .children()
                .filter(|c| c.kind() == ARG_LIST)
                .flat_map(|l| l.children().filter(|a| a.kind() == ARG).collect::<Vec<_>>())
                .filter_map(|a| a.children().find(|c| crate::infer::is_expr(c.kind())))
                .collect(),
            // the default value follows `=`
            _ => {
                let Some(eq) = token(node, EQ) else { return };
                node.children()
                    .filter(|c| {
                        crate::infer::is_expr(c.kind()) && c.text_range().start() >= eq.text_range().end()
                    })
                    .take(1)
                    .collect()
            }
        };
        for value in values {
            self.check_const_value(&value);
        }
    }

    fn check_const_value(&mut self, value: &SyntaxNode) {
        let v = self.version();
        let mut stack = vec![value.clone()];
        while let Some(n) = stack.pop() {
            match n.kind() {
                CLOSURE_EXPR if v >= PhpVersion::PHP_8_5 => {
                    if token(&n, STATIC_KW).is_none() {
                        self.report(
                            n.text_range(),
                            Severity::Error,
                            "P1090",
                            "Closures in constant expressions must be static.".into(),
                        );
                    } else if n.children().any(|c| c.kind() == CLOSURE_USE) {
                        self.report(
                            n.text_range(),
                            Severity::Error,
                            "P1091",
                            "Cannot use(...) variables in constant expression.".into(),
                        );
                    }
                    return;
                }
                CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR if !is_first_class_callable(&n) => {
                    return self.invalid_const(value);
                }
                VARIABLE_EXPR | CLOSURE_EXPR | ARROW_FN_EXPR | SHELL_EXEC | INCLUDE_EXPR => {
                    return self.invalid_const(value);
                }
                _ => stack.extend(n.children()),
            }
        }
    }

    fn invalid_const(&mut self, value: &SyntaxNode) {
        self.report(
            value.text_range(),
            Severity::Error,
            "P1089",
            "Constant expression contains invalid operations.".into(),
        );
    }

    // ------------------------------------------------------------------ types

    /// P1048 `void` / `never` outside a return type, P1064 standalone
    /// `null` / `false` before PHP 8.2.
    fn lint_type_context(&mut self, ty: &SyntaxNode) {
        let text = ast::text_without_trivia(ty);
        let lower = text.to_ascii_lowercase();
        let parent = ty.parent();
        let parent_kind = parent.as_ref().map(|p| p.kind());
        match lower.as_str() {
            "void" | "never" => {
                if parent_kind == Some(RETURN_TYPE)
                    || (lower == "never" && self.version() < PhpVersion::PHP_8_1)
                {
                    return;
                }
                // unions are a syntax error before PHP 8
                if parent_kind == Some(UNION_TYPE) && self.version() < PhpVersion::PHP_8_0 {
                    return;
                }
                if !matches!(
                    parent_kind,
                    Some(
                        PARAM
                            | PROPERTY_DECL
                            | UNION_TYPE
                            | NULLABLE_TYPE
                            | INTERSECTION_TYPE
                            | CLASS_CONST_DECL
                    )
                ) {
                    return;
                }
                self.report(
                    ty.text_range(),
                    Severity::Error,
                    "P1048",
                    format!("Type '{text}' cannot be used in this context."),
                );
            }
            "null" | "false" => {
                let v = self.version();
                if v >= PhpVersion::PHP_8_2
                    || !matches!(parent_kind, Some(PARAM | PROPERTY_DECL | RETURN_TYPE | CLASS_CONST_DECL))
                {
                    return;
                }
                if v < PhpVersion::PHP_8_0 {
                    self.report(
                        ty.text_range(),
                        Severity::Error,
                        "P1022",
                        format!("PHP 8.2+ syntax found. Targeting PHP {v}."),
                    );
                } else {
                    self.report(
                        ty.text_range(),
                        Severity::Error,
                        "P1064",
                        format!("'{text}' cannot be used as a standalone type."),
                    );
                }
            }
            _ => {}
        }
    }

    /// P1065: the same type twice in a union.
    fn lint_redundant_types(&mut self, union: &SyntaxNode) {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for member in union.children().filter(|c| matches!(c.kind(), NAMED_TYPE | PAREN_TYPE)) {
            if member.kind() == PAREN_TYPE {
                // `(A&B)|(B&A)`
                let mut parts: Vec<String> = member
                    .descendants()
                    .filter(|n| n.kind() == NAMED_TYPE)
                    .map(|n| self.type_key(&n))
                    .collect();
                parts.sort();
                if !seen.insert(parts.join("&")) {
                    out.push(member.text_range());
                }
                continue;
            }
            if !seen.insert(self.type_key(&member)) {
                out.push(member.text_range());
            }
        }
        for r in out {
            self.report(r, Severity::Error, "P1065", "Redundant type.".into());
        }
    }

    /// A named type, normalised for comparisons.
    fn type_key(&self, member: &SyntaxNode) -> String {
        {
            let text = ast::text_without_trivia(member);
            let key = match text.to_ascii_lowercase().as_str() {
                t @ ("int" | "float" | "string" | "bool" | "false" | "true" | "null" | "array" | "object"
                | "mixed" | "iterable" | "callable" | "void" | "never" | "self" | "static"
                | "parent") => t.to_string(),
                _ => self
                    .sema
                    .resolve_class_name(&text, member)
                    .map_or(text.to_ascii_lowercase(), |f| f.as_str().to_ascii_lowercase()),
            };
            key
        }
    }

    // ------------------------------------------------------------------ properties

    /// Asymmetric visibility (P1067 / P1068 / P1069 / P1071), hook
    /// signatures (P1074) and interface properties (P1082).
    fn lint_property_rules(&mut self, prop: &SyntaxNode) {
        let Some(class) = owner(prop) else { return };
        let mods = prop.children().find(|c| c.kind() == MODIFIER_LIST);
        let tokens: Vec<SyntaxToken> = mods
            .iter()
            .flat_map(|m| m.children_with_tokens().filter_map(|e| e.into_token()))
            .filter(|t| !t.kind().is_trivia())
            .collect();
        let rank = |k: php_syntax::SyntaxKind| match k {
            PUBLIC_KW => Some(0),
            PROTECTED_KW => Some(1),
            PRIVATE_KW => Some(2),
            _ => None,
        };
        // `private(set)`: visibility keyword, `(`, `set`, `)`
        let mut get = None;
        let mut set: Option<(u8, TextRange)> = None;
        for (i, t) in tokens.iter().enumerate() {
            let Some(r) = rank(t.kind()) else { continue };
            if tokens.get(i + 1).is_some_and(|n| n.kind() == L_PAREN) {
                let end = tokens.get(i + 3).map_or(t.text_range().end(), |e| e.text_range().end());
                set = Some((r, TextRange::new(t.text_range().start(), end)));
            } else {
                get = Some(r);
            }
        }
        let hooks: Vec<SyntaxNode> = prop.descendants().filter(|n| n.kind() == PROPERTY_HOOK).collect();
        let has_hook_list = prop.descendants().any(|n| n.kind() == PROPERTY_HOOK_LIST);
        let is_static = tokens.iter().any(|t| t.kind() == STATIC_KW);
        let mut out: Vec<(TextRange, &'static str, String)> = Vec::new();
        // hooks and asymmetric visibility before PHP 8.4 are syntax errors already
        let modern = self.version() >= PhpVersion::PHP_8_4;
        if let Some((set_rank, set_range)) = set.filter(|_| modern) {
            if !prop.children().any(|c| TYPE_NODES.contains(&c.kind())) {
                out.push((
                    prop.text_range(),
                    "P1067",
                    "A property with asymmetric visibility must have a type.".into(),
                ));
            }
            if set_rank < get.unwrap_or(0) {
                out.push((
                    set_range,
                    "P1068",
                    "Set visibility must be equal to or less than get visibility.".into(),
                ));
            }
            if is_static && self.version() < PhpVersion::PHP_8_5 {
                if let Some(m) = &mods {
                    out.push((
                        m.text_range(),
                        "P1069",
                        "Static property cannot have asymmetric visibility.".into(),
                    ));
                }
            }
            // only a `get` hook that does not use the backing value
            let name = prop
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == VARIABLE)
                .map(|t| t.text()[1..].to_string());
            let hook_name = |h: &SyntaxNode| {
                h.children()
                    .find(|c| c.kind() == MEMBER_NAME)
                    .map(|m| ast::text_without_trivia(&m).to_ascii_lowercase())
            };
            if let Some(name) = name {
                let only_get = has_hook_list && hooks.iter().all(|h| hook_name(h).as_deref() == Some("get"));
                let backed = hooks.iter().any(|h| {
                    h.descendants().any(|n| {
                        n.kind() == PROPERTY_FETCH_EXPR
                            && n.first_child().is_some_and(|o| ast::text_without_trivia(&o) == "$this")
                            && n.children()
                                .find_map(ast::MemberName::cast)
                                .and_then(|m| m.text())
                                .is_some_and(|m| m == name)
                    })
                });
                if only_get && !hooks.is_empty() && !backed {
                    out.push((
                        set_range,
                        "P1071",
                        format!("Cannot specify set visibility for a get only virtual property ${name}."),
                    ));
                }
            }
        }
        for h in hooks.iter().filter(|_| modern) {
            let name = h
                .children()
                .find(|c| c.kind() == MEMBER_NAME)
                .map(|m| ast::text_without_trivia(&m).to_ascii_lowercase());
            let params = h.children().find(|c| c.kind() == PARAM_LIST);
            let count = params.as_ref().map(|l| l.children().filter(|c| c.kind() == PARAM).count());
            let invalid = match name.as_deref() {
                Some("get") => params.is_some(),
                Some("set") => count.is_some_and(|c| c != 1),
                _ => false,
            };
            if invalid {
                out.push((h.text_range(), "P1074", "Invalid property hook signature.".into()));
            }
        }
        // P1072: a default value for a virtual property
        if modern && has_hook_list {
            for el in prop.children().filter(|c| c.kind() == PROPERTY_ELEMENT) {
                let (Some(eq), Some(var)) = (token(&el, EQ), token(&el, VARIABLE)) else { continue };
                let name = var.text()[1..].to_string();
                let Some(value) = el.children().find(|c| crate::infer::is_expr(c.kind())) else { continue };
                // `set => expr` writes the backing value
                let short_set = el.descendants().filter(|n| n.kind() == PROPERTY_HOOK).any(|h| {
                    h.children()
                        .find(|c| c.kind() == MEMBER_NAME)
                        .is_some_and(|m| ast::text_without_trivia(&m).eq_ignore_ascii_case("set"))
                        && token(&h, FAT_ARROW).is_some()
                });
                let backed = short_set
                    || el.descendants().any(|n| {
                        n.kind() == PROPERTY_FETCH_EXPR
                            && n.first_child().is_some_and(|o| ast::text_without_trivia(&o) == "$this")
                            && n.children()
                                .find_map(ast::MemberName::cast)
                                .and_then(|m| m.text())
                                .is_some_and(|m| m == name)
                    });
                if !backed {
                    out.push((
                        TextRange::new(eq.text_range().start(), value.text_range().end()),
                        "P1072",
                        format!("Virtual property ${name} cannot have a default value."),
                    ));
                }
            }
        }
        if class.kind() == INTERFACE_DECL && !has_hook_list {
            out.push((prop.text_range(), "P1082", "Interfaces may only contain hooked properties.".into()));
        }
        for (r, code, msg) in out {
            self.report(r, Severity::Error, code, msg);
        }
    }

    /// P1076 `unset($o->hooked)`.
    fn lint_unset_hooked(&mut self, stmt: &SyntaxNode) {
        let index = self.sema.index();
        let mut out = Vec::new();
        for fetch in stmt.children().filter(|c| c.kind() == PROPERTY_FETCH_EXPR) {
            let Some(name) = fetch.children().find_map(ast::MemberName::cast).and_then(|m| m.text()) else {
                continue;
            };
            let Some(obj) = fetch.first_child() else { continue };
            let classes = self.sema.infer().expr_type(&obj).class_names();
            let [class] = classes.as_slice() else { continue };
            let Some(found) = index.find_property(class, &name) else { continue };
            if self.property_hooked(found.file, found.def.range) {
                out.push((
                    fetch.text_range(),
                    format!("Cannot unset hooked property {}::${name}.", found.owner.fqn),
                ));
            }
        }
        for (r, msg) in out {
            self.report(r, Severity::Error, "P1076", msg);
        }
    }

    fn property_hooked(&self, file: php_index::FileId, range: TextRange) -> bool {
        let root = if file == self.sema.file {
            self.sema.root.clone()
        } else {
            let Some(parse) = self.sema.ws.parse(file) else { return false };
            parse.syntax_node()
        };
        root.covering_element(range)
            .ancestors()
            .find(|n| matches!(n.kind(), PROPERTY_DECL | PARAM))
            .is_some_and(|p| p.descendants().any(|n| n.kind() == PROPERTY_HOOK_LIST))
    }

    // ------------------------------------------------------------------ writes

    /// P1024: assignments, `++` / `--` and `foreach` targets that cannot be
    /// written (calls, constants, literals).
    fn lint_writable(&mut self, node: &SyntaxNode) {
        let targets: Vec<SyntaxNode> = match node.kind() {
            ASSIGN_EXPR => {
                let Some(lhs) = node.first_child() else { return };
                if matches!(lhs.kind(), ARRAY_EXPR | LIST_EXPR) {
                    destructure_targets(&lhs)
                } else {
                    vec![lhs]
                }
            }
            PREFIX_EXPR | POSTFIX_EXPR => {
                if !node.children_with_tokens().any(|e| matches!(e.kind(), PLUS2 | MINUS2)) {
                    return;
                }
                node.children().filter(|c| crate::infer::is_expr(c.kind())).collect()
            }
            _ => {
                let Some(e) = node.children().find(|c| crate::infer::is_expr(c.kind())) else { return };
                if matches!(e.kind(), ARRAY_EXPR | LIST_EXPR) {
                    destructure_targets(&e)
                } else {
                    vec![e]
                }
            }
        };
        for t in targets {
            if matches!(
                t.kind(),
                CALL_EXPR
                    | METHOD_CALL_EXPR
                    | STATIC_CALL_EXPR
                    | LITERAL
                    | NAME_EXPR
                    | CLASS_CONST_FETCH_EXPR
            ) {
                self.report(t.text_range(), Severity::Error, "P1024", "Expression is not writable.".into());
            }
        }
    }

    /// P1026: `$a[]` where a value is read.
    fn lint_empty_index_read(&mut self, node: &SyntaxNode) {
        if node.children().filter(|c| crate::infer::is_expr(c.kind())).count() != 1
            || token(node, L_BRACK).is_none()
        {
            return;
        }
        // climb through accesses based on this one (`$a[]['k'] = 1`, `$a[]->p = 1`)
        let mut cur = node.clone();
        while let Some(p) = cur.parent() {
            let based_on = p.first_child().as_ref() == Some(&cur)
                && matches!(p.kind(), INDEX_EXPR | PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR);
            let element = matches!(p.kind(), ARRAY_ELEMENT | ARRAY_EXPR | LIST_EXPR);
            if !(based_on || element) {
                break;
            }
            cur = p;
        }
        let Some(parent) = cur.parent() else { return };
        let written = match parent.kind() {
            ASSIGN_EXPR => {
                parent.first_child().as_ref() == Some(&cur)
                    || parent.children_with_tokens().any(|e| e.kind() == AMP)
            }
            FOREACH_VALUE | FOREACH_KEY | ARG | UNSET_STMT | ISSET_EXPR | GLOBAL_STMT | STATIC_VAR => true,
            PREFIX_EXPR => parent.children_with_tokens().any(|e| matches!(e.kind(), AMP | PLUS2 | MINUS2)),
            POSTFIX_EXPR => true,
            _ => false,
        };
        if !written {
            self.report(node.text_range(), Severity::Error, "P1026", "Key expected in read context.".into());
        }
    }

    /// P1073 `$r = &$o->hooked`.
    fn lint_reference_to_hooked(&mut self, node: &SyntaxNode) {
        if !node.children_with_tokens().any(|e| e.kind() == AMP) {
            return;
        }
        let Some(fetch) = node.children().filter(|c| crate::infer::is_expr(c.kind())).nth(1) else { return };
        if fetch.kind() != PROPERTY_FETCH_EXPR {
            return;
        }
        let Some(name) = fetch.children().find_map(ast::MemberName::cast).and_then(|m| m.text()) else {
            return;
        };
        let Some(obj) = fetch.first_child() else { return };
        let classes = self.sema.infer().expr_type(&obj).class_names();
        let [class] = classes.as_slice() else { return };
        let index = self.sema.index();
        let Some(found) = index.find_property(class, &name) else { return };
        if self.property_hooked(found.file, found.def.range) {
            let msg = format!("Cannot indirectly modify property {}::${name}.", found.owner.fqn);
            self.report(node.text_range(), Severity::Error, "P1073", msg);
        }
    }

    // ------------------------------------------------------------------ heredoc

    /// P1027 / P1028: the body of a flexible heredoc / nowdoc must be indented
    /// at least like the closing marker, with the same whitespace.
    fn lint_heredoc_indentation(&mut self, node: &SyntaxNode) {
        let (Some(start), Some(end)) = (token(node, HEREDOC_START), token(node, HEREDOC_END)) else { return };
        let end_text = end.text();
        let indent: &str = &end_text[..end_text.len() - end_text.trim_start_matches([' ', '\t']).len()];
        if indent.is_empty() {
            return;
        }
        let text = &self.sema.text;
        let body_start = usize::from(start.text_range().end());
        let body_end = usize::from(end.text_range().start()).saturating_sub(1).max(body_start);
        let body_range = TextRange::new(start.text_range().end(), TextSize::from(body_end as u32));
        if indent.contains(' ') && indent.contains('\t') {
            let r = TextRange::new(end.text_range().start() - TextSize::from(1), end.text_range().end());
            let r = if body_end > body_start { r } else { end.text_range() };
            self.report(
                r,
                Severity::Error,
                "P1028",
                "Invalid indentation. Mixed spaces and tabs are not allowed.".into(),
            );
            return;
        }
        let ch = indent.chars().next().unwrap_or(' ');
        for line in text[body_start..body_end].split('\n') {
            if line.trim().is_empty() {
                continue;
            }
            let lead: &str = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
            if lead.starts_with(indent) {
                continue;
            }
            if lead.chars().any(|c| c != ch) {
                self.report(
                    body_range,
                    Severity::Error,
                    "P1028",
                    "Invalid indentation. Mixed spaces and tabs are not allowed.".into(),
                );
                return;
            }
            {
                self.report(
                    body_range,
                    Severity::Error,
                    "P1027",
                    format!(
                        "Unexpected indentation level. Expected at least {}. Found {}.",
                        indent.len(),
                        lead.len()
                    ),
                );
                return;
            }
        }
    }

    // ------------------------------------------------------------------ declarations

    /// P1081: `static` on a parameter.
    fn lint_static_param(&mut self, param: &SyntaxNode) {
        let Some(mods) = param.children().find(|c| c.kind() == MODIFIER_LIST) else { return };
        if let Some(t) = token(&mods, STATIC_KW) {
            self.report(t.text_range(), Severity::Error, "P1081", "Static modifier not allowed.".into());
        }
    }

    /// P1040: reserved words in namespace names before PHP 8.
    fn lint_namespace_keywords(&mut self, ns: &SyntaxNode) {
        let Some(name) = ns.children().find(|c| c.kind() == QUALIFIED_NAME) else { return };
        let reserved = name
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == IDENT || t.kind().is_keyword())
            .any(|t| RESERVED_WORDS.contains(&t.text().to_ascii_lowercase().as_str()));
        if reserved {
            self.report(
                name.text_range(),
                Severity::Error,
                "P1040",
                "Namespace names can only contain keywords in PHP 8+.".into(),
            );
        }
    }

    // ------------------------------------------------------------------ results

    /// P1126: the result of a `#[\NoDiscard]` function is not used.
    fn lint_no_discard(&mut self, stmt: &SyntaxNode) {
        let Some(call) = stmt.first_child() else { return };
        let index = self.sema.index();
        let (def, label) = match call.kind() {
            CALL_EXPR => {
                let Some(callee) = call.first_child().filter(|c| c.kind() == NAME_EXPR) else { return };
                let Some((_, f)) = self
                    .sema
                    .resolve_function(&ast::text_without_trivia(&callee), callee.text_range().start())
                else {
                    return;
                };
                (f, format!("{}()", f.name))
            }
            METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
                let Some(name) = call.children().find_map(ast::MemberName::cast).and_then(|m| m.text())
                else {
                    return;
                };
                let Some(lhs) = call.first_child() else { return };
                let classes = if call.kind() == STATIC_CALL_EXPR {
                    crate::classify::static_lhs_classes(self.sema, &lhs)
                } else {
                    self.sema.infer().expr_type(&lhs).class_names()
                };
                let [class] = classes.as_slice() else { return };
                let Some(m) = index.find_method(class, &name) else { return };
                (m.def, format!("{}::{}()", m.owner.fqn, m.def.name))
            }
            _ => return,
        };
        if def.no_discard {
            self.report(
                call.text_range(),
                Severity::Warning,
                "P1126",
                format!("The return value of {label} is expected to be consumed."),
            );
        }
    }
}

/// PHP's reserved words (not allowed in namespace names before PHP 8).
const RESERVED_WORDS: &[&str] = &[
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "eval",
    "exit",
    "extends",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "or",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "xor",
    "yield",
];

/// The variables of a destructuring pattern (`[$a, [$b]] = ...`).
fn destructure_targets(pattern: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    for el in pattern.children().filter(|c| c.kind() == ARRAY_ELEMENT) {
        // `'key' => $value`: only the value is written
        let Some(value) = el.children().filter(|c| crate::infer::is_expr(c.kind())).last() else { continue };
        if matches!(value.kind(), ARRAY_EXPR | LIST_EXPR) {
            out.extend(destructure_targets(&value));
        } else {
            out.push(value);
        }
    }
    out
}

/// The `...` of a first-class callable.
fn dots_of(call: &SyntaxNode) -> Option<TextRange> {
    let list = call.children().find(|c| c.kind() == ARG_LIST)?;
    list.descendants_with_tokens().find(|e| e.kind() == DOT3).map(|e| e.text_range())
}

/// `private(set)` in the modifiers of a declaration.
fn set_visibility(decl: &SyntaxNode) -> Option<TextRange> {
    let mods = decl.children().find(|c| c.kind() == MODIFIER_LIST)?;
    let toks: Vec<SyntaxToken> = mods
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .collect();
    toks.iter().enumerate().find_map(|(i, t)| {
        (matches!(t.kind(), PUBLIC_KW | PROTECTED_KW | PRIVATE_KW)
            && toks.get(i + 1).is_some_and(|n| n.kind() == L_PAREN))
        .then(|| {
            TextRange::new(
                t.text_range().start(),
                toks.get(i + 3).map_or(t.text_range().end(), |e| e.text_range().end()),
            )
        })
    })
}

/// `strlen(...)`: allowed in constant expressions since PHP 8.5.
fn is_first_class_callable(call: &SyntaxNode) -> bool {
    call.children().find(|c| c.kind() == ARG_LIST).is_some_and(|l| {
        let args: Vec<SyntaxNode> = l.children().filter(|c| c.kind() == ARG).collect();
        args.len() == 1 && ast::text_without_trivia(&args[0]) == "..."
    })
}
