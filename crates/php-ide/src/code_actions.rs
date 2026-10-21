//! Code actions: quick fixes, refactorings and source actions.

use php_hir::symbols::FunctionDef;
use php_hir::{Fqn, Type};
use php_syntax::ast::{self, AstNode, ClassLikeKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::completion::{use_edit, TextEdit};
use crate::rename::SourceChange;
use crate::sema::Sema;
use crate::{render, DiagnosticCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeActionKind {
    QuickFix,
    Refactor,
    RefactorRewrite,
    Source,
    SourceOrganizeImports,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    pub change: SourceChange,
    pub is_preferred: bool,
    /// Diagnostic code this action fixes.
    pub fixes: Option<DiagnosticCode>,
}

pub(crate) fn code_actions(sema: &Sema, range: TextRange) -> Vec<CodeAction> {
    let mut out = Vec::new();
    let Some(token) = sema.token_at(range.start()) else { return out };
    let Some(node) = token.parent() else { return out };
    import_class(sema, &node, &mut out);
    implement_methods(sema, &node, &mut out);
    remove_unused_import(sema, &node, &mut out);
    add_phpdoc(sema, &node, &mut out);
    getters_setters(sema, &node, &mut out);
    organize_imports(sema, range.start(), &mut out);
    out
}

fn edit(sema: &Sema, range: TextRange, text: impl Into<String>) -> SourceChange {
    SourceChange::single(sema.file, TextEdit { range, new_text: text.into() })
}

/// Indentation (leading whitespace) of the line containing `offset`.
fn line_indent(sema: &Sema, offset: TextSize) -> String {
    let text = &sema.text;
    let o = usize::from(offset).min(text.len());
    let start = text[..o].rfind('\n').map_or(0, |i| i + 1);
    text[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

fn indent_unit(sema: &Sema) -> String {
    // detect from the file: first indented line
    for line in sema.text.lines() {
        if line.starts_with('\t') {
            return "\t".into();
        }
        let n = line.chars().take_while(|c| *c == ' ').count();
        if n > 0 && n <= 8 && !line.trim().is_empty() && !line.trim_start().starts_with('*') {
            return " ".repeat(n);
        }
    }
    "    ".into()
}

// ------------------------------------------------------------------ import

fn import_class(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(qn) = node.ancestors().find(|n| n.kind() == QUALIFIED_NAME) else { return };
    if qn.ancestors().any(|a| a.kind() == USE_DECL || a.kind() == NAMESPACE_DECL) {
        return;
    }
    let text = ast::text_without_trivia(&qn);
    if text.starts_with('\\') || text.contains('\\') {
        return;
    }
    let scope = sema.scope_at(qn.text_range().start());
    let resolved = scope.resolve_class(&text);
    let is_fn = qn
        .parent()
        .filter(|p| p.kind() == NAME_EXPR)
        .and_then(|p| p.parent())
        .is_some_and(|g| g.kind() == CALL_EXPR);
    if is_fn {
        if sema.resolve_function(&text, qn.text_range().start()).is_some() {
            return;
        }
        let mut cands: Vec<Fqn> = sema
            .index()
            .all_functions()
            .filter(|(_, f)| f.fqn.short().eq_ignore_ascii_case(&text))
            .map(|(_, f)| f.fqn.clone())
            .collect();
        cands.sort();
        cands.dedup();
        for fqn in cands.into_iter().take(10) {
            out.push(CodeAction {
                title: format!("Import function '{fqn}'"),
                kind: CodeActionKind::QuickFix,
                change: SourceChange::single(sema.file, use_edit(sema, scope, &format!("function {fqn}"))),
                is_preferred: false,
                fixes: Some(DiagnosticCode::UndefinedFunction),
            });
        }
        return;
    }
    if sema.index().class(&resolved).is_some() {
        return;
    }
    let mut cands: Vec<Fqn> = sema
        .index()
        .all_classes()
        .filter(|(_, c)| c.fqn.short().eq_ignore_ascii_case(&text))
        .map(|(_, c)| c.fqn.clone())
        .collect();
    cands.sort();
    cands.dedup_by(|a, b| a.eq_ignore_case(b));
    let single = cands.len() == 1;
    for fqn in cands.into_iter().take(10) {
        out.push(CodeAction {
            title: format!("Import '{fqn}'"),
            kind: CodeActionKind::QuickFix,
            change: SourceChange::single(sema.file, use_edit(sema, scope, fqn.as_str())),
            is_preferred: single,
            fixes: Some(DiagnosticCode::UndefinedType),
        });
        out.push(CodeAction {
            title: format!("Fully qualify as '\\{fqn}'"),
            kind: CodeActionKind::QuickFix,
            change: edit(sema, qn.text_range(), format!("\\{fqn}")),
            is_preferred: false,
            fixes: Some(DiagnosticCode::UndefinedType),
        });
    }
}

// ------------------------------------------------------------------ implement

pub(crate) fn method_stub(f: &FunctionDef, indent: &str, unit: &str, owner_is_interface: bool) -> String {
    let mut sig = render::function_signature(f, true);
    if f.is_abstract || owner_is_interface {
        sig = sig.replacen("abstract ", "", 1);
    }
    let body = format!("{indent}{unit}throw new \\Exception('Not implemented');");
    format!("{indent}{sig}\n{indent}{{\n{body}\n{indent}}}")
}

fn implement_methods(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(class) = node.ancestors().find(|n| matches!(n.kind(), CLASS_DECL | ENUM_DECL)) else { return };
    // only on the header of the class
    let Some(members) = class.children().find(|c| c.kind() == MEMBER_LIST) else { return };
    if members.text_range().contains(node.text_range().start()) {
        return;
    }
    let Some(fqn) = sema.class_fqn(&class) else { return };
    let Some((_, def)) = sema.index().class(&fqn) else { return };
    if def.is_abstract {
        return;
    }
    let all = sema.index().members_with(&fqn, false);
    let missing: Vec<_> = all
        .methods
        .iter()
        .filter(|m| m.def.is_abstract && !m.def.is_magic && !m.owner.fqn.eq_ignore_case(&fqn))
        .filter(|m| {
            !(def.kind == ClassLikeKind::Enum
                && matches!(m.def.name.to_ascii_lowercase().as_str(), "cases" | "from" | "tryfrom"))
        })
        .collect();
    if missing.is_empty() {
        return;
    }
    let Some(close) = members.last_token().filter(|t| t.kind() == R_CURLY) else { return };
    let unit = indent_unit(sema);
    let indent = format!("{}{}", line_indent(sema, class.text_range().start()), unit);
    let has_members = members.children().next().is_some();
    let mut text = String::new();
    for (i, m) in missing.iter().enumerate() {
        if i > 0 || has_members {
            text.push('\n');
        }
        let is_iface =
            sema.index().class(&m.owner.fqn).is_some_and(|(_, c)| c.kind == ClassLikeKind::Interface);
        text.push_str(&method_stub(m.def, &indent, &unit, is_iface));
        text.push('\n');
    }
    // insert before the closing brace, at the start of its line
    let close_start = close.text_range().start();
    let line_start = sema.text[..usize::from(close_start)].rfind('\n').map_or(0, |i| i + 1);
    let only_ws = sema.text[line_start..usize::from(close_start)].trim().is_empty();
    let (at, prefix) = if only_ws { (TextSize::from(line_start as u32), "") } else { (close_start, "\n") };
    out.push(CodeAction {
        title: format!(
            "Implement {} missing method{}",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" }
        ),
        kind: CodeActionKind::QuickFix,
        change: edit(sema, TextRange::empty(at), format!("{prefix}{text}")),
        is_preferred: true,
        fixes: Some(DiagnosticCode::MissingImplementation),
    });
}

// ------------------------------------------------------------------ imports

fn remove_unused_import(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(decl) = node.ancestors().find(|n| n.kind() == USE_DECL) else { return };
    let diags = crate::diagnostics::semantic_diagnostics(sema);
    let unused: Vec<TextRange> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnusedSymbol && decl.text_range().contains_range(d.range))
        .map(|d| d.range)
        .collect();
    if unused.is_empty() {
        return;
    }
    let clauses = decl
        .descendants()
        .filter(|n| n.kind() == USE_CLAUSE && n.children().all(|c| c.kind() != USE_GROUP))
        .count();
    let change = if unused.len() >= clauses {
        // remove the whole statement including its line break
        let r = decl.text_range();
        let end = usize::from(r.end());
        let end = if sema.text[end..].starts_with('\n') { end + 1 } else { end };
        edit(sema, TextRange::new(r.start(), TextSize::from(end as u32)), "")
    } else {
        let mut change = SourceChange::default();
        for u in &unused {
            // remove the clause and an adjacent comma
            let after = usize::from(u.end());
            let rest = &sema.text[after..];
            let comma = rest.find(',').filter(|i| rest[..*i].trim().is_empty());
            let range = match comma {
                Some(i) => {
                    let ws = rest[i + 1..].chars().take_while(|c| c.is_whitespace()).count();
                    TextRange::new(u.start(), TextSize::from((after + i + 1 + ws) as u32))
                }
                None => {
                    let before = &sema.text[..usize::from(u.start())];
                    match before.rfind(',') {
                        Some(i) if before[i + 1..].trim().is_empty() => {
                            TextRange::new(TextSize::from(i as u32), u.end())
                        }
                        _ => *u,
                    }
                }
            };
            change.push(sema.file, TextEdit { range, new_text: String::new() });
        }
        change
    };
    out.push(CodeAction {
        title: "Remove unused import".into(),
        kind: CodeActionKind::QuickFix,
        change,
        is_preferred: true,
        fixes: Some(DiagnosticCode::UnusedSymbol),
    });
}

fn organize_imports(sema: &Sema, offset: TextSize, out: &mut Vec<CodeAction>) {
    let scope_node = sema
        .root
        .descendants()
        .filter(|n| n.kind() == NAMESPACE_DECL && n.text_range().contains_inclusive(offset))
        .find_map(|n| n.children().find(|c| c.kind() == BLOCK))
        .unwrap_or_else(|| sema.root.clone());
    let uses: Vec<SyntaxNode> = scope_node.children().filter(|c| c.kind() == USE_DECL).collect();
    if uses.len() < 2 {
        return;
    }
    // only contiguous blocks of use statements are sorted
    let first = uses[0].text_range().start();
    let last = uses[uses.len() - 1].text_range().end();
    let between = &sema.text[usize::from(first)..usize::from(last)];
    let non_use_between = scope_node
        .children()
        .any(|c| c.kind() != USE_DECL && c.text_range().start() > first && c.text_range().end() < last);
    if non_use_between || between.contains("/*") || between.contains("//") || between.contains('#') {
        return;
    }
    let mut stmts: Vec<String> = uses.iter().map(|u| u.text().to_string().trim().to_string()).collect();
    let kind_rank = |s: &str| {
        let l = s.to_ascii_lowercase();
        if l.starts_with("use function") {
            1
        } else if l.starts_with("use const") {
            2
        } else {
            0
        }
    };
    stmts.sort_by(|a, b| {
        kind_rank(a).cmp(&kind_rank(b)).then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()))
    });
    stmts.dedup();
    let indent = line_indent(sema, first);
    let new_text = stmts.join(&format!("\n{indent}"));
    if new_text == between {
        return;
    }
    out.push(CodeAction {
        title: "Sort imports".into(),
        kind: CodeActionKind::SourceOrganizeImports,
        change: edit(sema, TextRange::new(first, last), new_text),
        is_preferred: false,
        fixes: None,
    });
}

// ------------------------------------------------------------------ phpdoc

pub(crate) fn phpdoc_for(sema: &Sema, decl: &SyntaxNode, indent: &str, snippet: bool) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut tab = 1;
    let mut placeholder = |default: &str| {
        if snippet {
            let s = format!("${{{tab}:{default}}}");
            tab += 1;
            s
        } else {
            default.to_string()
        }
    };
    match decl.kind() {
        FUNCTION_DECL | METHOD_DECL => {
            lines.push(placeholder(""));
            let params = decl.children().find_map(ast::ParamList::cast);
            let mut tags = Vec::new();
            if let Some(params) = params {
                for p in params.params() {
                    let ty = p.ty().map(|t| ast::text_without_trivia(&t)).unwrap_or_else(|| "mixed".into());
                    let mut name = String::new();
                    if p.is_variadic() {
                        name.push_str("...");
                    }
                    name.push_str(p.variable().map(|v| v.text().to_string()).as_deref().unwrap_or("$"));
                    tags.push(format!("@param {} {}", placeholder(&ty), name));
                }
            }
            let ret = decl
                .children()
                .find_map(ast::ReturnType::cast)
                .and_then(|r| r.ty())
                .map(|t| ast::text_without_trivia(&t));
            let is_ctor = decl
                .children()
                .find(|c| c.kind() == MEMBER_NAME)
                .is_some_and(|n| ast::text_without_trivia(&n).eq_ignore_ascii_case("__construct"));
            if !is_ctor {
                let ret = ret
                    .or_else(|| crate::inlay_hints::inferred_return_type(sema, decl).map(|t| t.to_string()));
                if let Some(r) = ret.filter(|r| r != "void") {
                    tags.push(format!("@return {}", placeholder(&r)));
                }
            }
            if !tags.is_empty() {
                lines.push(String::new());
                lines.extend(tags);
            }
        }
        CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL => lines.push(placeholder("")),
        PROPERTY_DECL | CLASS_CONST_DECL => {
            let ty = decl
                .children()
                .find(|c| ast::is_type_kind(c.kind()))
                .map(|t| ast::text_without_trivia(&t))
                .unwrap_or_else(|| "mixed".into());
            lines.push(format!("@var {}", placeholder(&ty)));
        }
        _ => return None,
    }
    let mut s = String::from("/**\n");
    for l in lines {
        if l.is_empty() {
            s.push_str(&format!("{indent} *\n"));
        } else {
            s.push_str(&format!("{indent} * {l}\n"));
        }
    }
    s.push_str(&format!("{indent} */"));
    Some(s)
}

fn add_phpdoc(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(decl) = node.ancestors().find(|n| {
        matches!(
            n.kind(),
            FUNCTION_DECL
                | METHOD_DECL
                | CLASS_DECL
                | INTERFACE_DECL
                | TRAIT_DECL
                | ENUM_DECL
                | PROPERTY_DECL
                | CLASS_CONST_DECL
        )
    }) else {
        return;
    };
    // only on the declaration header, not inside bodies
    if let Some(body) = decl.children().find(|c| matches!(c.kind(), BLOCK | MEMBER_LIST)) {
        if body.text_range().contains(node.text_range().start()) {
            return;
        }
    }
    if ast::doc_comment(&decl).is_some() {
        return;
    }
    let start = decl.text_range().start();
    let indent = line_indent(sema, start);
    let Some(doc) = phpdoc_for(sema, &decl, &indent, false) else { return };
    out.push(CodeAction {
        title: "Add PHPDoc".into(),
        kind: CodeActionKind::Refactor,
        change: edit(sema, TextRange::empty(start), format!("{doc}\n{indent}")),
        is_preferred: false,
        fixes: None,
    });
}

// ------------------------------------------------------------------ getters / setters

fn getters_setters(sema: &Sema, node: &SyntaxNode, out: &mut Vec<CodeAction>) {
    let Some(prop) = node.ancestors().find(|n| n.kind() == PROPERTY_DECL) else { return };
    let Some(class) = prop.ancestors().find(|n| matches!(n.kind(), CLASS_DECL | TRAIT_DECL)) else { return };
    let Some(fqn) = sema.class_fqn(&class) else { return };
    let Some((_, def)) = sema.index().class(&fqn) else { return };
    let Some(members) = class.children().find(|c| c.kind() == MEMBER_LIST) else { return };
    let Some(close) = members.last_token().filter(|t| t.kind() == R_CURLY) else { return };
    let unit = indent_unit(sema);
    let indent = format!("{}{}", line_indent(sema, class.text_range().start()), unit);
    let elements = ast::PropertyDecl::cast(prop.clone())
        .map(|p| p.elements().filter_map(|e| e.name()).collect::<Vec<_>>())
        .unwrap_or_default();
    for name in elements {
        let Some(p) = def.property(&name) else { continue };
        let ty = p.ty.as_ref().map(|t| t.to_string());
        let ucfirst = {
            let mut c = name.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        let stat = if p.is_static { "static " } else { "" };
        let access = if p.is_static { format!("self::${name}") } else { format!("$this->{name}") };
        let is_bool = matches!(p.ty, Some(Type::Bool));
        let getter_name = if is_bool { format!("is{ucfirst}") } else { format!("get{ucfirst}") };
        let mut actions = Vec::new();
        if def.method(&getter_name).is_none() {
            let ret = ty.as_ref().map(|t| format!(": {t}")).unwrap_or_default();
            actions.push((
                format!("Generate getter for ${name}"),
                format!("{indent}public {stat}function {getter_name}(){ret}\n{indent}{{\n{indent}{unit}return {access};\n{indent}}}\n"),
            ));
        }
        let setter_name = format!("set{ucfirst}");
        if def.method(&setter_name).is_none() && !p.is_readonly {
            let param_ty = ty.as_ref().map(|t| format!("{t} ")).unwrap_or_default();
            let ret = if p.is_static { "void" } else { "static" };
            let body = if p.is_static {
                format!("{indent}{unit}{access} = ${name};\n")
            } else {
                format!("{indent}{unit}{access} = ${name};\n\n{indent}{unit}return $this;\n")
            };
            actions.push((
                format!("Generate setter for ${name}"),
                format!("{indent}public {stat}function {setter_name}({param_ty}${name}): {ret}\n{indent}{{\n{body}{indent}}}\n"),
            ));
        }
        let close_start = close.text_range().start();
        let line_start = sema.text[..usize::from(close_start)].rfind('\n').map_or(0, |i| i + 1);
        let at = TextSize::from(line_start as u32);
        let both: String = actions.iter().map(|(_, t)| format!("\n{t}")).collect();
        if actions.len() == 2 {
            out.push(CodeAction {
                title: format!("Generate getter and setter for ${name}"),
                kind: CodeActionKind::Refactor,
                change: edit(sema, TextRange::empty(at), both),
                is_preferred: false,
                fixes: None,
            });
        }
        for (title, text) in actions {
            out.push(CodeAction {
                title,
                kind: CodeActionKind::Refactor,
                change: edit(sema, TextRange::empty(at), format!("\n{text}")),
                is_preferred: false,
                fixes: None,
            });
        }
    }
}
