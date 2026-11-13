//! Refactorings offered as code actions: extract variable / method /
//! constant, inline variable, constructor promotion, generate constructor,
//! add return type and PSR-4 namespace fixes.

use std::collections::BTreeSet;

use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind, SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::code_actions::{edit, indent_unit, line_indent, CodeAction, CodeActionKind};
use crate::completion::TextEdit;
use crate::infer::is_expr;
use crate::rename::SourceChange;
use crate::sema::Sema;

pub(crate) fn refactorings(sema: &Sema, range: TextRange, out: &mut Vec<CodeAction>) {
    if !range.is_empty() {
        extract_variable(sema, range, out);
        extract_method(sema, range, out);
        extract_constant(sema, range, out);
    }
    let Some(token) = sema.token_at(range.start()) else { return };
    let Some(node) = token.parent() else { return };
    inline_variable(sema, &node, out);
    promote_constructor(sema, &node, out);
    generate_constructor(sema, &node, out);
    add_return_type(sema, &node, out);
    fix_namespace(sema, &node, out);
}

fn is_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        EXPR_STMT
            | ECHO_STMT
            | RETURN_STMT
            | IF_STMT
            | WHILE_STMT
            | DO_WHILE_STMT
            | FOR_STMT
            | FOREACH_STMT
            | SWITCH_STMT
            | TRY_STMT
            | THROW_STMT
            | UNSET_STMT
            | GLOBAL_STMT
            | STATIC_STMT
            | BLOCK
            | BREAK_STMT
            | CONTINUE_STMT
    )
}

/// The expression node exactly covering `range` (ignoring surrounding whitespace).
fn expr_at(sema: &Sema, range: TextRange) -> Option<SyntaxNode> {
    let text = &sema.text[range];
    let trimmed_start = range.start() + TextSize::from((text.len() - text.trim_start().len()) as u32);
    let trimmed_end = range.end() - TextSize::from((text.len() - text.trim_end().len()) as u32);
    if trimmed_start >= trimmed_end {
        return None;
    }
    let r = TextRange::new(trimmed_start, trimmed_end);
    let element = sema.root.covering_element(r);
    let node = match element {
        php_syntax::NodeOrToken::Node(n) => n,
        php_syntax::NodeOrToken::Token(t) => t.parent()?,
    };
    node.ancestors().find(|n| is_expr(n.kind()) && n.text_range() == r)
}

/// Suggests a variable name for an expression.
fn suggest_name(expr: &SyntaxNode) -> String {
    let name = match expr.kind() {
        METHOD_CALL_EXPR | STATIC_CALL_EXPR | PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR => {
            expr.children().find_map(ast::MemberName::cast).and_then(|m| m.text())
        }
        CALL_EXPR => {
            expr.first_child().filter(|c| c.kind() == NAME_EXPR).map(|c| ast::text_without_trivia(&c))
        }
        NEW_EXPR => expr.children().find(|c| c.kind() == NAME_EXPR).map(|c| ast::text_without_trivia(&c)),
        _ => None,
    };
    let Some(name) = name else { return "value".into() };
    let name = name.rsplit('\\').next().unwrap_or(&name).to_string();
    let stripped = ["get", "find", "fetch", "load", "create", "make", "is", "has"]
        .iter()
        .find_map(|p| {
            name.strip_prefix(p).filter(|r| r.chars().next().is_some_and(|c| c.is_uppercase() || c == '_'))
        })
        .unwrap_or(&name);
    let mut chars = stripped.trim_start_matches('_').chars();
    let first = chars.next().map(|c| c.to_lowercase().collect::<String>()).unwrap_or_default();
    let n = format!("{first}{}", chars.as_str());
    if n.is_empty() || n.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        "value".into()
    } else {
        n
    }
}

fn unique_var(scope: &SyntaxNode, base: &str) -> String {
    let used: BTreeSet<String> = scope
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == VARIABLE)
        .map(|t| t.text()[1..].to_string())
        .collect();
    if !used.contains(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}{i}")).find(|n| !used.contains(n)).unwrap()
}

fn extract_variable(sema: &Sema, range: TextRange, out: &mut Vec<CodeAction>) {
    let Some(expr) = expr_at(sema, range) else { return };
    // assignments targets and simple variables are not worth extracting
    if matches!(expr.kind(), VARIABLE_EXPR | ASSIGN_EXPR | CLOSURE_EXPR | ARROW_FN_EXPR) {
        return;
    }
    if expr.parent().is_some_and(|p| p.kind() == ASSIGN_EXPR && p.first_child().as_ref() == Some(&expr)) {
        return;
    }
    let Some(stmt) = expr.ancestors().find(|n| is_statement(n.kind()) && n.kind() != BLOCK) else { return };
    // inside conditions of loops the value would change per iteration
    if matches!(stmt.kind(), WHILE_STMT | DO_WHILE_STMT | FOR_STMT)
        && !stmt.children().any(|c| c.kind() == BLOCK && c.text_range().contains_range(expr.text_range()))
    {
        return;
    }
    let scope = sema.variable_scope(&expr);
    let name = unique_var(&scope, &suggest_name(&expr));
    let indent = line_indent(sema, stmt.text_range().start());
    let mut change = SourceChange::default();
    change.push(
        sema.file,
        TextEdit {
            range: TextRange::empty(stmt.text_range().start()),
            new_text: format!("${name} = {};\n{indent}", expr.text()),
        },
    );
    change.push(sema.file, TextEdit { range: expr.text_range(), new_text: format!("${name}") });
    out.push(CodeAction {
        title: "Extract variable".into(),
        kind: CodeActionKind::RefactorExtract,
        change,
        is_preferred: false,
        fixes: None,
    });
}

fn extract_constant(sema: &Sema, range: TextRange, out: &mut Vec<CodeAction>) {
    let Some(expr) = expr_at(sema, range) else { return };
    if expr.kind() != LITERAL {
        return;
    }
    let Some(class) = sema.enclosing_class_node(&expr).filter(|c| c.kind() != ANON_CLASS) else { return };
    let Some(members) = class.children().find(|c| c.kind() == MEMBER_LIST) else { return };
    let Some(open) = members.first_token().filter(|t| t.kind() == L_CURLY) else { return };
    let literal = expr.text().to_string();
    let base: String = literal
        .trim_matches(['\'', '"'])
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let base = if base.is_empty() || base.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("VALUE_{base}")
    } else {
        base
    };
    let base: String = base.chars().take(40).collect();
    let unit = indent_unit(sema);
    let indent = format!("{}{}", line_indent(sema, class.text_range().start()), unit);
    let mut change = SourceChange::default();
    change.push(
        sema.file,
        TextEdit {
            range: TextRange::empty(open.text_range().end()),
            new_text: format!("\n{indent}private const {base} = {literal};\n"),
        },
    );
    change.push(sema.file, TextEdit { range: expr.text_range(), new_text: format!("self::{base}") });
    out.push(CodeAction {
        title: "Extract constant".into(),
        kind: CodeActionKind::RefactorExtract,
        change,
        is_preferred: false,
        fixes: None,
    });
}

/// Variables read / written in `nodes` (excluding nested closures' own vars).
fn variables(nodes: &[SyntaxNode]) -> (BTreeSet<String>, BTreeSet<String>, bool) {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut uses_this = false;
    for n in nodes {
        for t in n.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if t.kind() != VARIABLE {
                continue;
            }
            let name = t.text()[1..].to_string();
            if name == "this" {
                uses_this = true;
                continue;
            }
            let Some(p) = t.parent() else { continue };
            if p.ancestors().any(|a| {
                a.kind() == CLOSURE_EXPR
                    && n.text_range().contains_range(a.text_range())
                    && !a
                        .children()
                        .any(|c| c.kind() == CLOSURE_USE && c.text_range().contains_range(t.text_range()))
            }) {
                continue;
            }
            let is_write = p.kind() == VARIABLE_EXPR
                && p.parent()
                    .is_some_and(|a| a.kind() == ASSIGN_EXPR && a.first_child().as_ref() == Some(&p))
                || matches!(p.parent().map(|x| x.kind()), Some(FOREACH_VALUE | FOREACH_KEY))
                || p.kind() == CATCH_CLAUSE;
            if is_write {
                writes.insert(name);
            } else {
                reads.insert(name);
            }
        }
    }
    (reads, writes, uses_this)
}

fn extract_method(sema: &Sema, range: TextRange, out: &mut Vec<CodeAction>) {
    // whole statements of one block
    let Some(func) = sema
        .root
        .covering_element(range)
        .into_node()
        .or_else(|| sema.root.covering_element(range).into_token().and_then(|t| t.parent()))
        .and_then(|n| n.ancestors().find(|a| matches!(a.kind(), METHOD_DECL | FUNCTION_DECL)))
    else {
        return;
    };
    let Some(block) = sema
        .root
        .covering_element(range)
        .into_node()
        .and_then(|n| n.ancestors().find(|a| matches!(a.kind(), BLOCK | STMT_LIST | CASE_CLAUSE)))
        .or_else(|| {
            sema.root
                .covering_element(range)
                .into_token()
                .and_then(|t| t.parent_ancestors().find(|a| a.kind() == BLOCK))
        })
    else {
        return;
    };
    let stmts: Vec<SyntaxNode> =
        block.children().filter(|s| is_statement(s.kind()) && range.contains_range(s.text_range())).collect();
    if stmts.is_empty() {
        return;
    }
    // the selection must not cut statements
    let covered = TextRange::new(stmts[0].text_range().start(), stmts[stmts.len() - 1].text_range().end());
    let sel = &sema.text[range];
    let inner = &sema.text[covered];
    if sel.trim() != inner.trim() {
        return;
    }
    // control flow leaving the selection cannot be extracted safely
    if stmts.iter().any(|s| {
        s.descendants().any(|d| {
            matches!(d.kind(), RETURN_STMT | BREAK_STMT | CONTINUE_STMT | YIELD_EXPR | GOTO_STMT)
                && !d.ancestors().any(|a| a.kind() == CLOSURE_EXPR && covered.contains_range(a.text_range()))
        })
    }) {
        return;
    }
    let (reads, writes, uses_this) = variables(&stmts);
    // parameters: read before being written in the selection and defined earlier
    let body = func.children().find(|c| c.kind() == BLOCK);
    let before: Vec<SyntaxNode> = func
        .descendants()
        .filter(|n| {
            n.text_range().end() <= covered.start()
                && matches!(
                    n.kind(),
                    PARAM | ASSIGN_EXPR | FOREACH_STMT | CATCH_CLAUSE | STATIC_VAR | GLOBAL_STMT
                )
        })
        .collect();
    let (_, defined_before_w, _) = variables(&before);
    let params_before: BTreeSet<String> = func
        .children()
        .find_map(ast::ParamList::cast)
        .map(|pl| pl.params().filter_map(|p| p.name()).collect())
        .unwrap_or_default();
    let defined_before: BTreeSet<String> = defined_before_w.union(&params_before).cloned().collect();
    let params: Vec<String> = reads.iter().filter(|r| defined_before.contains(*r)).cloned().collect();
    // results: written in the selection and used afterwards
    let after: Vec<SyntaxNode> = body
        .iter()
        .flat_map(|b| b.descendants())
        .filter(|n| n.text_range().start() >= covered.end() && n.kind() == VARIABLE_EXPR)
        .collect();
    let used_after: BTreeSet<String> =
        after.iter().filter_map(|n| ast::VariableExpr::cast(n.clone()).and_then(|v| v.name())).collect();
    let results: Vec<&String> = writes.iter().filter(|w| used_after.contains(*w)).collect();
    if results.len() > 1 {
        return; // would need to return several values
    }
    let is_method = func.kind() == METHOD_DECL;
    let is_static = is_method
        && func
            .children()
            .any(|c| c.kind() == MODIFIER_LIST && c.children_with_tokens().any(|t| t.kind() == STATIC_KW));
    if uses_this && (!is_method || is_static) {
        return;
    }
    let name = {
        let existing: BTreeSet<String> = sema
            .root
            .descendants()
            .filter(|n| matches!(n.kind(), NAME | MEMBER_NAME))
            .map(|n| ast::text_without_trivia(&n).to_ascii_lowercase())
            .collect();
        let mut n = "extracted".to_string();
        let mut i = 2;
        while existing.contains(&n) {
            n = format!("extracted{i}");
            i += 1;
        }
        n
    };
    let unit = indent_unit(sema);
    let decl_indent = line_indent(sema, func.text_range().start());
    let body_indent = format!("{decl_indent}{unit}");
    let stmt_indent = line_indent(sema, covered.start());
    // re-indent the extracted statements
    let mut body_text = String::new();
    for line in sema.text[covered].lines() {
        let l = line.strip_prefix(stmt_indent.as_str()).unwrap_or(line.trim_start());
        if l.is_empty() {
            body_text.push('\n');
        } else {
            body_text.push_str(&format!("{body_indent}{l}\n"));
        }
    }
    let ret = results.first().map(|r| format!("{body_indent}return ${r};\n")).unwrap_or_default();
    let param_list: Vec<String> = params.iter().map(|p| format!("${p}")).collect();
    let (call_prefix, decl_prefix) = if is_method {
        if is_static {
            ("self::".to_string(), "private static function".to_string())
        } else {
            ("$this->".to_string(), "private function".to_string())
        }
    } else {
        (String::new(), "function".to_string())
    };
    let new_fn = format!(
        "\n\n{decl_indent}{decl_prefix} {name}({})\n{decl_indent}{{\n{body_text}{ret}{decl_indent}}}",
        param_list.join(", ")
    );
    let call = format!("{call_prefix}{name}({})", param_list.join(", "));
    let call_stmt = match results.first() {
        Some(r) => format!("${r} = {call};"),
        None => format!("{call};"),
    };
    let mut change = SourceChange::default();
    change.push(sema.file, TextEdit { range: covered, new_text: call_stmt });
    change.push(sema.file, TextEdit { range: TextRange::empty(func.text_range().end()), new_text: new_fn });
    out.push(CodeAction {
        title: if is_method { "Extract method".into() } else { "Extract function".into() },
        kind: CodeActionKind::RefactorExtract,
        change,
        is_preferred: false,
        fixes: None,
    });
}

fn inline_variable(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(var) = node.ancestors().find(|n| n.kind() == VARIABLE_EXPR) else { return };
    let Some(assign) =
        var.parent().filter(|p| p.kind() == ASSIGN_EXPR && p.first_child().as_ref() == Some(&var))
    else {
        return;
    };
    if !assign.children_with_tokens().any(|e| e.kind() == EQ)
        || assign.children_with_tokens().any(|e| e.kind() == AMP)
    {
        return;
    }
    let Some(stmt) = assign.parent().filter(|p| p.kind() == EXPR_STMT) else { return };
    let Some(name) = ast::VariableExpr::cast(var.clone()).and_then(|v| v.name()) else { return };
    let Some(value) = assign.children().filter(|c| is_expr(c.kind())).nth(1) else { return };
    let scope = sema.variable_scope(&var);
    let occurrences: Vec<SyntaxNode> = scope
        .descendants()
        .filter(|n| {
            n.kind() == VARIABLE_EXPR
                && n != &var
                && ast::VariableExpr::cast(n.clone()).and_then(|v| v.name()).as_deref() == Some(&name)
        })
        .collect();
    // only a single assignment, all uses afterwards and none written
    let writes = occurrences.iter().any(|o| {
        o.parent().is_some_and(|p| p.kind() == ASSIGN_EXPR && p.first_child().as_ref() == Some(o))
            || o.text_range().start() < stmt.text_range().start()
    });
    if writes || occurrences.is_empty() {
        return;
    }
    let simple = matches!(
        value.kind(),
        LITERAL
            | VARIABLE_EXPR
            | NAME_EXPR
            | CLASS_CONST_FETCH_EXPR
            | PROPERTY_FETCH_EXPR
            | ARRAY_EXPR
            | INTERPOLATED_STRING
    );
    let text = if simple
        || occurrences.iter().all(|o| {
            o.parent().is_some_and(|p| {
                matches!(p.kind(), ARG | ARRAY_ELEMENT | ASSIGN_EXPR | RETURN_STMT | ECHO_STMT)
            })
        }) {
        value.text().to_string()
    } else {
        format!("({})", value.text())
    };
    let mut change = SourceChange::default();
    let stmt_range = stmt.text_range();
    let end = usize::from(stmt_range.end());
    let rest = &sema.text[end..];
    let trailing = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let trailing = if rest[trailing..].starts_with('\n') { trailing + 1 } else { 0 };
    let line_start = sema.text[..usize::from(stmt_range.start())].rfind('\n').map_or(0, |i| i + 1);
    let only_ws_before = sema.text[line_start..usize::from(stmt_range.start())].trim().is_empty();
    let start =
        if only_ws_before && trailing > 0 { TextSize::from(line_start as u32) } else { stmt_range.start() };
    change.push(
        sema.file,
        TextEdit {
            range: TextRange::new(start, TextSize::from((end + trailing) as u32)),
            new_text: String::new(),
        },
    );
    for o in &occurrences {
        change.push(sema.file, TextEdit { range: o.text_range(), new_text: text.clone() });
    }
    out.push(CodeAction {
        title: format!("Inline variable ${name}"),
        kind: CodeActionKind::RefactorInline,
        change,
        is_preferred: false,
        fixes: None,
    });
}

fn constructor_of(class: &SyntaxNode) -> Option<SyntaxNode> {
    class.children().find(|c| c.kind() == MEMBER_LIST)?.children().find(|m| {
        m.kind() == METHOD_DECL
            && m.children().any(|c| {
                c.kind() == MEMBER_NAME && ast::text_without_trivia(&c).eq_ignore_ascii_case("__construct")
            })
    })
}

fn promote_constructor(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    if sema.ws.config().php_version < php_syntax::PhpVersion::PHP_8_0 {
        return;
    }
    let Some(class) = node.ancestors().find(|n| n.kind() == CLASS_DECL) else { return };
    let Some(ctor) = constructor_of(&class) else { return };
    if !ctor.text_range().contains_range(node.text_range()) {
        return;
    }
    let Some(params) = ctor.children().find_map(ast::ParamList::cast) else { return };
    let Some(body) = ctor.children().find(|c| c.kind() == BLOCK) else { return };
    let members = class.children().find(|c| c.kind() == MEMBER_LIST).unwrap();
    let mut change = SourceChange::default();
    let mut count = 0;
    for p in params.params() {
        if p.modifiers().is_some() || p.is_variadic() {
            continue;
        }
        let Some(pname) = p.name() else { continue };
        // `$this->name = $name;`
        let Some(assign_stmt) = body.children().find(|s| {
            s.kind() == EXPR_STMT
                && s.first_child().is_some_and(|a| {
                    a.kind() == ASSIGN_EXPR && {
                        let parts: Vec<SyntaxNode> = a.children().collect();
                        parts.len() == 2
                            && parts[0].kind() == PROPERTY_FETCH_EXPR
                            && ast::text_without_trivia(&parts[0]).starts_with("$this->")
                            && ast::text_without_trivia(&parts[1]) == format!("${pname}")
                    }
                })
        }) else {
            continue;
        };
        let prop_name = assign_stmt
            .first_child()
            .and_then(|a| a.first_child())
            .and_then(|f| f.children().find_map(ast::MemberName::cast))
            .and_then(|m| m.text());
        let Some(prop_name) = prop_name.filter(|n| *n == pname) else { continue };
        // the property declaration (single element)
        let Some(prop_decl) = members.children().filter_map(ast::PropertyDecl::cast).find(|d| {
            let els: Vec<_> = d.elements().collect();
            els.len() == 1
                && els[0].name().as_deref() == Some(prop_name.as_str())
                && !els[0].syntax().children().any(|c| is_expr(c.kind()))
        }) else {
            continue;
        };
        let mods: Vec<String> = prop_decl
            .modifiers()
            .map(|m| m.modifiers().map(|t| t.text().to_string()).collect())
            .unwrap_or_default();
        let mods = if mods.is_empty() { "public".to_string() } else { mods.join(" ") };
        let ty =
            prop_decl.ty().map(|t| t.text().to_string()).or_else(|| p.ty().map(|t| t.text().to_string()));
        // rewrite the parameter
        let default = p.default_value().map(|d| format!(" = {}", d.text())).unwrap_or_default();
        let by_ref = if p.by_ref() { "&" } else { "" };
        let new_param =
            format!("{mods} {}{by_ref}${pname}{default}", ty.map(|t| format!("{t} ")).unwrap_or_default());
        change.push(sema.file, TextEdit { range: p.syntax().text_range(), new_text: new_param });
        // remove the assignment and the property declaration (with their lines)
        for n in [assign_stmt.clone(), prop_decl.syntax().clone()] {
            let r = n.text_range();
            let line_start = sema.text[..usize::from(r.start())].rfind('\n').map_or(0, |i| i + 1);
            let end = usize::from(r.end());
            let line_end = sema.text[end..].find('\n').map_or(sema.text.len(), |i| end + i + 1);
            let doc = ast::doc_comment(&n).map(|d| d.text_range().start());
            let start = doc
                .map(|d| sema.text[..usize::from(d)].rfind('\n').map_or(0, |i| i + 1))
                .unwrap_or(line_start);
            change.push(
                sema.file,
                TextEdit {
                    range: TextRange::new(TextSize::from(start as u32), TextSize::from(line_end as u32)),
                    new_text: String::new(),
                },
            );
        }
        count += 1;
    }
    if count > 0 {
        out.push(CodeAction {
            title: "Promote constructor properties".into(),
            kind: CodeActionKind::RefactorRewrite,
            change,
            is_preferred: false,
            fixes: None,
        });
    }
}

fn generate_constructor(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(class) = node.ancestors().find(|n| n.kind() == CLASS_DECL) else { return };
    let Some(members) = class.children().find(|c| c.kind() == MEMBER_LIST) else { return };
    if constructor_of(&class).is_some()
        || members.text_range().contains(node.text_range().start())
            && node.ancestors().any(|a| matches!(a.kind(), METHOD_DECL))
    {
        return;
    }
    let props: Vec<(String, Option<String>)> = members
        .children()
        .filter_map(ast::PropertyDecl::cast)
        .filter(|d| !d.modifiers().is_some_and(|m| m.has(STATIC_KW)))
        .flat_map(|d| {
            let ty = d.ty().map(|t| t.text().to_string());
            d.elements()
                .filter(|e| !e.syntax().children().any(|c| is_expr(c.kind())))
                .filter_map(move |e| Some((e.name()?, ty.clone())))
                .collect::<Vec<_>>()
        })
        .collect();
    if props.is_empty() {
        return;
    }
    let unit = indent_unit(sema);
    let indent = format!("{}{}", line_indent(sema, class.text_range().start()), unit);
    let params: Vec<String> = props
        .iter()
        .map(|(n, t)| format!("{}${n}", t.as_ref().map(|t| format!("{t} ")).unwrap_or_default()))
        .collect();
    let assigns: String = props.iter().map(|(n, _)| format!("{indent}{unit}$this->{n} = ${n};\n")).collect();
    // after the last property
    let last_prop = members.children().filter(|c| c.kind() == PROPERTY_DECL).last().unwrap();
    let at = last_prop.text_range().end();
    let text = format!(
        "\n\n{indent}public function __construct({})\n{indent}{{\n{assigns}{indent}}}",
        params.join(", ")
    );
    out.push(CodeAction {
        title: "Generate constructor".into(),
        kind: CodeActionKind::Refactor,
        change: edit(sema, TextRange::empty(at), text),
        is_preferred: false,
        fixes: None,
    });
}

fn add_return_type(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(func) = node
        .ancestors()
        .find(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR))
    else {
        return;
    };
    if func.children().any(|c| c.kind() == RETURN_TYPE) {
        return;
    }
    if let Some(body) = func.children().find(|c| c.kind() == BLOCK) {
        if body.text_range().contains(node.text_range().start()) {
            return;
        }
    }
    if func
        .children()
        .any(|c| c.kind() == MEMBER_NAME && ast::text_without_trivia(&c).eq_ignore_ascii_case("__construct"))
    {
        return;
    }
    let Some(ty) = crate::inlay_hints::inferred_return_type(sema, &func) else { return };
    if ty.is_mixed() || matches!(ty, php_hir::Type::Shape(_)) {
        return;
    }
    // render with short names where possible and PHP syntax only
    let text = type_declaration(sema, &ty, func.text_range().start());
    let Some(text) = text else { return };
    let Some(params) = func.children().find(|c| c.kind() == PARAM_LIST) else { return };
    let anchor = func.children().find(|c| c.kind() == CLOSURE_USE).unwrap_or(params);
    out.push(CodeAction {
        title: format!("Add return type '{text}'"),
        kind: CodeActionKind::RefactorRewrite,
        change: edit(sema, TextRange::empty(anchor.text_range().end()), format!(": {text}")),
        is_preferred: false,
        fixes: None,
    });
}

/// A type as valid PHP declaration syntax (or `None` if not expressible).
fn type_declaration(sema: &Sema, ty: &php_hir::Type, at: TextSize) -> Option<String> {
    use php_hir::Type::*;
    type S = std::string::String;
    let scope = sema.scope_at(at);
    let one = |t: &php_hir::Type| -> Option<S> {
        Some(match t {
            Int => "int".into(),
            Float => "float".into(),
            php_hir::Type::String | Literal(_) => "string".into(),
            Bool | True | False => "bool".into(),
            Null => "null".into(),
            Void => "void".into(),
            Never => "never".into(),
            Array(..) | List(_) | Shape(_) => "array".into(),
            Iterable(..) => "iterable".into(),
            Callable => "callable".into(),
            Object => "object".into(),
            Static => "static".into(),
            Closure(..) => "\\Closure".into(),
            Named(n, _) => scope.shortest_class_name(n).unwrap_or_else(|| format!("\\{n}")),
            _ => return None,
        })
    };
    match ty {
        Union(ts) => {
            let mut parts: Vec<S> = Vec::new();
            for t in ts {
                let p = one(t)?;
                if !parts.contains(&p) {
                    parts.push(p);
                }
            }
            if parts.len() == 2 && parts.iter().any(|p| p == "null") {
                let other = parts.iter().find(|p| *p != "null").unwrap();
                return Some(format!("?{other}"));
            }
            if sema.ws.config().php_version < php_syntax::PhpVersion::PHP_8_0 {
                return None;
            }
            Some(parts.join("|"))
        }
        t => one(t),
    }
}

/// PSR-4: suggest the namespace matching the file location (composer.json).
fn fix_namespace(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(ns) = node.ancestors().find(|n| n.kind() == NAMESPACE_DECL) else { return };
    let Some(name) = ns.children().find(|c| c.kind() == QUALIFIED_NAME) else { return };
    if !name.text_range().contains_inclusive(node.text_range().start()) && node.kind() != NAMESPACE_DECL {
        return;
    }
    let Some(file) = sema.ws.file(sema.file) else { return };
    let Some(expected) = crate::composer::expected_namespace(&file.path) else { return };
    let current = ast::text_without_trivia(&name);
    if current.eq_ignore_ascii_case(&expected) {
        return;
    }
    out.push(CodeAction {
        title: format!("Change namespace to '{expected}'"),
        kind: CodeActionKind::QuickFix,
        change: edit(sema, name.text_range(), expected),
        is_preferred: true,
        fixes: None,
    });
}
