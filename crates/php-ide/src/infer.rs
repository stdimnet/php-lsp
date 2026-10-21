//! Local type inference for expressions and variables.

use php_hir::phpdoc::DocBlock;
use php_hir::symbols::{FunctionDef, TypeResolver};
use php_hir::{Fqn, Type};
use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, TextSize};

use crate::sema::Sema;

const MAX_DEPTH: u32 = 40;

pub(crate) fn is_expr(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        LITERAL
            | INTERPOLATED_STRING
            | HEREDOC
            | SHELL_EXEC
            | VARIABLE_EXPR
            | DYNAMIC_VARIABLE_EXPR
            | NAME_EXPR
            | ARRAY_EXPR
            | LIST_EXPR
            | CALL_EXPR
            | NEW_EXPR
            | PROPERTY_FETCH_EXPR
            | METHOD_CALL_EXPR
            | STATIC_PROPERTY_FETCH_EXPR
            | STATIC_CALL_EXPR
            | CLASS_CONST_FETCH_EXPR
            | INDEX_EXPR
            | BIN_EXPR
            | PREFIX_EXPR
            | POSTFIX_EXPR
            | ASSIGN_EXPR
            | TERNARY_EXPR
            | CAST_EXPR
            | PAREN_EXPR
            | CLOSURE_EXPR
            | ARROW_FN_EXPR
            | MATCH_EXPR
            | ISSET_EXPR
            | EMPTY_EXPR
            | EVAL_EXPR
            | INCLUDE_EXPR
            | EXIT_EXPR
            | PRINT_EXPR
            | CLONE_EXPR
            | INSTANCEOF_EXPR
            | YIELD_EXPR
            | YIELD_FROM_EXPR
            | THROW_EXPR
            | MAGIC_CONST_EXPR
    )
}

pub(crate) struct Infer<'s, 'a> {
    sema: &'s Sema<'a>,
}

/// Child expression nodes of `node`.
fn expr_children(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> {
    node.children().filter(|c| is_expr(c.kind()))
}

fn literal_type(text: &str) -> Type {
    let t = text.trim();
    if t.starts_with('\'') || t.starts_with('"') || t.starts_with("<<<") {
        Type::String
    } else if t.eq_ignore_ascii_case("true") {
        Type::True
    } else if t.eq_ignore_ascii_case("false") {
        Type::False
    } else if t.eq_ignore_ascii_case("null") {
        Type::Null
    } else if t.starts_with('[') || t.to_ascii_lowercase().starts_with("array(") {
        Type::Array(None, None)
    } else if t.parse::<i64>().is_ok() || t.starts_with("0x") || t.starts_with("0b") {
        Type::Int
    } else if t.parse::<f64>().is_ok() {
        Type::Float
    } else {
        Type::Mixed
    }
}

impl<'s, 'a> Infer<'s, 'a> {
    pub fn new(sema: &'s Sema<'a>) -> Self {
        Infer { sema }
    }

    /// Infers the type of an expression. `self`/`static`/`parent` are resolved.
    pub fn expr_type(&self, node: &SyntaxNode) -> Type {
        let key = (node.text_range(), node.kind());
        if let Some(t) = self.sema.expr_cache.borrow().get(&key) {
            return t.clone();
        }
        let depth = &self.sema.infer_depth;
        if depth.get() > MAX_DEPTH {
            self.sema.infer_truncated.set(self.sema.infer_truncated.get() + 1);
            return Type::Mixed;
        }
        let truncated = self.sema.infer_truncated.get();
        depth.set(depth.get() + 1);
        let t = self.expr_type_inner(node);
        depth.set(depth.get() - 1);
        if self.sema.infer_truncated.get() == truncated && !self.sema.in_loop_carried.get() {
            self.sema.expr_cache.borrow_mut().insert(key, t.clone());
        }
        t
    }

    fn self_class(&self, node: &SyntaxNode) -> Option<Fqn> {
        self.sema.enclosing_class(node)
    }

    fn resolve_rel(&self, t: &Type, owner: Option<&Fqn>, receiver: Option<&Fqn>) -> Type {
        let parent = owner.and_then(|o| self.sema.index().parent_of(o));
        t.resolve_relative(owner, receiver.or(owner), parent.as_ref())
    }

    fn expr_type_inner(&self, node: &SyntaxNode) -> Type {
        match node.kind() {
            LITERAL => match node.first_token().map(|t| t.kind()) {
                Some(INT_NUMBER) => Type::Int,
                Some(FLOAT_NUMBER) => Type::Float,
                Some(STRING) | Some(IDENT) => Type::String,
                _ => Type::Mixed,
            },
            INTERPOLATED_STRING | HEREDOC | SHELL_EXEC => Type::String,
            MAGIC_CONST_EXPR => {
                if node.first_token().is_some_and(|t| t.kind() == LINE_C_KW) {
                    Type::Int
                } else {
                    Type::String
                }
            }
            VARIABLE_EXPR => {
                let Some(name) = ast::VariableExpr::cast(node.clone()).and_then(|v| v.name()) else {
                    return Type::Mixed;
                };
                if name == "this" {
                    return self.self_class(node).map(|c| Type::Named(c, Vec::new())).unwrap_or(Type::Object);
                }
                self.variable_type(&name, node)
            }
            PAREN_EXPR | CLONE_EXPR => {
                expr_children(node).next().map(|e| self.expr_type(&e)).unwrap_or(Type::Mixed)
            }
            ASSIGN_EXPR => {
                let op = node
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .find(|t| t.kind().is_assign_op());
                match op.map(|t| t.kind()) {
                    Some(DOT_EQ) => Type::String,
                    Some(EQ) | Some(QUESTION2_EQ) | None => {
                        expr_children(node).nth(1).map(|e| self.expr_type(&e)).unwrap_or(Type::Mixed)
                    }
                    _ => Type::union(vec![Type::Int, Type::Float]),
                }
            }
            NEW_EXPR => {
                if let Some(anon) = node.children().find(|c| c.kind() == ANON_CLASS) {
                    let fqn = php_hir::symbols::anon_class_fqn(&anon);
                    if self.sema.index().class(&fqn).is_some() {
                        return Type::Named(fqn, Vec::new());
                    }
                    let ext =
                        anon.children().find_map(ast::ExtendsClause::cast).and_then(|e| e.names().next());
                    return match ext {
                        Some(n) => Type::Named(
                            self.sema.scope_at(n.syntax().text_range().start()).resolve_class(&n.text()),
                            Vec::new(),
                        ),
                        None => Type::Object,
                    };
                }
                let Some(class) = node.children().find(|c| c.kind() != ARG_LIST) else { return Type::Object };
                if class.kind() == NAME_EXPR {
                    let text = ast::text_without_trivia(&class);
                    return self
                        .sema
                        .resolve_class_name(&text, &class)
                        .map(|c| Type::Named(c, Vec::new()))
                        .unwrap_or(Type::Object);
                }
                // `new $class` with `$class` a class-string
                let t = self.expr_type(&class);
                let mut out = Vec::new();
                for m in t.members() {
                    match m {
                        Type::ClassString(Some(inner)) => out.push((**inner).clone()),
                        Type::Named(..) => out.push(m.clone()),
                        _ => {}
                    }
                }
                if out.is_empty() {
                    Type::Object
                } else {
                    Type::union(out)
                }
            }
            ARRAY_EXPR => self.array_type(node),
            NAME_EXPR => {
                let text = ast::text_without_trivia(node);
                match text.to_ascii_lowercase().trim_start_matches('\\') {
                    "true" => Type::True,
                    "false" => Type::False,
                    "null" => Type::Null,
                    _ => match self.sema.resolve_constant(&text, node.text_range().start()) {
                        Some((_, c)) => c.value.as_deref().map(literal_type).unwrap_or(Type::Mixed),
                        None => Type::Mixed,
                    },
                }
            }
            // first-class callable syntax: `strlen(...)`, `$o->m(...)`
            CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR if is_first_class_callable(node) => {
                Type::Named(Fqn::new("Closure"), Vec::new())
            }
            CALL_EXPR => self.call_type(node),
            METHOD_CALL_EXPR | STATIC_CALL_EXPR => self.method_call_type(node),
            PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR => {
                self.narrowed_expr(node).unwrap_or_else(|| self.property_type(node))
            }
            CLASS_CONST_FETCH_EXPR => self.class_const_type(node),
            INDEX_EXPR => {
                let mut children = expr_children(node);
                let Some(base) = children.next() else { return Type::Mixed };
                let key = children.next().and_then(|k| {
                    let t = k.first_token()?;
                    if k.kind() == LITERAL && t.kind() == STRING {
                        let s = t.text();
                        Some(crate::plugins::unquote(s))
                    } else if k.kind() == LITERAL {
                        Some(t.text().to_string())
                    } else {
                        None
                    }
                });
                let base_ty = self.expr_type(&base);
                if base_ty == Type::String {
                    return Type::String;
                }
                // ArrayAccess objects: offsetGet()
                for c in base_ty.class_names() {
                    if let Some(m) = self.sema.index().find_method(&c, "offsetGet") {
                        if let Some(t) = m.def.effective_return_type() {
                            if !t.is_mixed() {
                                return self.resolve_rel(&t, Some(&m.owner.fqn), Some(&c));
                            }
                        }
                    }
                }
                base_ty.index(key.as_deref()).unwrap_or(Type::Mixed)
            }
            TERNARY_EXPR => {
                let parts: Vec<SyntaxNode> = expr_children(node).collect();
                match parts.len() {
                    3 => Type::union(vec![self.expr_type(&parts[1]), self.expr_type(&parts[2])]),
                    2 => {
                        Type::union(vec![self.expr_type(&parts[0]).without_null(), self.expr_type(&parts[1])])
                    }
                    _ => Type::Mixed,
                }
            }
            BIN_EXPR => {
                let op = node
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .find(|t| !t.kind().is_trivia())
                    .map(|t| t.kind());
                let parts: Vec<SyntaxNode> = expr_children(node).collect();
                match op {
                    Some(QUESTION2) if parts.len() == 2 => {
                        Type::union(vec![self.expr_type(&parts[0]).without_null(), self.expr_type(&parts[1])])
                    }
                    Some(DOT) => Type::String,
                    Some(PLUS | MINUS | STAR | STAR2 | PERCENT) => {
                        let tys: Vec<Type> = parts.iter().map(|p| self.expr_type(p)).collect();
                        if tys.iter().all(|t| *t == Type::Int) {
                            Type::Int
                        } else if tys.contains(&Type::Float) {
                            Type::Float
                        } else if op == Some(PLUS) && tys.iter().all(|t| t.iterable_value().is_some()) {
                            tys[0].clone()
                        } else if op == Some(PLUS)
                            && tys
                                .iter()
                                .any(|t| matches!(t, Type::Array(..) | Type::List(_) | Type::Shape(_)))
                        {
                            // array union with an operand of unknown type
                            Type::Array(None, None)
                        } else if tys.iter().any(|t| t.is_mixed()) {
                            Type::Mixed
                        } else {
                            Type::union(vec![Type::Int, Type::Float])
                        }
                    }
                    Some(SLASH) => Type::union(vec![Type::Int, Type::Float]),
                    Some(AMP | PIPE | CARET | SHL | SHR) => Type::Int,
                    Some(SPACESHIP) => Type::Int,
                    Some(PIPE_GT) => Type::Mixed,
                    _ => Type::Bool,
                }
            }
            INSTANCEOF_EXPR | ISSET_EXPR | EMPTY_EXPR => Type::Bool,
            PREFIX_EXPR => {
                let op = node.first_token().map(|t| t.kind());
                let inner = expr_children(node).next();
                match op {
                    Some(BANG) => Type::Bool,
                    Some(AT) | Some(AMP) => inner.map(|e| self.expr_type(&e)).unwrap_or(Type::Mixed),
                    Some(TILDE) => Type::Int,
                    _ => match inner.map(|e| self.expr_type(&e)) {
                        Some(Type::Int) => Type::Int,
                        Some(Type::Float) => Type::Float,
                        _ => Type::union(vec![Type::Int, Type::Float]),
                    },
                }
            }
            POSTFIX_EXPR => expr_children(node).next().map(|e| self.expr_type(&e)).unwrap_or(Type::Int),
            CAST_EXPR => {
                let cast = node.first_token().map(|t| t.text().to_ascii_lowercase()).unwrap_or_default();
                let cast: String = cast.chars().filter(|c| c.is_ascii_alphabetic()).collect();
                match cast.as_str() {
                    "int" | "integer" => Type::Int,
                    "bool" | "boolean" => Type::Bool,
                    "float" | "double" | "real" => Type::Float,
                    "string" | "binary" => Type::String,
                    "array" => Type::Array(None, None),
                    "object" => Type::named("stdClass"),
                    "unset" => Type::Null,
                    _ => Type::Mixed,
                }
            }
            CLOSURE_EXPR | ARROW_FN_EXPR => {
                let ret = node
                    .children()
                    .find_map(ast::ReturnType::cast)
                    .and_then(|r| r.ty())
                    .map(|t| self.syntax_type(&t, node));
                let params = node
                    .children()
                    .find_map(ast::ParamList::cast)
                    .map(|pl| {
                        pl.params()
                            .map(|p| p.ty().map(|t| self.syntax_type(&t, node)).unwrap_or(Type::Mixed))
                            .collect()
                    })
                    .unwrap_or_default();
                let ret = ret.or_else(|| {
                    if node.kind() == ARROW_FN_EXPR {
                        node.children().filter(|c| is_expr(c.kind())).last().map(|b| self.expr_type(&b))
                    } else {
                        None
                    }
                });
                Type::Closure(params, ret.map(Box::new))
            }
            MATCH_EXPR => {
                let arms: Vec<Type> = node
                    .children()
                    .filter(|c| c.kind() == MATCH_ARM)
                    .filter_map(|arm| expr_children(&arm).last())
                    .map(|e| self.expr_type(&e))
                    .collect();
                if arms.is_empty() {
                    Type::Mixed
                } else {
                    Type::union(arms)
                }
            }
            PRINT_EXPR => Type::Int,
            EXIT_EXPR | THROW_EXPR => Type::Never,
            _ => Type::Mixed,
        }
    }

    fn array_type(&self, node: &SyntaxNode) -> Type {
        let mut values = Vec::new();
        let mut shape = Vec::new();
        let mut all_string_keys = true;
        let mut any = false;
        for el in node.children().filter(|c| c.kind() == ARRAY_ELEMENT).take(50) {
            any = true;
            let parts: Vec<SyntaxNode> = expr_children(&el).collect();
            let has_key = el.children_with_tokens().any(|e| e.kind() == FAT_ARROW);
            match (has_key, parts.as_slice()) {
                (true, [k, v]) => {
                    let vt = self.expr_type(v);
                    let key = (k.kind() == LITERAL)
                        .then(|| k.first_token())
                        .flatten()
                        .filter(|t| t.kind() == STRING)
                        .map(|t| t.text()[1..t.text().len().saturating_sub(1)].to_string());
                    match key {
                        Some(key) => {
                            shape.push(php_hir::types::ShapeEntry { key, ty: vt.clone(), optional: false })
                        }
                        None => all_string_keys = false,
                    }
                    values.push(vt);
                }
                (false, [v]) => {
                    all_string_keys = false;
                    let is_spread = el.children_with_tokens().any(|e| e.kind() == DOT3);
                    let vt = self.expr_type(v);
                    values.push(if is_spread { vt.iterable_value().unwrap_or(Type::Mixed) } else { vt });
                }
                _ => all_string_keys = false,
            }
        }
        if !any {
            return Type::Array(None, None);
        }
        if all_string_keys && !shape.is_empty() {
            let mut deduped: Vec<php_hir::types::ShapeEntry> = Vec::new();
            for e in shape {
                deduped.retain(|d| d.key != e.key);
                deduped.push(e);
            }
            return Type::Shape(deduped);
        }
        let v = Type::union(values);
        if v.is_mixed() {
            Type::Array(None, None)
        } else {
            Type::Array(None, Some(Box::new(v)))
        }
    }

    pub(crate) fn syntax_type(&self, ty: &SyntaxNode, at: &SyntaxNode) -> Type {
        let scope = self.sema.scope_at(at.text_range().start());
        let tr = TypeResolver { scope, templates: &[], class_templates: &[] };
        let t = tr.syntax_type(ty);
        let class = self.self_class(at);
        self.resolve_rel(&t, class.as_ref(), class.as_ref())
    }

    pub(crate) fn doc_type(&self, text: &str, at: &SyntaxNode) -> Type {
        let scope = self.sema.scope_at(at.text_range().start());
        let tr = TypeResolver { scope, templates: &[], class_templates: &[] };
        let t = tr.doc_type(text);
        let class = self.self_class(at);
        self.resolve_rel(&t, class.as_ref(), class.as_ref())
    }

    fn function_return(&self, f: &FunctionDef, args: Option<&SyntaxNode>) -> Type {
        let Some(ret) = f.effective_return_type() else { return Type::Mixed };
        // simple template inference: `@template T` + `@param T $x` / `class-string<T>`
        if f.templates.is_empty() {
            return ret;
        }
        let arg_nodes: Vec<SyntaxNode> =
            args.map(|a| a.children().filter(|c| c.kind() == ARG).collect()).unwrap_or_default();
        let subst = |name: &str| -> Option<Type> {
            for (i, p) in f.params.iter().enumerate() {
                let pt = p.effective_type()?;
                let arg = arg_nodes.get(i)?.children().find(|c| is_expr(c.kind()))?;
                match &pt {
                    Type::Template(t) if &**t == name => return Some(self.expr_type(&arg)),
                    Type::ClassString(Some(inner)) if matches!(&**inner, Type::Template(t) if &**t == name) =>
                    {
                        let at = self.expr_type(&arg);
                        if let Type::ClassString(Some(c)) = at {
                            return Some(*c);
                        }
                    }
                    _ => {}
                }
            }
            None
        };
        ret.substitute(&subst)
    }

    /// Argument summary for plugins.
    pub(crate) fn arg_infos(&self, args: Option<&SyntaxNode>) -> Vec<crate::plugins::ArgInfo> {
        use crate::plugins::ArgInfo;
        let Some(args) = args else { return Vec::new() };
        args.children()
            .filter(|c| c.kind() == ARG)
            .map(|arg| {
                let Some(e) = arg.children().find(|c| is_expr(c.kind())) else { return ArgInfo::Other };
                match e.kind() {
                    CLASS_CONST_FETCH_EXPR
                        if e.children()
                            .find_map(ast::MemberName::cast)
                            .and_then(|m| m.text())
                            .is_some_and(|n| n.eq_ignore_ascii_case("class")) =>
                    {
                        match e.first_child().and_then(|l| {
                            crate::classify::static_lhs_classes(self.sema, &l).into_iter().next()
                        }) {
                            Some(c) => ArgInfo::ClassString(c),
                            None => ArgInfo::Other,
                        }
                    }
                    LITERAL => match e.first_token().filter(|t| t.kind() == STRING) {
                        Some(t) => ArgInfo::String(crate::plugins::unquote(t.text())),
                        None => ArgInfo::Other,
                    },
                    _ => ArgInfo::Other,
                }
            })
            .collect()
    }

    fn plugin_call_type(&self, callee: crate::plugins::Callee, args: Option<&SyntaxNode>) -> Option<Type> {
        let plugins = self.sema.ws.plugins();
        if plugins.is_empty() {
            return None;
        }
        let call = crate::plugins::CallInfo { callee, args: self.arg_infos(args) };
        let cx = crate::plugins::PluginCx { ws: self.sema.ws, file: self.sema.file };
        plugins.iter().find_map(|p| p.call_return_type(&cx, &call))
    }

    fn call_type(&self, node: &SyntaxNode) -> Type {
        let Some(callee) = node.first_child() else { return Type::Mixed };
        let args = node.children().find(|c| c.kind() == ARG_LIST);
        if callee.kind() == NAME_EXPR {
            let text = ast::text_without_trivia(&callee);
            let found = self.sema.resolve_function(&text, callee.text_range().start());
            let fqn = match found {
                Some((_, f)) => f.fqn.clone(),
                None => {
                    let r = self.sema.scope_at(callee.text_range().start()).resolve_function(&text);
                    r.fallback.unwrap_or(r.name)
                }
            };
            if let Some(t) = self.plugin_call_type(crate::plugins::Callee::Function(fqn), args.as_ref()) {
                return t;
            }
            return match found {
                Some((_, f)) => self.function_return(f, args.as_ref()),
                None => Type::Mixed,
            };
        }
        match self.expr_type(&callee) {
            Type::Closure(_, Some(ret)) => *ret,
            t => {
                // invokable objects
                for c in t.class_names() {
                    if let Some(m) = self.sema.index().find_method(&c, "__invoke") {
                        return self.function_return(m.def, args.as_ref());
                    }
                }
                Type::Mixed
            }
        }
    }

    fn receiver_classes(&self, node: &SyntaxNode) -> Vec<Fqn> {
        let Some(lhs) = node.first_child() else { return Vec::new() };
        if matches!(node.kind(), STATIC_CALL_EXPR | STATIC_PROPERTY_FETCH_EXPR | CLASS_CONST_FETCH_EXPR) {
            return crate::classify::static_lhs_classes(self.sema, &lhs);
        }
        self.expr_type(&lhs).class_names()
    }

    fn member_name(node: &SyntaxNode) -> Option<String> {
        node.children().find_map(ast::MemberName::cast).and_then(|m| m.text())
    }

    fn method_call_type(&self, node: &SyntaxNode) -> Type {
        let Some(name) = Self::member_name(node) else { return Type::Mixed };
        let args = node.children().find(|c| c.kind() == ARG_LIST);
        let mut out = Vec::new();
        let receiver_ty =
            node.first_child().filter(|_| node.kind() == METHOD_CALL_EXPR).map(|r| self.expr_type(&r));
        for class in self.receiver_classes(node) {
            let callee = crate::plugins::Callee::Method {
                class: class.clone(),
                name: name.clone(),
                is_static: node.kind() == STATIC_CALL_EXPR,
            };
            if let Some(t) = self.plugin_call_type(callee, args.as_ref()) {
                out.push(t);
                continue;
            }
            let Some(m) = self.sema.index().find_method(&class, &name) else { continue };
            let mut t = self.function_return(m.def, args.as_ref());
            // class level generics, also through `@extends Base<T>` chains
            let targs = self.receiver_args(receiver_ty.as_ref(), &class);
            t = self.sema.index().substitute_templates(&t, &class, &targs, &m.owner.fqn);
            out.push(self.resolve_rel(&t, Some(&m.owner.fqn), Some(&class)));
        }
        if out.is_empty() {
            Type::Mixed
        } else {
            Type::union(out)
        }
    }

    /// Generic arguments of `class` in the receiver type (`Collection<User>`).
    fn receiver_args(&self, receiver: Option<&Type>, class: &Fqn) -> Vec<Type> {
        receiver
            .and_then(|r| {
                r.members().into_iter().find_map(|t| match t {
                    Type::Named(n, args) if n.eq_ignore_case(class) => Some(args.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default()
    }

    fn property_type(&self, node: &SyntaxNode) -> Type {
        let Some(name) = Self::member_name(node) else { return Type::Mixed };
        let mut out = Vec::new();
        let receiver_ty =
            node.first_child().filter(|_| node.kind() == PROPERTY_FETCH_EXPR).map(|r| self.expr_type(&r));
        for class in self.receiver_classes(node) {
            if let Some(p) = self.sema.index().find_property(&class, &name) {
                if let Some(t) = p.def.effective_type() {
                    let targs = self.receiver_args(receiver_ty.as_ref(), &class);
                    let t = self.sema.index().substitute_templates(&t, &class, &targs, &p.owner.fqn);
                    out.push(self.resolve_rel(&t, Some(&p.owner.fqn), Some(&class)));
                    continue;
                }
            }
            let cx = crate::plugins::PluginCx { ws: self.sema.ws, file: self.sema.file };
            if let Some(t) = self.sema.ws.plugins().iter().find_map(|p| p.property_type(&cx, &class, &name)) {
                out.push(t);
                continue;
            }
            // enum `->name` / `->value`
            if let Some((_, c)) = self.sema.index().class(&class) {
                if c.kind == ast::ClassLikeKind::Enum {
                    match name.as_str() {
                        "name" => out.push(Type::String),
                        "value" => out.push(c.backing_type.clone().unwrap_or(Type::Mixed)),
                        _ => {}
                    }
                }
            }
        }
        if out.is_empty() {
            Type::Mixed
        } else {
            Type::union(out)
        }
    }

    fn class_const_type(&self, node: &SyntaxNode) -> Type {
        let Some(name) = Self::member_name(node) else { return Type::Mixed };
        let classes = self.receiver_classes(node);
        if name.eq_ignore_ascii_case("class") {
            return match classes.into_iter().next() {
                Some(c) => Type::ClassString(Some(Box::new(Type::Named(c, Vec::new())))),
                None => Type::ClassString(None),
            };
        }
        let mut out = Vec::new();
        for c in classes {
            if let Some(case) = self.sema.index().find_case(&c, &name) {
                out.push(Type::Named(case.owner.fqn.clone(), Vec::new()));
            } else if let Some(k) = self.sema.index().find_class_constant(&c, &name) {
                out.push(
                    k.def
                        .ty
                        .clone()
                        .unwrap_or_else(|| k.def.value.as_deref().map(literal_type).unwrap_or(Type::Mixed)),
                );
            }
        }
        if out.is_empty() {
            Type::Mixed
        } else {
            Type::union(out)
        }
    }

    // ------------------------------------------------------------------ variables

    /// Type of variable `$name` as seen at `at`.
    pub fn variable_type(&self, name: &str, at: &SyntaxNode) -> Type {
        let key = (name.to_string(), at.text_range().start());
        if let Some(t) = self.sema.var_cache.borrow().get(&key) {
            return t.clone();
        }
        let depth = &self.sema.infer_depth;
        if depth.get() > MAX_DEPTH {
            self.sema.infer_truncated.set(self.sema.infer_truncated.get() + 1);
            return Type::Mixed;
        }
        let truncated = self.sema.infer_truncated.get();
        depth.set(depth.get() + 1);
        let t = self.variable_type_inner(name, at);
        depth.set(depth.get() - 1);
        if self.sema.infer_truncated.get() == truncated && !self.sema.in_loop_carried.get() {
            self.sema.var_cache.borrow_mut().insert(key, t.clone());
        }
        t
    }

    fn variable_type_inner(&self, name: &str, at: &SyntaxNode) -> Type {
        let offset = at.text_range().start();
        let scope = self.sema.variable_scope(at);
        // a narrowing condition wins unless the variable is reassigned after it
        let narrowed = self.narrowed(name, at, &scope);
        let mut best: Option<(TextSize, Type)> = None;
        let consider =
            |pos: TextSize, ty: Box<dyn FnOnce() -> Type + '_>, best: &mut Option<(TextSize, Type)>| {
                if best.as_ref().is_none_or(|(p, _)| pos >= *p) {
                    *best = Some((pos, ty()));
                }
            };

        // parameters
        if let Some(pl) = scope.children().find_map(ast::ParamList::cast) {
            for p in pl.params() {
                if p.name().as_deref() == Some(name) {
                    let t = self.param_type(&p, &scope);
                    consider(scope.text_range().start(), Box::new(move || t), &mut best);
                }
            }
        }
        // arrow functions between `at` and the scope also bind parameters
        for anc in at.ancestors().take_while(|n| n != &scope) {
            if anc.kind() == ARROW_FN_EXPR {
                if let Some(pl) = anc.children().find_map(ast::ParamList::cast) {
                    for p in pl.params() {
                        if p.name().as_deref() == Some(name) {
                            let t = self.param_type(&p, &anc);
                            consider(anc.text_range().start(), Box::new(move || t), &mut best);
                        }
                    }
                }
            }
        }
        // closure `use` variables come from the outer scope
        if scope.kind() == CLOSURE_EXPR {
            if let Some(uses) = scope.children().find(|c| c.kind() == CLOSURE_USE) {
                let used = uses
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| t.kind() == VARIABLE && &t.text()[1..] == name);
                if used {
                    if let Some(parent) = scope.parent() {
                        let t = self.variable_type(name, &parent_expr_anchor(&scope, &parent));
                        consider(scope.text_range().start(), Box::new(move || t), &mut best);
                    }
                }
            }
        }

        // in templates the loop body follows `@foreach(...)` as separate statements
        let template = scope.kind() == SOURCE_FILE
            && self.sema.ws.file(self.sema.file).is_some_and(|f| crate::blade::is_blade(&f.path));
        let mut stack: Vec<SyntaxNode> = scope.children().collect();
        while let Some(n) = stack.pop() {
            if n.text_range().start() >= offset {
                continue;
            }
            match n.kind() {
                FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | CLASS_DECL | INTERFACE_DECL | TRAIT_DECL
                | ENUM_DECL | ANON_CLASS => continue,
                // inside an `if`: assignments of the other branches do not reach `at`
                IF_STMT | ELSE_IF_CLAUSE if n.text_range().contains(offset) => {
                    stack.extend(
                        n.children().filter(|c| is_expr(c.kind()) || c.text_range().contains(offset)),
                    );
                }
                ASSIGN_EXPR if n.text_range().end() <= offset => {
                    if let Some(lhs) = n.first_child() {
                        if lhs.kind() == VARIABLE_EXPR
                            && ast::VariableExpr::cast(lhs.clone()).and_then(|v| v.name()).as_deref()
                                == Some(name)
                        {
                            let pos = n.text_range().start();
                            let doc = statement_doc_var(&n, name);
                            let node = n.clone();
                            consider(
                                pos,
                                Box::new(move || match doc {
                                    Some(d) => self.doc_type(&d, &node),
                                    None => self.expr_type(&node),
                                }),
                                &mut best,
                            );
                        } else if matches!(lhs.kind(), ARRAY_EXPR | LIST_EXPR) {
                            self.destructuring(&n, &lhs, name, &mut best);
                        }
                    }
                }
                FOREACH_STMT if n.text_range().contains(offset) || template => {
                    let iter = expr_children(&n).next();
                    let bound = |child_kind: SyntaxKind, best: &mut Option<(TextSize, Type)>, key: bool| {
                        let Some(binding) = n.children().find(|c| c.kind() == child_kind) else { return };
                        let Some(var) = binding.children().find(|c| c.kind() == VARIABLE_EXPR) else {
                            return;
                        };
                        if ast::VariableExpr::cast(var).and_then(|v| v.name()).as_deref() != Some(name) {
                            return;
                        }
                        let doc = statement_doc_var(&n, name);
                        let t = match (doc, &iter) {
                            (Some(d), _) => self.doc_type(&d, &n),
                            (None, Some(it)) => {
                                let it_ty = self.expr_type(it);
                                if key {
                                    it_ty.iterable_key().unwrap_or(Type::union(vec![Type::Int, Type::String]))
                                } else {
                                    self.iteration_value(&it_ty)
                                }
                            }
                            _ => Type::Mixed,
                        };
                        if best.as_ref().is_none_or(|(p, _)| n.text_range().start() >= *p) {
                            *best = Some((n.text_range().start(), t));
                        }
                    };
                    bound(FOREACH_VALUE, &mut best, false);
                    bound(FOREACH_KEY, &mut best, true);
                    stack.extend(n.children());
                }
                CATCH_CLAUSE if n.text_range().contains(offset) => {
                    let var = n
                        .children_with_tokens()
                        .filter_map(|e| e.into_token())
                        .find(|t| t.kind() == VARIABLE);
                    if var.is_some_and(|v| &v.text()[1..] == name) {
                        let types: Vec<Type> = n
                            .children()
                            .find(|c| c.kind() == CATCH_TYPES)
                            .map(|ct| {
                                ct.children()
                                    .map(|q| {
                                        Type::Named(
                                            self.sema
                                                .scope_at(q.text_range().start())
                                                .resolve_class(&ast::text_without_trivia(&q)),
                                            Vec::new(),
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let t = Type::union(types);
                        consider(n.text_range().start(), Box::new(move || t), &mut best);
                    }
                    stack.extend(n.children());
                }
                STATIC_VAR | GLOBAL_STMT => {
                    if n.descendants_with_tokens()
                        .filter_map(|e| e.into_token())
                        .any(|t| t.kind() == VARIABLE && &t.text()[1..] == name)
                    {
                        let doc = statement_doc_var(&n, name);
                        let value = expr_children(&n).nth(1);
                        let node = n.clone();
                        consider(
                            n.text_range().start(),
                            Box::new(move || match (doc, value) {
                                (Some(d), _) => self.doc_type(&d, &node),
                                (None, Some(v)) => self.expr_type(&v),
                                _ => Type::Mixed,
                            }),
                            &mut best,
                        );
                    }
                    stack.extend(n.children());
                }
                CALL_EXPR => {
                    // assert($x instanceof Foo)
                    if let Some(t) = self.assert_instanceof(&n, name) {
                        consider(n.text_range().start(), Box::new(move || t), &mut best);
                    }
                    stack.extend(n.children());
                }
                _ => stack.extend(n.children()),
            }
        }
        // standalone `/** @var Type $name */` comments
        for tok in scope.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if tok.kind() != DOC_COMMENT || tok.text_range().start() >= offset {
                continue;
            }
            if !tok.text().contains("@var") || !tok.text().contains(&format!("${name}")) {
                continue;
            }
            let doc = DocBlock::parse(tok.text());
            if let Some(v) =
                doc.var(Some(name)).filter(|v| v.var.as_deref() == Some(name) && !v.type_text.is_empty())
            {
                let pos = tok.text_range().start();
                if best.as_ref().is_none_or(|(p, _)| pos >= *p) {
                    best = Some((pos, self.doc_type(&v.type_text, at)));
                }
            }
        }
        if let Some((t, anchor)) = narrowed {
            if best.as_ref().is_none_or(|(p, _)| *p <= anchor) {
                return t;
            }
        }
        // inside a loop, assignments later in the loop reach the next iteration
        if let Some((pos, t)) = best.take() {
            let carried = self.loop_carried(name, at, &scope);
            best = Some((
                pos,
                if carried.is_empty() { t } else { Type::union(std::iter::once(t).chain(carried).collect()) },
            ));
        }
        if best.is_none() && scope.kind() == SOURCE_FILE {
            if let Some(t) = self.template_variable(name) {
                return t;
            }
        }
        best.map(|(_, t)| t).unwrap_or(if is_superglobal(name) {
            Type::Array(None, None)
        } else {
            Type::Mixed
        })
    }

    /// Variables of a template passed in by its caller (see plugins).
    fn template_variable(&self, name: &str) -> Option<Type> {
        let file = self.sema.ws.file(self.sema.file)?;
        if !crate::blade::is_blade(&file.path) {
            return None;
        }
        let cx = crate::plugins::PluginCx { ws: self.sema.ws, file: self.sema.file };
        self.sema.ws.plugins().iter().find_map(|p| p.template_variable_type(&cx, name))
    }

    fn iteration_value(&self, t: &Type) -> Type {
        if let Some(v) = t.iterable_value() {
            return v;
        }
        // Traversable objects: generics `Collection<K, V>` or getIterator()/current()
        let mut out = Vec::new();
        for m in t.members() {
            if let Type::Named(c, args) = m {
                // `Traversable<K, V>` reached through @extends / @implements
                let index = self.sema.index();
                let traversable = ["Traversable", "IteratorAggregate", "Iterator"]
                    .iter()
                    .find_map(|t| index.ancestor_args(c, args, &Fqn::new(t)).filter(|a| !a.is_empty()));
                if let Some(v) =
                    traversable.and_then(|a| a.last().cloned()).filter(|v| !matches!(v, Type::Template(_)))
                {
                    out.push(v);
                    continue;
                }
                if let Some(last) = args.last() {
                    out.push(last.clone());
                    continue;
                }
                for method in ["current", "getIterator"] {
                    if let Some(found) = self.sema.index().find_method(c, method) {
                        if let Some(rt) = found.def.effective_return_type() {
                            let rt = index.substitute_templates(&rt, c, args, &found.owner.fqn);
                            let rt = self.resolve_rel(&rt, Some(&found.owner.fqn), Some(c));
                            if method == "current" {
                                out.push(rt);
                            } else if let Some(v) = rt.iterable_value() {
                                out.push(v);
                            } else if let Type::Named(_, args) = &rt {
                                if let Some(l) = args.last() {
                                    out.push(l.clone());
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            Type::Mixed
        } else {
            Type::union(out)
        }
    }

    fn destructuring(
        &self,
        assign: &SyntaxNode,
        pattern: &SyntaxNode,
        name: &str,
        best: &mut Option<(TextSize, Type)>,
    ) {
        let rhs = expr_children(assign).nth(1).map(|r| self.expr_type(&r)).unwrap_or(Type::Mixed);
        for (i, el) in pattern.children().filter(|c| c.kind() == ARRAY_ELEMENT).enumerate() {
            let parts: Vec<SyntaxNode> = expr_children(&el).collect();
            let (key, target) = match parts.as_slice() {
                [k, v] => {
                    (k.first_token().map(|t| t.text().trim_matches(['\'', '"']).to_string()), v.clone())
                }
                [v] => (Some(i.to_string()), v.clone()),
                _ => continue,
            };
            if target.kind() == VARIABLE_EXPR
                && ast::VariableExpr::cast(target).and_then(|v| v.name()).as_deref() == Some(name)
            {
                let t = rhs.index(key.as_deref()).unwrap_or(Type::Mixed);
                let pos = assign.text_range().start();
                if best.as_ref().is_none_or(|(p, _)| pos >= *p) {
                    *best = Some((pos, t));
                }
            }
        }
    }

    fn param_type(&self, p: &ast::Param, func: &SyntaxNode) -> Type {
        let name = p.name().unwrap_or_default();
        let doc = php_syntax::ast::doc_comment(func)
            .or_else(|| func.parent().filter(|pp| pp.kind() == ARG).and_then(|_| None))
            .map(|d| DocBlock::parse(d.text()));
        let declared = p.ty().map(|t| self.syntax_type(&t, func));
        let doc_ty = doc
            .as_ref()
            .and_then(|d| d.param(&name))
            .filter(|t| !t.type_text.is_empty())
            .map(|t| self.doc_type(&t.type_text, func));
        // no own `@param`: inherit the documented type of the overridden method
        let doc_ty = doc_ty.or_else(|| {
            let vague = declared.as_ref().is_none_or(|d| {
                matches!(d, Type::Array(None, None) | Type::Iterable(None, None) | Type::Mixed)
            });
            if vague {
                self.inherited_param_doc(func, &name)
            } else {
                None
            }
        });
        let t = match (declared, doc_ty) {
            (_, Some(d)) if !d.is_mixed() => d,
            (Some(d), _) => d,
            _ => Type::Mixed,
        };
        if p.is_variadic() {
            Type::List(Some(Box::new(t)))
        } else {
            t
        }
    }

    /// Types assigned to `$name` after `at` inside loops enclosing `at`.
    fn loop_carried(&self, name: &str, at: &SyntaxNode, scope: &SyntaxNode) -> Vec<Type> {
        let flag = &self.sema.in_loop_carried;
        if flag.get() {
            return Vec::new();
        }
        flag.set(true);
        let out = self.loop_carried_inner(name, at, scope);
        flag.set(false);
        out
    }

    fn loop_carried_inner(&self, name: &str, at: &SyntaxNode, scope: &SyntaxNode) -> Vec<Type> {
        let offset = at.text_range().start();
        let mut out = Vec::new();
        for lp in at.ancestors().take_while(|a| a != scope) {
            if !matches!(lp.kind(), FOREACH_STMT | WHILE_STMT | FOR_STMT | DO_WHILE_STMT) {
                continue;
            }
            for assign in
                lp.descendants().filter(|n| n.kind() == ASSIGN_EXPR && n.text_range().start() > offset)
            {
                // not inside nested functions
                if assign
                    .ancestors()
                    .take_while(|a| a != &lp)
                    .any(|a| matches!(a.kind(), CLOSURE_EXPR | ARROW_FN_EXPR | FUNCTION_DECL))
                {
                    continue;
                }
                let Some(lhs) = assign.first_child() else { continue };
                let is_eq = assign.children_with_tokens().any(|e| e.kind() == EQ);
                if is_eq
                    && lhs.kind() == VARIABLE_EXPR
                    && ast::VariableExpr::cast(lhs).and_then(|v| v.name()).as_deref() == Some(name)
                {
                    if let Some(rhs) = expr_children(&assign).nth(1) {
                        out.push(self.expr_type(&rhs));
                    }
                }
            }
        }
        out
    }

    /// `if ($a->b instanceof C) { ...$a->b... }` (also after `&&` and in `?:`).
    fn narrowed_expr(&self, node: &SyntaxNode) -> Option<Type> {
        let text = ast::text_without_trivia(node);
        let scope = self.sema.variable_scope(node);
        for anc in node.ancestors() {
            if anc == scope {
                break;
            }
            let cond = match anc.kind() {
                IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT => {
                    let c = expr_children(&anc).next()?;
                    let in_else = anc
                        .children()
                        .filter(|c| matches!(c.kind(), ELSE_CLAUSE | ELSE_IF_CLAUSE))
                        .any(|c| c.text_range().contains_range(node.text_range()));
                    (!c.text_range().contains_range(node.text_range()) && !in_else).then_some(c)
                }
                BIN_EXPR if anc.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW)) => {
                    expr_children(&anc).next().filter(|l| !l.text_range().contains_range(node.text_range()))
                }
                TERNARY_EXPR => {
                    let parts: Vec<SyntaxNode> = expr_children(&anc).collect();
                    (parts.len() == 3 && parts[1].text_range().contains_range(node.text_range()))
                        .then(|| parts[0].clone())
                }
                _ => None,
            };
            if let Some(t) = cond.and_then(|c| self.instanceof_expr(&c, &text)) {
                return Some(t);
            }
        }
        None
    }

    fn instanceof_expr(&self, cond: &SyntaxNode, text: &str) -> Option<Type> {
        match cond.kind() {
            INSTANCEOF_EXPR => {
                let mut parts = expr_children(cond);
                let lhs = parts.next()?;
                let rhs = parts.next()?;
                if ast::text_without_trivia(&lhs) != text || rhs.kind() != NAME_EXPR {
                    return None;
                }
                let name = ast::text_without_trivia(&rhs);
                self.sema.resolve_class_name(&name, &rhs).map(|c| Type::Named(c, Vec::new()))
            }
            PAREN_EXPR => self.instanceof_expr(&expr_children(cond).next()?, text),
            BIN_EXPR if cond.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW)) => {
                expr_children(cond).find_map(|c| self.instanceof_expr(&c, text))
            }
            _ => None,
        }
    }

    /// `@param` type of parameter `name` in a method overridden by `func`.
    fn inherited_param_doc(&self, func: &SyntaxNode, name: &str) -> Option<Type> {
        if func.kind() != METHOD_DECL {
            return None;
        }
        let class = func
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL))?;
        let fqn = self.sema.class_fqn(&class)?;
        let method = func
            .children()
            .find_map(ast::MemberName::cast)
            .and_then(|m| m.text())
            .or_else(|| func.children().find_map(ast::Name::cast).map(|n| n.text()))?;
        self.sema
            .index()
            .ancestors(&fqn)
            .into_iter()
            .filter(|(_, c)| c.fqn != fqn)
            .find_map(|(_, c)| c.method(&method)?.params.iter().find(|p| p.name == name)?.doc_ty.clone())
    }

    /// `if ($x instanceof Foo) { ... $x ... }`
    fn narrowed(&self, name: &str, at: &SyntaxNode, scope: &SyntaxNode) -> Option<(Type, TextSize)> {
        for anc in at.ancestors() {
            if &anc == scope {
                break;
            }
            // `$x instanceof A && $x->...`
            if anc.kind() == BIN_EXPR && anc.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW))
            {
                let lhs = expr_children(&anc).next()?;
                if !lhs.text_range().contains_range(at.text_range()) {
                    if let Some(t) = self.instanceof_condition(&lhs, name) {
                        return Some((t, anc.text_range().start()));
                    }
                }
                continue;
            }
            // `!$x instanceof A || $x->...`
            if anc.kind() == BIN_EXPR && anc.children_with_tokens().any(|e| matches!(e.kind(), PIPE2 | OR_KW))
            {
                let lhs = expr_children(&anc).next()?;
                if !lhs.text_range().contains_range(at.text_range()) {
                    if let Some(inner) = negated(&lhs) {
                        if let Some(t) = self.instanceof_condition(&inner, name) {
                            return Some((t, anc.text_range().start()));
                        }
                    }
                }
                continue;
            }
            // `$x instanceof A ? $x->... : ...`
            if anc.kind() == TERNARY_EXPR {
                let parts: Vec<SyntaxNode> = expr_children(&anc).collect();
                if parts.len() == 3 && parts[1].text_range().contains_range(at.text_range()) {
                    if let Some(t) = self.instanceof_condition(&parts[0], name) {
                        return Some((t, anc.text_range().start()));
                    }
                }
                continue;
            }
            if !matches!(anc.kind(), IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT) {
                continue;
            }
            let cond = expr_children(&anc).next()?;
            if cond.text_range().contains_range(at.text_range()) {
                continue;
            }
            // only the "then" branch: not inside else / elseif clauses of this if
            let in_else = anc
                .children()
                .filter(|c| matches!(c.kind(), ELSE_CLAUSE | ELSE_IF_CLAUSE))
                .any(|c| c.text_range().contains_range(at.text_range()));
            if in_else {
                continue;
            }
            if let Some(t) = self.instanceof_condition(&cond, name) {
                return Some((t, anc.text_range().start()));
            }
        }
        None
    }

    fn instanceof_condition(&self, cond: &SyntaxNode, name: &str) -> Option<Type> {
        match cond.kind() {
            INSTANCEOF_EXPR => {
                let mut parts = expr_children(cond);
                let lhs = parts.next()?;
                let rhs = parts.next()?;
                if lhs.kind() != VARIABLE_EXPR
                    || ast::VariableExpr::cast(lhs).and_then(|v| v.name()).as_deref() != Some(name)
                {
                    return None;
                }
                if rhs.kind() == NAME_EXPR {
                    let text = ast::text_without_trivia(&rhs);
                    return self.sema.resolve_class_name(&text, &rhs).map(|c| Type::Named(c, Vec::new()));
                }
                None
            }
            PAREN_EXPR => self.instanceof_condition(&expr_children(cond).next()?, name),
            // `is_array($x)`, `is_string($x)`, ...
            CALL_EXPR => {
                let callee = cond.first_child().filter(|c| c.kind() == NAME_EXPR)?;
                let f = ast::text_without_trivia(&callee).trim_start_matches('\\').to_ascii_lowercase();
                let arg =
                    cond.children().find(|c| c.kind() == ARG_LIST)?.children().find(|c| c.kind() == ARG)?;
                let e = expr_children(&arg).next()?;
                if e.kind() != VARIABLE_EXPR
                    || ast::VariableExpr::cast(e).and_then(|v| v.name()).as_deref() != Some(name)
                {
                    return None;
                }
                Some(match f.as_str() {
                    "is_array" => Type::Array(None, None),
                    "is_string" => Type::String,
                    "is_int" | "is_integer" | "is_long" => Type::Int,
                    "is_float" | "is_double" => Type::Float,
                    "is_bool" => Type::Bool,
                    "is_numeric" => Type::union(vec![Type::Int, Type::Float, Type::String]),
                    _ => return None,
                })
            }
            BIN_EXPR => {
                // `$x !== null`, `null !== $x`, `$x != null`
                if cond.children_with_tokens().any(|e| matches!(e.kind(), NEQ | NEQ2)) {
                    let parts: Vec<SyntaxNode> = expr_children(cond).collect();
                    let is_null = |n: &SyntaxNode| ast::text_without_trivia(n).eq_ignore_ascii_case("null");
                    let var = match parts.as_slice() {
                        [a, b] if is_null(b) => a,
                        [a, b] if is_null(a) => b,
                        _ => return None,
                    };
                    if var.kind() != VARIABLE_EXPR
                        || ast::VariableExpr::cast(var.clone()).and_then(|v| v.name()).as_deref()
                            != Some(name)
                    {
                        return None;
                    }
                    let t = self.variable_type(name, cond).without_null();
                    return (!t.is_mixed()).then_some(t);
                }
                let is_and = cond.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW));
                if !is_and {
                    return None;
                }
                expr_children(cond).find_map(|c| self.instanceof_condition(&c, name))
            }
            _ => None,
        }
    }

    fn assert_instanceof(&self, call: &SyntaxNode, name: &str) -> Option<Type> {
        let callee = call.first_child()?;
        if callee.kind() != NAME_EXPR
            || !ast::text_without_trivia(&callee).trim_start_matches('\\').eq_ignore_ascii_case("assert")
        {
            return None;
        }
        let arg = call.children().find(|c| c.kind() == ARG_LIST)?.children().find(|c| c.kind() == ARG)?;
        let e = expr_children(&arg).next()?;
        self.instanceof_condition(&e, name)
    }
}

fn is_first_class_callable(call: &SyntaxNode) -> bool {
    call.children().find(|c| c.kind() == ARG_LIST).is_some_and(|l| {
        let args: Vec<SyntaxNode> = l.children().filter(|c| c.kind() == ARG).collect();
        args.len() == 1
            && args[0].children_with_tokens().any(|e| e.kind() == DOT3)
            && !args[0].children().any(|c| is_expr(c.kind()))
    }) || call.children().find(|c| c.kind() == ARG_LIST).is_some_and(|l| {
        // the parser may keep `...` as a bare token of the argument list
        let toks: Vec<_> =
            l.children_with_tokens().filter(|e| !e.kind().is_trivia()).map(|e| e.kind()).collect();
        toks == [L_PAREN, DOT3, R_PAREN]
    })
}

/// `!expr` -> `expr` (also `!(expr)`).
fn negated(node: &SyntaxNode) -> Option<SyntaxNode> {
    if node.kind() != PREFIX_EXPR || !node.children_with_tokens().any(|e| e.kind() == BANG) {
        return None;
    }
    let inner = expr_children(node).next()?;
    if inner.kind() == PAREN_EXPR {
        return expr_children(&inner).next();
    }
    Some(inner)
}

/// A node in the outer scope positioned at the closure (for looking up
/// `use` variables).
fn parent_expr_anchor(closure: &SyntaxNode, _parent: &SyntaxNode) -> SyntaxNode {
    // the closure node itself lives in the outer scope; looking up from its
    // parent finds assignments before the closure
    closure.parent().unwrap_or_else(|| closure.clone())
}

/// `/** @var Type $name */` (or without name) directly before the statement
/// containing `node`.
fn statement_doc_var(node: &SyntaxNode, name: &str) -> Option<String> {
    let stmt = node.ancestors().find(|n| {
        matches!(n.kind(), EXPR_STMT | FOREACH_STMT | STATIC_STMT | GLOBAL_STMT | RETURN_STMT | ECHO_STMT)
    })?;
    let doc = ast::doc_comment(&stmt)?;
    let doc = DocBlock::parse(doc.text());
    let v = doc.var(Some(name))?;
    if v.var.is_none() || v.var.as_deref() == Some(name) {
        Some(v.type_text).filter(|t| !t.is_empty())
    } else {
        None
    }
}

pub(crate) fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "_GET" | "_POST" | "_SERVER" | "_REQUEST" | "_COOKIE" | "_FILES" | "_ENV" | "_SESSION" | "GLOBALS"
    )
}
