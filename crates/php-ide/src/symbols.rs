//! Document symbols (outline).

use php_syntax::ast::{self, AstNode, ClassLike, Member};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Namespace,
    Class,
    Interface,
    Trait,
    Enum,
    EnumMember,
    Method,
    Constructor,
    Property,
    Constant,
    Function,
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKind,
    pub deprecated: bool,
    /// Range of the whole declaration.
    pub range: TextRange,
    /// Range of the name.
    pub selection_range: TextRange,
    pub children: Vec<Symbol>,
}

pub(crate) fn document_symbols(file: &ast::SourceFile) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut current_ns: Option<Symbol> = None;
    for stmt in file.statements() {
        if let Some(ns) = ast::NamespaceDecl::cast(stmt.clone()) {
            if let Some(prev) = current_ns.take() {
                out.push(prev);
            }
            let name = ns.name().map(|n| n.text()).unwrap_or_default();
            let selection_range =
                ns.name().map(|n| n.syntax().text_range()).unwrap_or_else(|| first_token_range(&stmt));
            let mut sym = Symbol {
                name: if name.is_empty() { "(global)".to_string() } else { name },
                detail: None,
                kind: SymbolKind::Namespace,
                deprecated: false,
                range: stmt.text_range(),
                selection_range,
                children: Vec::new(),
            };
            if let Some(body) = ns.body() {
                collect(body.syntax(), &mut sym.children);
                out.push(sym);
            } else {
                current_ns = Some(sym);
            }
            continue;
        }
        match current_ns.as_mut() {
            Some(ns) => {
                ns.range = ns.range.cover(stmt.text_range());
                collect_node(&stmt, &mut ns.children);
            }
            None => collect_node(&stmt, &mut out),
        }
    }
    if let Some(ns) = current_ns {
        out.push(ns);
    }
    out
}

fn first_token_range(node: &SyntaxNode) -> TextRange {
    node.first_token().map(|t| t.text_range()).unwrap_or_else(|| node.text_range())
}

fn collect(parent: &SyntaxNode, out: &mut Vec<Symbol>) {
    for child in parent.children() {
        collect_node(&child, out);
    }
}

fn collect_node(node: &SyntaxNode, out: &mut Vec<Symbol>) {
    match node.kind() {
        FUNCTION_DECL => {
            let f = ast::FunctionDecl::cast(node.clone()).unwrap();
            let Some(name) = f.name() else { return };
            out.push(Symbol {
                name: name.text(),
                detail: Some(signature(f.param_list(), f.return_type())),
                kind: SymbolKind::Function,
                deprecated: is_deprecated(node),
                range: node.text_range(),
                selection_range: name.syntax().text_range(),
                children: Vec::new(),
            });
        }
        CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL => {
            let c = ClassLike::cast(node.clone()).unwrap();
            if let Some(sym) = class_symbol(&c) {
                out.push(sym);
            }
        }
        CONST_DECL => {
            let c = ast::ConstDecl::cast(node.clone()).unwrap();
            for el in c.elements() {
                if let (Some(name), Some(name_node)) = (el.name_text(), el.name_node()) {
                    out.push(Symbol {
                        name,
                        detail: el.value().map(|v| short_text(&v)),
                        kind: SymbolKind::Constant,
                        deprecated: is_deprecated(node),
                        range: el.syntax().text_range(),
                        selection_range: name_node.text_range(),
                        children: Vec::new(),
                    });
                }
            }
        }
        CALL_EXPR => {
            if let Some(sym) = define_call(node) {
                out.push(sym);
            }
            collect(node, out);
        }
        // Declarations inside these are not part of the outline.
        METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR | ANON_CLASS | MEMBER_LIST => {}
        _ => collect(node, out),
    }
}

/// `define('NAME', value)`
fn define_call(node: &SyntaxNode) -> Option<Symbol> {
    let call = ast::CallExpr::cast(node.clone())?;
    let callee = call.syntax().first_child()?;
    if callee.kind() != NAME_EXPR
        || !ast::text_without_trivia(&callee).trim_start_matches('\\').eq_ignore_ascii_case("define")
    {
        return None;
    }
    let args = call.syntax().children().find(|c| c.kind() == ARG_LIST)?;
    let mut arg_nodes = args.children().filter(|c| c.kind() == ARG);
    let first = arg_nodes.next()?;
    let lit = first.first_child().filter(|c| c.kind() == LITERAL)?;
    let tok = lit.first_token()?;
    if tok.kind() != STRING {
        return None;
    }
    let name = tok.text().trim_matches(|c| c == '\'' || c == '"').to_string();
    Some(Symbol {
        name,
        detail: arg_nodes.next().and_then(|a| a.first_child()).map(|v| short_text(&v)),
        kind: SymbolKind::Constant,
        deprecated: false,
        range: node.text_range(),
        selection_range: tok.text_range(),
        children: Vec::new(),
    })
}

fn class_symbol(c: &ClassLike) -> Option<Symbol> {
    let name = c.name()?;
    let kind = match c.kind() {
        ast::ClassLikeKind::Class => SymbolKind::Class,
        ast::ClassLikeKind::Interface => SymbolKind::Interface,
        ast::ClassLikeKind::Trait => SymbolKind::Trait,
        ast::ClassLikeKind::Enum => SymbolKind::Enum,
    };
    let mut children = Vec::new();
    if let Some(members) = c.member_list() {
        for m in members.members() {
            member_symbols(&m, &mut children);
        }
    }
    let mut detail = Vec::new();
    if let Some(ext) = c.extends() {
        let names: Vec<_> = ext.names().map(|n| n.text()).collect();
        detail.push(format!("extends {}", names.join(", ")));
    }
    if let Some(imp) = c.implements() {
        let names: Vec<_> = imp.names().map(|n| n.text()).collect();
        detail.push(format!("implements {}", names.join(", ")));
    }
    Some(Symbol {
        name: name.text(),
        detail: if detail.is_empty() { None } else { Some(detail.join(" ")) },
        kind,
        deprecated: is_deprecated(c.syntax()),
        range: c.syntax().text_range(),
        selection_range: name.syntax().text_range(),
        children,
    })
}

fn member_symbols(m: &Member, out: &mut Vec<Symbol>) {
    match m {
        Member::Method(method) => {
            let Some(name) = method.name() else { return };
            let text = name.text().unwrap_or_default();
            let kind = if text.eq_ignore_ascii_case("__construct") {
                SymbolKind::Constructor
            } else {
                SymbolKind::Method
            };
            let mut children = Vec::new();
            // promoted constructor properties
            if let Some(params) = method.param_list() {
                for p in params.params().filter(|p| p.modifiers().is_some()) {
                    if let (Some(n), Some(v)) = (p.name(), p.variable()) {
                        children.push(Symbol {
                            name: format!("${n}"),
                            detail: p.ty().map(|t| ast::text_without_trivia(&t)),
                            kind: SymbolKind::Property,
                            deprecated: false,
                            range: p.syntax().text_range(),
                            selection_range: v.text_range(),
                            children: Vec::new(),
                        });
                    }
                }
            }
            out.push(Symbol {
                name: text,
                detail: Some(signature(method.param_list(), method.return_type())),
                kind,
                deprecated: is_deprecated(method.syntax()),
                range: method.syntax().text_range(),
                selection_range: name.syntax().text_range(),
                children,
            });
        }
        Member::Property(prop) => {
            let ty = prop.ty().map(|t| ast::text_without_trivia(&t));
            for el in prop.elements() {
                let Some(var) = el.variable() else { continue };
                out.push(Symbol {
                    name: var.text().to_string(),
                    detail: ty.clone(),
                    kind: SymbolKind::Property,
                    deprecated: is_deprecated(prop.syntax()),
                    range: el.syntax().text_range(),
                    selection_range: var.text_range(),
                    children: Vec::new(),
                });
            }
        }
        Member::Const(c) => {
            for el in c.elements() {
                if let (Some(name), Some(name_node)) = (el.name_text(), el.name_node()) {
                    out.push(Symbol {
                        name,
                        detail: el.value().map(|v| short_text(&v)),
                        kind: SymbolKind::Constant,
                        deprecated: is_deprecated(c.syntax()),
                        range: el.syntax().text_range(),
                        selection_range: name_node.text_range(),
                        children: Vec::new(),
                    });
                }
            }
        }
        Member::EnumCase(case) => {
            let Some(name) = case.name() else { return };
            out.push(Symbol {
                name: name.text().unwrap_or_default(),
                detail: None,
                kind: SymbolKind::EnumMember,
                deprecated: is_deprecated(case.syntax()),
                range: case.syntax().text_range(),
                selection_range: name.syntax().text_range(),
                children: Vec::new(),
            });
        }
        Member::TraitUse(_) => {}
    }
}

/// `(int $a, string ...$rest): bool`
pub(crate) fn signature(params: Option<ast::ParamList>, ret: Option<ast::ReturnType>) -> String {
    let mut s = String::from("(");
    if let Some(params) = params {
        let parts: Vec<String> = params
            .params()
            .map(|p| {
                let mut part = String::new();
                if let Some(m) = p.modifiers() {
                    for t in m.modifiers() {
                        part.push_str(t.text());
                        part.push(' ');
                    }
                }
                if let Some(t) = p.ty() {
                    part.push_str(&ast::text_without_trivia(&t));
                    part.push(' ');
                }
                if p.by_ref() {
                    part.push('&');
                }
                if p.is_variadic() {
                    part.push_str("...");
                }
                if let Some(v) = p.variable() {
                    part.push_str(v.text());
                }
                if let Some(d) = p.default_value() {
                    part.push_str(" = ");
                    part.push_str(&short_text(&d));
                }
                part
            })
            .collect();
        s.push_str(&parts.join(", "));
    }
    s.push(')');
    if let Some(t) = ret.and_then(|r| r.ty()) {
        s.push_str(": ");
        s.push_str(&ast::text_without_trivia(&t));
    }
    s
}

fn short_text(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 40 {
        let mut t: String = text.chars().take(37).collect();
        t.push_str("...");
        t
    } else {
        text
    }
}

fn is_deprecated(node: &SyntaxNode) -> bool {
    if ast::doc_comment(node).is_some_and(|d| d.text().contains("@deprecated")) {
        return true;
    }
    node.children().filter(|c| c.kind() == ATTRIBUTE_GROUP).flat_map(|g| g.children()).any(|a| {
        ast::Attribute::cast(a)
            .and_then(|a| a.name())
            .is_some_and(|n| n.text().trim_start_matches('\\').eq_ignore_ascii_case("Deprecated"))
    })
}

#[cfg(test)]
mod tests {
    use crate::{Config, Workspace};

    fn outline(text: &str) -> String {
        let mut ws = Workspace::new(Config::default());
        let id = ws.set_file("t.php", text.to_string());
        let mut out = String::new();
        fn walk(s: &[super::Symbol], depth: usize, out: &mut String) {
            for sym in s {
                out.push_str(&format!(
                    "{:indent$}{:?} {} {}{}\n",
                    "",
                    sym.kind,
                    sym.name,
                    sym.detail.clone().unwrap_or_default(),
                    if sym.deprecated { " (deprecated)" } else { "" },
                    indent = depth * 2
                ));
                walk(&sym.children, depth + 1, out);
            }
        }
        walk(&ws.document_symbols(id), 0, &mut out);
        out
    }

    #[test]
    fn outline_tree() {
        let text = r#"<?php
namespace App\Models;

const VERSION = '1.0';
define('DEBUG', true);

/** @deprecated */
function helper(int $a, ?string ...$rest): bool { function inner() {} return true; }

final class User extends Model implements Jsonable {
    const TABLE = 'users';
    public ?string $name = null, $email;
    public function __construct(private readonly int $id) {}
    public static function find(int $id): ?static { return null; }
}

interface Jsonable { public function toJson(): string; }
trait HasName { protected string $name; }
enum Status: int { case Active = 1; case Inactive = 0; }
if (!function_exists('polyfill')) { function polyfill() {} }
"#;
        expect_test::expect![[r#"
            Namespace App\Models 
              Constant VERSION '1.0'
              Constant DEBUG true
              Function helper (int $a, ?string ...$rest): bool (deprecated)
              Class User extends Model implements Jsonable
                Constant TABLE 'users'
                Property $name ?string
                Property $email ?string
                Constructor __construct (private readonly int $id)
                  Property $id int
                Method find (int $id): ?static
              Interface Jsonable 
                Method toJson (): string
              Trait HasName 
                Property $name string
              Enum Status 
                EnumMember Active 
                EnumMember Inactive 
              Function polyfill ()
        "#]]
        .assert_eq(&outline(text));
    }
}
