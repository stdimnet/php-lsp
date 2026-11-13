//! Structural checks from Intelephense's diagnostic catalog: modifiers,
//! method bodies, `$this` / `parent` usage, arguments, readonly rules,
//! member visibility and "suspect code" (duplicate keys, assignments in
//! conditions, ...).

use std::collections::HashMap;

use php_hir::symbols::Visibility;
use php_hir::{Fqn, Type};
use php_syntax::ast::{self, AstNode, ClassLikeKind};
use php_syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange};

use crate::diagnostics::{Checker, Diagnostic, DiagnosticCode as Code, Severity};

fn modifiers(node: &SyntaxNode) -> Vec<SyntaxToken> {
    node.children()
        .find(|c| c.kind() == MODIFIER_LIST)
        .map(|m| {
            m.children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| !t.kind().is_trivia())
                .collect()
        })
        .unwrap_or_default()
}

fn has(mods: &[SyntaxToken], kind: SyntaxKind) -> bool {
    mods.iter().any(|t| t.kind() == kind)
}

fn find(mods: &[SyntaxToken], kind: SyntaxKind) -> Option<TextRange> {
    mods.iter().find(|t| t.kind() == kind).map(|t| t.text_range())
}

fn class_like_of(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors()
        .skip(1)
        .find(|n| matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS))
}

fn member_name_range(node: &SyntaxNode) -> TextRange {
    node.children().find_map(ast::MemberName::cast).map_or(node.text_range(), |m| m.syntax().text_range())
}

impl Checker<'_, '_> {
    fn err(&mut self, range: TextRange, code: Code, msg: impl Into<String>) {
        self.push(Diagnostic::new(range, Severity::Error, code, msg));
    }

    fn info(&mut self, range: TextRange, code: Code, msg: impl Into<String>) {
        self.push(Diagnostic::new(range, Severity::Information, code, msg));
    }

    fn hint(&mut self, range: TextRange, code: Code, msg: impl Into<String>) {
        self.push(Diagnostic::new(range, Severity::Hint, code, msg));
    }

    pub(crate) fn lint(&mut self, node: &SyntaxNode, index_ready: bool) {
        let lc = self.config.language_constraints;
        self.lint_compat(node, index_ready);
        match node.kind() {
            METHOD_DECL if lc => self.lint_method(node),
            PROPERTY_DECL if lc => self.lint_property(node),
            FUNCTION_DECL | CLOSURE_EXPR | ARROW_FN_EXPR if lc => self.lint_params(node),
            MODIFIER_LIST if lc => self.lint_visibility(node),
            VARIABLE_EXPR if lc && ast::text_without_trivia(node) == "$this" => self.lint_this(node),
            BREAK_STMT | CONTINUE_STMT if lc => self.lint_break(node),
            ARG_LIST if lc => self.lint_named_args(node, index_ready),
            NAME_EXPR if lc && ast::text_without_trivia(node).eq_ignore_ascii_case("parent") => {
                self.lint_parent(node)
            }
            ARRAY_EXPR if self.config.suspect_code => self.lint_array_keys(node),
            IF_STMT | ELSE_IF_CLAUSE if self.config.suspect_code => self.lint_condition(node),
            MATCH_EXPR if self.config.suspect_code => self.lint_match(node),
            BIN_EXPR if self.config.suspect_code => self.lint_concat(node),
            ASSIGN_EXPR if self.config.suspect_code && index_ready => self.lint_void_assignment(node),
            MEMBER_LIST if self.config.duplicate_symbols => self.lint_duplicate_members(node),
            SOURCE_FILE if self.config.duplicate_symbols => self.lint_duplicate_declarations(node),
            PROPERTY_FETCH_EXPR
            | METHOD_CALL_EXPR
            | STATIC_CALL_EXPR
            | STATIC_PROPERTY_FETCH_EXPR
            | CLASS_CONST_FETCH_EXPR
                if self.config.member_access && index_ready =>
            {
                self.lint_member_access(node)
            }
            _ => {}
        }
        if index_ready && matches!(node.kind(), CLASS_DECL | ENUM_DECL | INTERFACE_DECL | TRAIT_DECL) {
            self.lint_class(node);
        }
        if index_ready
            && matches!(
                node.kind(),
                CLASS_DECL
                    | ENUM_DECL
                    | INTERFACE_DECL
                    | TRAIT_DECL
                    | FUNCTION_DECL
                    | METHOD_DECL
                    | PROPERTY_DECL
                    | CLASS_CONST_DECL
                    | PARAM
                    | ANON_CLASS
                    | CLOSURE_EXPR
                    | ARROW_FN_EXPR
                    | ENUM_CASE
            )
        {
            self.lint_attributes(node);
        }
        if node.kind() == METHOD_DECL || node.kind() == FUNCTION_DECL {
            self.lint_doc_params(node);
            if index_ready {
                self.check_doc_compatibility(node);
            }
        }
    }

    // ------------------------------------------------------------------ declarations

    fn lint_visibility(&mut self, list: &SyntaxNode) {
        let tokens: Vec<SyntaxToken> = list
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| !t.kind().is_trivia())
            .collect();
        // `public private(set)`: the set visibility is followed by `(`
        let vis: Vec<&SyntaxToken> = tokens
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                matches!(t.kind(), PUBLIC_KW | PROTECTED_KW | PRIVATE_KW)
                    && tokens.get(i + 1).is_none_or(|n| n.kind() != L_PAREN)
            })
            .map(|(_, t)| t)
            .collect();
        if vis.len() > 1 {
            self.err(vis[1].text_range(), Code::MultipleVisibility, "Multiple visibility modifiers found.");
        }
    }

    fn lint_method(&mut self, method: &SyntaxNode) {
        let mods = modifiers(method);
        let Some(class) = class_like_of(method) else { return };
        let has_body = method.children().any(|c| c.kind() == BLOCK);
        let name_range = member_name_range(method);
        let name =
            method.children().find_map(ast::MemberName::cast).and_then(|m| m.text()).unwrap_or_default();
        let is_abstract = has(&mods, ABSTRACT_KW);
        if class.kind() == INTERFACE_DECL {
            if let Some(r) = find(&mods, PRIVATE_KW).or_else(|| find(&mods, PROTECTED_KW)) {
                self.err(r, Code::InterfaceMethodNotPublic, "Interface methods must be public.");
            }
            if let Some(r) = find(&mods, ABSTRACT_KW) {
                self.err(r, Code::AbstractNotAllowed, "Abstract modifier not allowed.");
            }
            if let Some(r) = find(&mods, FINAL_KW) {
                self.err(r, Code::FinalNotAllowed, "Final modifier not allowed.");
            }
            if has_body {
                self.err(name_range, Code::MethodBodyNotAllowed, "Method body not allowed.");
            }
        } else {
            if is_abstract {
                if let Some(r) = find(&mods, PRIVATE_KW).filter(|_| class.kind() != TRAIT_DECL) {
                    self.err(r, Code::PrivateAbstract, "Private method cannot be abstract.");
                }
                if let Some(r) = find(&mods, FINAL_KW) {
                    self.err(r, Code::FinalNotAllowed, "Final modifier not allowed.");
                }
                if has_body {
                    self.err(name_range, Code::MethodBodyNotAllowed, "Method body not allowed.");
                }
                let class_abstract = matches!(class.kind(), INTERFACE_DECL | TRAIT_DECL)
                    || has(&modifiers(&class), ABSTRACT_KW);
                if !class_abstract {
                    self.err(
                        find(&mods, ABSTRACT_KW).unwrap_or(name_range),
                        Code::AbstractInNonAbstract,
                        "Abstract method in non abstract class.",
                    );
                }
            } else if !has_body {
                self.err(name_range, Code::MethodBodyExpected, "Method body expected.");
            }
        }
        if name.eq_ignore_ascii_case("__construct") {
            if let Some(ret) = method.children().find(|c| c.kind() == RETURN_TYPE) {
                self.err(
                    ret.text_range(),
                    Code::ConstructorReturnType,
                    "Method '__construct' cannot declare a return type.",
                );
            }
        }
        self.lint_params(method);
    }

    fn lint_property(&mut self, prop: &SyntaxNode) {
        let mods = modifiers(prop);
        if let Some(r) = find(&mods, ABSTRACT_KW) {
            // PHP 8.4 allows abstract hooked properties
            let hooked = prop.descendants().any(|n| n.kind() == PROPERTY_HOOK_LIST);
            if !hooked {
                self.err(r, Code::AbstractNotAllowed, "Abstract modifier not allowed.");
            }
        }
        let readonly =
            has(&mods, READONLY_KW) || class_like_of(prop).is_some_and(|c| has(&modifiers(&c), READONLY_KW));
        if !readonly {
            return;
        }
        let Some(decl) = ast::PropertyDecl::cast(prop.clone()) else { return };
        // static properties of readonly classes are P1058
        if has(&mods, STATIC_KW) && has(&mods, READONLY_KW) {
            if let Some(r) = find(&mods, READONLY_KW) {
                self.err(r, Code::StaticReadonly, "Static property cannot be readonly.");
            }
        }
        if decl.ty().is_none() {
            for el in decl.elements() {
                if let Some(v) = el.variable() {
                    self.err(
                        v.text_range(),
                        Code::ReadonlyWithoutType,
                        "Readonly property must have a type.",
                    );
                }
            }
        }
        for el in decl.elements() {
            if el.syntax().children_with_tokens().any(|e| e.kind() == EQ) {
                if let Some(v) = el.variable() {
                    self.err(
                        v.text_range(),
                        Code::ReadonlyWithDefault,
                        "Readonly property cannot have a default value.",
                    );
                }
            }
        }
    }

    fn lint_params(&mut self, func: &SyntaxNode) {
        let Some(list) = func.children().find_map(ast::ParamList::cast) else { return };
        let params: Vec<ast::Param> = list.params().collect();
        let is_ctor = func.kind() == METHOD_DECL
            && func
                .children()
                .find_map(ast::MemberName::cast)
                .and_then(|m| m.text())
                .is_some_and(|n| n.eq_ignore_ascii_case("__construct"));
        let version = self.sema.ws.config().php_version;
        for (i, p) in params.iter().enumerate() {
            let Some(var) = p.variable() else { continue };
            let promoted = p.modifiers().is_some();
            if promoted && !is_ctor {
                // the whole parameter, without attributes (like Intelephense)
                let start = p
                    .syntax()
                    .children_with_tokens()
                    .find(|e| !e.kind().is_trivia() && e.kind() != ATTRIBUTE_GROUP)
                    .map_or(var.text_range().start(), |e| e.text_range().start());
                self.err(
                    TextRange::new(start, p.syntax().text_range().end()),
                    Code::PromotionOutsideConstructor,
                    "Cannot declare property promotion outside of constructor.",
                );
            }
            if promoted
                && p.syntax()
                    .children()
                    .find(|c| c.kind() == MODIFIER_LIST)
                    .is_some_and(|m| m.text().to_string().contains("readonly"))
                && p.ty().is_none()
            {
                self.err(var.text_range(), Code::ReadonlyWithoutType, "Readonly property must have a type.");
            }
            if p.is_variadic() {
                if p.default_value().is_some() {
                    self.err(
                        var.text_range(),
                        Code::VariadicDefault,
                        "A variadic parameter cannot have a default argument.",
                    );
                }
                if i + 1 < params.len() {
                    self.err(
                        var.text_range(),
                        Code::VariadicNotLast,
                        "Only the last parameter can be variadic.",
                    );
                }
            }
            if let Some(default) = p.default_value() {
                let required_after =
                    params[i + 1..].iter().any(|q| q.default_value().is_none() && !q.is_variadic());
                let null_default = ast::text_without_trivia(&default).eq_ignore_ascii_case("null");
                if required_after && !(null_default && p.ty().is_some()) {
                    self.hint(
                        var.text_range(),
                        Code::OptionalBeforeRequired,
                        "Optional parameter declared before required parameter is implicitly treated as required.",
                    );
                }
                // `Foo $x = null` without `?Foo` (deprecated in PHP 8.4)
                if null_default && version >= php_syntax::PhpVersion::new(8, 4) {
                    if let Some(ty) = p.ty() {
                        let t = ast::text_without_trivia(&ty).to_ascii_lowercase();
                        let nullable = ty.kind() == NULLABLE_TYPE
                            || t.split(['|', '(', ')'])
                                .any(|x| matches!(x.trim_start_matches('\\'), "null" | "mixed"));
                        if !nullable {
                            let mut d = Diagnostic::new(
                                ty.text_range(),
                                Severity::Hint,
                                Code::ImplicitlyNullable,
                                "Implicitly nullable parameters are deprecated.",
                            );
                            d.deprecated = true;
                            self.push(d);
                        }
                    }
                }
            }
        }
    }

    /// `@param T $name` for a parameter that does not exist.
    fn lint_doc_params(&mut self, func: &SyntaxNode) {
        if !self.config.suspect_code {
            return;
        }
        let Some(doc) = ast::doc_comment(func) else { return };
        let names: Vec<String> = func
            .children()
            .find_map(ast::ParamList::cast)
            .map(|l| l.params().filter_map(|p| p.name()).collect())
            .unwrap_or_default();
        let text = doc.text();
        let base = doc.text_range().start();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let line_start = offset;
            offset += line.len();
            let Some(tag) = line.find("@param") else { continue };
            let rest = &line[tag + 6..];
            if !rest.starts_with(char::is_whitespace) {
                continue; // @param-out, @param-closure-this, ...
            }
            // skip the type (brackets may contain spaces and `$this`)
            let mut depth = 0i32;
            let mut type_end = None;
            let mut started = false;
            for (i, c) in rest.char_indices() {
                match c {
                    '<' | '(' | '{' | '[' => depth += 1,
                    '>' | ')' | '}' | ']' => depth -= 1,
                    c if c.is_whitespace() && depth <= 0 && started => {
                        type_end = Some(i);
                        break;
                    }
                    '$' | '&' | '.' if depth <= 0 && !started => {
                        // no type: `@param $name`
                        type_end = Some(i);
                        break;
                    }
                    _ => {}
                }
                if !c.is_whitespace() {
                    started = true;
                }
            }
            let Some(type_end) = type_end else { continue };
            let after = &rest[type_end..];
            let trimmed = after.trim_start().trim_start_matches(['&', '.']);
            if !trimmed.starts_with('$') {
                continue;
            }
            let dollar = type_end + (after.len() - trimmed.len());
            let name: String =
                rest[dollar + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.is_empty() || names.iter().any(|n| n == &name) {
                continue;
            }
            // `...$name`: variadic spelled differently is still a name
            let start = base + php_syntax::TextSize::from((line_start + tag + 6 + dollar) as u32);
            self.info(
                TextRange::at(start, php_syntax::TextSize::from(name.len() as u32 + 1)),
                Code::UnknownParamName,
                format!("Unknown @param parameter name '${name}'."),
            );
        }
    }

    // ------------------------------------------------------------------ statements

    fn lint_this(&mut self, var: &SyntaxNode) {
        for anc in var.ancestors() {
            match anc.kind() {
                CLOSURE_EXPR | ARROW_FN_EXPR => {
                    if has(&modifiers(&anc), STATIC_KW)
                        || anc.children_with_tokens().any(|e| e.kind() == STATIC_KW)
                    {
                        self.err(
                            var.text_range(),
                            Code::ThisInStatic,
                            "Cannot use '$this' in non-object context.",
                        );
                    }
                    return; // may be bound to an object
                }
                METHOD_DECL => {
                    if has(&modifiers(&anc), STATIC_KW) {
                        self.err(
                            var.text_range(),
                            Code::ThisInStatic,
                            "Cannot use '$this' in non-object context.",
                        );
                    }
                    return;
                }
                FUNCTION_DECL => {
                    self.err(
                        var.text_range(),
                        Code::ThisInStatic,
                        "Cannot use '$this' in non-object context.",
                    );
                    return;
                }
                CLASS_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS | INTERFACE_DECL => return,
                _ => {}
            }
        }
        // top level: templates included from methods use `$this`, as do
        // view files; not reported
    }

    fn lint_break(&mut self, stmt: &SyntaxNode) {
        for anc in stmt.ancestors().skip(1) {
            match anc.kind() {
                WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT | SWITCH_STMT => return,
                FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR | SOURCE_FILE => break,
                _ => {}
            }
        }
        self.err(
            stmt.text_range(),
            Code::BreakOutsideLoop,
            "Break and continue can only be used in loop or switch context.",
        );
    }

    fn lint_parent(&mut self, name: &SyntaxNode) {
        let Some(class) = name
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS | INTERFACE_DECL))
        else {
            return;
        };
        if matches!(class.kind(), TRAIT_DECL) {
            return;
        }
        let has_parent = class.children().any(|c| c.kind() == EXTENDS_CLAUSE);
        if !has_parent {
            self.err(
                name.text_range(),
                Code::ParentWithoutParent,
                "Cannot use 'parent' in a class with no parent.",
            );
        }
    }

    fn lint_named_args(&mut self, list: &SyntaxNode, index_ready: bool) {
        let mut seen_named = false;
        let mut names: HashMap<String, TextRange> = HashMap::new();
        for arg in list.children().filter(|c| c.kind() == ARG) {
            let spread = arg.children_with_tokens().any(|e| e.kind() == DOT3);
            match arg
                .children()
                .find(|c| c.kind() == NAME)
                .filter(|_| arg.children_with_tokens().any(|e| e.kind() == COLON))
            {
                Some(n) => {
                    seen_named = true;
                    let name = ast::text_without_trivia(&n);
                    if names.insert(name.clone(), n.text_range()).is_some() {
                        self.err(
                            n.text_range(),
                            Code::NamedArgumentOverwrites,
                            format!("Named argument {name} overwrites previous argument."),
                        );
                    }
                }
                None if seen_named && !spread => {
                    self.err(
                        arg.text_range(),
                        Code::PositionalAfterNamed,
                        "Cannot use positional argument after named argument.",
                    );
                }
                None => {}
            }
        }
        if !index_ready {
            return;
        }
        let Some(call) = list.parent() else { return };
        let Some(params) = self.callee_params(&call) else { return };
        self.check_argument_types(&call, &params);
        self.check_by_ref_args(&call, &params);
        if names.is_empty() {
            return;
        }
        // unknown names (only without variadics)
        if params.iter().any(|p| p.variadic) {
            return;
        }
        for (name, range) in names {
            if !params.iter().any(|p| p.name == name) {
                self.err(range, Code::UnknownNamedArgument, format!("Unknown named argument ${name}."));
            }
        }
    }

    fn callee_params(&self, call: &SyntaxNode) -> Option<Vec<php_hir::symbols::ParamDef>> {
        let index = self.sema.index();
        match call.kind() {
            CALL_EXPR => {
                let callee = call.first_child().filter(|c| c.kind() == NAME_EXPR)?;
                let (_, f) = self
                    .sema
                    .resolve_function(&ast::text_without_trivia(&callee), callee.text_range().start())?;
                (self.sema.index().functions(&f.fqn).len() == 1).then(|| f.params.clone())
            }
            METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
                let name = call.children().find_map(ast::MemberName::cast)?.text()?;
                let lhs = call.first_child()?;
                let classes = if call.kind() == STATIC_CALL_EXPR {
                    crate::classify::static_lhs_classes(self.sema, &lhs)
                } else {
                    self.sema.infer().expr_type(&lhs).class_names()
                };
                let [class] = classes.as_slice() else { return None };
                if index.has_unknown_ancestor(class) {
                    return None;
                }
                let m = index.find_method(class, &name)?;
                (!m.def.is_magic).then(|| m.def.params.clone())
            }
            NEW_EXPR => {
                let class = call.children().find(|c| c.kind() == NAME_EXPR)?;
                let fqn = self.sema.resolve_class_name(&ast::text_without_trivia(&class), &class)?;
                if index.has_unknown_ancestor(&fqn) {
                    return None;
                }
                Some(index.find_method(&fqn, "__construct")?.def.params.clone())
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------------ member access

    fn lint_member_access(&mut self, node: &SyntaxNode) {
        let Some(m) = node.children().find_map(ast::MemberName::cast) else { return };
        let Some(name) = m.text() else { return };
        if m.syntax().children().next().is_some() {
            return; // dynamic name
        }
        let Some(lhs) = node.first_child() else { return };
        let classes: Vec<Fqn> = match node.kind() {
            STATIC_CALL_EXPR | STATIC_PROPERTY_FETCH_EXPR | CLASS_CONST_FETCH_EXPR => {
                crate::classify::static_lhs_classes(self.sema, &lhs)
            }
            _ => self
                .sema
                .infer()
                .expr_type(&lhs)
                .members()
                .into_iter()
                .filter_map(|t| match t {
                    Type::Named(n, _) => Some(n.clone()),
                    _ => None,
                })
                .collect(),
        };
        if classes.len() != 1 || name.eq_ignore_ascii_case("class") {
            return;
        }
        let class = &classes[0];
        let index = self.sema.index();
        let (kind, vis, owner) = match node.kind() {
            METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
                let Some(f) = index.find_method(class, &name) else { return };
                if f.def.is_magic {
                    return;
                }
                ("method", f.def.visibility, f.owner.clone())
            }
            CLASS_CONST_FETCH_EXPR => {
                let Some(c) = index.find_class_constant(class, &name) else { return };
                ("constant", c.def.visibility, c.owner.clone())
            }
            _ => {
                let Some(p) = index.find_property(class, &name) else { return };
                ("property", p.def.visibility, p.owner.clone())
            }
        };
        if vis == Visibility::Public || owner.kind == ClassLikeKind::Trait {
            return;
        }
        // `__get` / `__call` make inaccessible members reachable
        let magic = if kind == "method" { "__call" } else { "__get" };
        if kind != "constant" && index.find_method(class, magic).is_some() {
            return;
        }
        let caller = self.sema.enclosing_class(node);
        let allowed = match (&caller, vis) {
            (None, _) => false,
            (Some(c), Visibility::Private) => c == &owner.fqn,
            (Some(c), _) => {
                c == &owner.fqn
                    || index.ancestors(c).iter().any(|(_, a)| a.fqn == owner.fqn)
                    || index.ancestors(&owner.fqn).iter().any(|(_, a)| &a.fqn == c)
            }
        };
        // closures may be bound to the class (`Closure::bind`, `->call()`)
        let in_closure = node.ancestors().any(|a| matches!(a.kind(), CLOSURE_EXPR | ARROW_FN_EXPR));
        if allowed || in_closure || index.has_unknown_ancestor(class) {
            return;
        }
        let what = if vis == Visibility::Private { "private" } else { "protected" };
        let member = match kind {
            "property" => format!("{}::${name}", owner.fqn),
            _ => format!("{}::{name}", owner.fqn),
        };
        let scope = caller.map_or_else(|| "global".to_string(), |c| c.to_string());
        self.err(
            m.syntax().text_range(),
            Code::MemberAccess,
            format!("Cannot access {what} {kind} {member} from {scope} scope."),
        );
    }

    // ------------------------------------------------------------------ suspect code

    fn lint_array_keys(&mut self, array: &SyntaxNode) {
        let mut seen: HashMap<String, ()> = HashMap::new();
        for el in array.children().filter(|c| c.kind() == ARRAY_ELEMENT) {
            if !el.children_with_tokens().any(|e| e.kind() == FAT_ARROW) {
                continue;
            }
            let Some(key) = el.first_child().filter(|k| k.kind() == LITERAL) else { continue };
            let Some(tok) = key.first_token() else { continue };
            let norm = match tok.kind() {
                STRING if !tok.text().starts_with('"') || !tok.text().contains('$') => {
                    let v = crate::plugins::unquote(tok.text());
                    // "1" and 1 are the same key
                    if v.parse::<i64>().is_ok_and(|n| n.to_string() == v) {
                        v
                    } else {
                        format!("'{v}'")
                    }
                }
                INT_NUMBER => tok.text().replace('_', ""),
                _ => continue,
            };
            if seen.insert(norm, ()).is_some() {
                self.info(
                    key.text_range(),
                    Code::DuplicateArrayKey,
                    format!("Duplicate array key {}.", tok.text()),
                );
            }
        }
    }

    fn lint_condition(&mut self, stmt: &SyntaxNode) {
        let Some(cond) = stmt.children().find(|c| crate::infer::is_expr(c.kind())) else { return };
        // `if ($a = 1)`: assigning a constant value is most likely a typo for `==`
        let constant_rhs =
            cond.children().filter(|c| crate::infer::is_expr(c.kind())).nth(1).is_some_and(|r| {
                r.kind() == LITERAL || r.kind() == NAME_EXPR && !ast::text_without_trivia(&r).contains('(')
            });
        if cond.kind() == ASSIGN_EXPR && cond.children_with_tokens().any(|e| e.kind() == EQ) && constant_rhs {
            self.info(
                cond.text_range(),
                Code::AssignmentInCondition,
                "Possible unintended assignment in conditional expression.",
            );
        }
        // `if (a) ... elseif (a)`
        if stmt.kind() == IF_STMT {
            let mut seen = vec![ast::text_without_trivia(&cond)];
            for clause in stmt.children().filter(|c| c.kind() == ELSE_IF_CLAUSE) {
                let Some(c) = clause.children().find(|c| crate::infer::is_expr(c.kind())) else { continue };
                let text = ast::text_without_trivia(&c);
                if seen.contains(&text) {
                    self.info(
                        c.text_range(),
                        Code::DuplicateBranch,
                        format!("Duplicate branch expression {text}."),
                    );
                }
                seen.push(text);
            }
        }
    }

    fn lint_match(&mut self, m: &SyntaxNode) {
        let mut seen: Vec<String> = Vec::new();
        for cond in m.descendants().filter(|n| n.kind() == MATCH_CONDITIONS) {
            if cond.ancestors().find(|a| a.kind() == MATCH_EXPR).as_ref() != Some(m) {
                continue;
            }
            for c in cond.children().filter(|c| crate::infer::is_expr(c.kind()) && c.kind() == LITERAL) {
                let text = ast::text_without_trivia(&c);
                if seen.contains(&text) {
                    self.info(
                        c.text_range(),
                        Code::DuplicateBranch,
                        format!("Duplicate branch expression {text}."),
                    );
                }
                seen.push(text);
            }
        }
    }

    /// `'a' + 'b'`: `+` with a (non numeric) string literal operand.
    fn lint_concat(&mut self, bin: &SyntaxNode) {
        if !bin.children_with_tokens().any(|e| e.kind() == PLUS) {
            return;
        }
        let stringy = bin.children().filter(|c| crate::infer::is_expr(c.kind())).any(|c| {
            c.kind() == LITERAL
                && c.first_token().is_some_and(|t| {
                    t.kind() == STRING && {
                        let v = crate::plugins::unquote(t.text());
                        v.trim().parse::<f64>().is_err()
                    }
                })
        });
        if stringy {
            let op = bin
                .children_with_tokens()
                .find(|e| e.kind() == PLUS)
                .map(|e| e.text_range())
                .unwrap_or(bin.text_range());
            self.info(op, Code::StringConcatOperator, "Incorrect string concatenation operator.");
        }
    }

    fn lint_void_assignment(&mut self, assign: &SyntaxNode) {
        let Some(rhs) = assign.children().filter(|c| crate::infer::is_expr(c.kind())).nth(1) else { return };
        if !matches!(rhs.kind(), CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR) {
            return;
        }
        if matches!(self.sema.infer().expr_type(&rhs), Type::Void) {
            self.info(rhs.text_range(), Code::VoidAssignment, "Assigning return type of 'void'.");
        }
    }

    // ------------------------------------------------------------------ duplicates

    fn lint_duplicate_members(&mut self, list: &SyntaxNode) {
        let mut seen: HashMap<String, ()> = HashMap::new();
        for member in list.children() {
            let names: Vec<(String, TextRange)> = match member.kind() {
                METHOD_DECL => member
                    .children()
                    .find_map(ast::MemberName::cast)
                    .and_then(|m| {
                        Some((format!("m:{}", m.text()?.to_ascii_lowercase()), m.syntax().text_range()))
                    })
                    .into_iter()
                    .collect(),
                PROPERTY_DECL => ast::PropertyDecl::cast(member.clone())
                    .map(|d| {
                        d.elements()
                            .filter_map(|e| Some((format!("p:{}", e.name()?), e.variable()?.text_range())))
                            .collect()
                    })
                    .unwrap_or_default(),
                CLASS_CONST_DECL => member
                    .descendants()
                    .filter(|n| n.kind() == MEMBER_NAME)
                    // declared names, not `Other::NAME` in the value
                    .filter(|n| {
                        n.parent().is_some_and(|p| !matches!(p.kind(), ATTRIBUTE | CLASS_CONST_FETCH_EXPR))
                    })
                    .filter(|n| n.ancestors().all(|a| a.kind() != CLASS_CONST_FETCH_EXPR))
                    .map(|n| (format!("c:{}", ast::text_without_trivia(&n)), n.text_range()))
                    .collect(),
                ENUM_CASE => member
                    .children()
                    .find_map(ast::MemberName::cast)
                    .and_then(|m| Some((format!("c:{}", m.text()?), m.syntax().text_range())))
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            };
            for (key, range) in names {
                if seen.insert(key.clone(), ()).is_some() {
                    let shown = key.split_once(':').map_or(key.as_str(), |(_, n)| n).to_string();
                    let shown = if key.starts_with("p:") { format!("${shown}") } else { shown };
                    self.err(
                        range,
                        Code::DuplicateSymbol,
                        format!("Duplicate symbol declaration '{shown}'."),
                    );
                }
            }
        }
    }

    /// Classes / functions declared twice in one file (not conditionally).
    fn lint_duplicate_declarations(&mut self, root: &SyntaxNode) {
        let mut seen: HashMap<String, ()> = HashMap::new();
        let decls = root.descendants().filter(|n| {
            matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | FUNCTION_DECL)
        });
        for decl in decls {
            // only unconditional, top level (or namespace block) declarations
            let conditional = decl.ancestors().skip(1).any(|a| {
                !matches!(a.kind(), SOURCE_FILE | NAMESPACE_DECL | BLOCK)
                    || a.kind() == BLOCK && a.parent().is_some_and(|p| p.kind() != NAMESPACE_DECL)
            });
            if conditional {
                continue;
            }
            let Some(name) = decl.children().find_map(ast::Name::cast) else { continue };
            let scope = self.sema.scope_at(decl.text_range().start());
            let fqn = Fqn::join(scope.namespace.as_ref(), &name.text());
            let kind = if decl.kind() == FUNCTION_DECL { "f" } else { "c" };
            if seen.insert(format!("{kind}:{}", fqn.as_str().to_ascii_lowercase()), ()).is_some() {
                self.err(
                    name.syntax().text_range(),
                    Code::DuplicateSymbol,
                    format!("Duplicate symbol declaration '{}'.", fqn),
                );
            }
        }
    }
}
