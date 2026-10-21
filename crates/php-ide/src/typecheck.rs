//! Argument type checks (Intelephense P1006), relaxed like Intelephense's
//! default `diagnostics.relaxedTypeCheck`: a union is accepted when one of
//! its members fits, scalars convert into each other (unless the file
//! declares `strict_types=1`), and unknown classes are always accepted.

use php_hir::{Fqn, Type};
use php_syntax::ast::AstNode;
use php_syntax::{SyntaxKind::*, SyntaxNode};

use crate::diagnostics::{Checker, Diagnostic, DiagnosticCode, Severity};

fn is_scalar(t: &Type) -> bool {
    matches!(
        t,
        Type::Int
            | Type::Float
            | Type::String
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Literal(_)
            | Type::ClassString(_)
    )
}

fn is_arrayish(t: &Type) -> bool {
    matches!(t, Type::Array(..) | Type::List(_) | Type::Shape(_))
}

impl Checker<'_, '_> {
    /// Whether a value of type `arg` may be passed as `param`; `None` when
    /// it cannot be decided.
    fn accepts(&self, param: &Type, arg: &Type, strict: bool) -> bool {
        let index = self.sema.index();
        // names we do not know (template parameters of functions, pseudo
        // types like `uppercase-string`, missing dependencies)
        if let Type::Named(c, _) = arg {
            if index.class(c).is_none() {
                return true;
            }
        }
        match (param, arg) {
            (_, Type::Mixed | Type::Template(_) | Type::Never) => true,
            (Type::Mixed | Type::Template(_) | Type::SelfType | Type::Static | Type::Parent, _) => true,
            (Type::Intersection(_), _) | (_, Type::Intersection(_)) => true,
            (Type::Union(ps), _) => ps.iter().any(|p| self.accepts(p, arg, strict)),
            // relaxed: one member of the argument union is enough
            (_, Type::Union(args)) => args.iter().any(|a| self.accepts(param, a, strict)),
            (Type::Null, a) => matches!(a, Type::Null),
            (_, Type::Null) => false,
            (Type::Void, _) => true,
            (Type::Bool | Type::True | Type::False, a) => is_scalar(a) || !strict && matches!(a, Type::Null),
            (Type::Int | Type::Float | Type::String, a) if is_scalar(a) => {
                if !strict {
                    return true;
                }
                match (param, a) {
                    (Type::Float, Type::Int | Type::Float) => true,
                    (Type::Int, Type::Int) | (Type::String, Type::String | Type::ClassString(_)) => true,
                    (_, Type::Literal(l)) => match param {
                        Type::String => l.starts_with(['\'', '"']),
                        _ => !l.starts_with(['\'', '"']),
                    },
                    _ => false,
                }
            }
            // objects with __toString for string parameters
            (Type::String, Type::Named(c, _)) => {
                !strict && (index.find_method(c, "__toString").is_some() || self.unknown(c))
            }
            (Type::Int | Type::Float | Type::String, _) => false,
            // `iterable` may be an array
            (Type::Array(..) | Type::List(_) | Type::Shape(_), a) => {
                is_arrayish(a) || matches!(a, Type::Iterable(..))
            }
            (Type::Iterable(..), a) => {
                is_arrayish(a)
                    || matches!(a, Type::Iterable(..) | Type::Object)
                    || matches!(a, Type::Named(c, _) if self.unknown(c) || index.is_subtype(c, &Fqn::new("Traversable")))
            }
            (Type::Callable | Type::Closure(..), a) => {
                matches!(
                    a,
                    Type::Closure(..) | Type::Callable | Type::String | Type::Literal(_) | Type::Object
                ) || is_arrayish(a)
                    || matches!(a, Type::Named(c, _) if self.unknown(c) || c.short().eq_ignore_ascii_case("Closure") || index.find_method(c, "__invoke").is_some())
            }
            (Type::Object, a) => matches!(
                a,
                Type::Named(..) | Type::Object | Type::Closure(..) | Type::Static | Type::SelfType
            ),
            (Type::ClassString(_), a) => matches!(a, Type::String | Type::Literal(_) | Type::ClassString(_)),
            (Type::Resource, a) => matches!(a, Type::Resource),
            (Type::Named(p, _), a) => match a {
                Type::Callable if p.short().eq_ignore_ascii_case("Closure") => true,
                // `iterable` may be a Traversable; resources became objects in PHP 8
                Type::Iterable(..) | Type::Resource => true,
                Type::Named(c, _) => {
                    self.unknown(c) || self.unknown(p) || index.is_subtype(c, p) || is_closure_like(p, a)
                }
                Type::Closure(..) => p.short().eq_ignore_ascii_case("Closure") || self.unknown(p),
                Type::Object | Type::Static | Type::SelfType | Type::Parent => true,
                // `Stringable` / `Traversable` / `Countable` etc. are classes too
                Type::String | Type::Literal(_) => p.short().eq_ignore_ascii_case("Stringable") && !strict,
                _ => self.unknown(p) && false,
            },
            _ => true,
        }
    }

    fn unknown(&self, c: &Fqn) -> bool {
        let index = self.sema.index();
        index.class(c).is_none() || index.has_unknown_ancestor(c)
    }

    /// P1006 for the arguments of `call` against `params`.
    pub(crate) fn check_argument_types(&mut self, call: &SyntaxNode, params: &[php_hir::symbols::ParamDef]) {
        if !self.config.type_errors {
            return;
        }
        let Some(list) = call.children().find(|c| c.kind() == ARG_LIST) else { return };
        let strict = self.sema.root.descendants().take(40).any(|n| {
            n.kind() == DECLARE_STMT
                && n.text().to_string().replace(char::is_whitespace, "").contains("strict_types=1")
        });
        for (i, arg) in list.children().filter(|c| c.kind() == ARG).enumerate() {
            if arg.children_with_tokens().any(|e| e.kind() == DOT3) {
                return; // spread: positions unknown
            }
            let named = arg
                .children()
                .find(|c| c.kind() == NAME)
                .filter(|_| arg.children_with_tokens().any(|e| e.kind() == COLON))
                .map(|n| php_syntax::ast::text_without_trivia(&n));
            let param = match &named {
                Some(n) => params.iter().find(|p| &p.name == n),
                None => params.get(i).or_else(|| params.last().filter(|p| p.variadic)),
            };
            let Some(param) = param else { continue };
            let (Some(declared), false) = (&param.ty, param.by_ref) else { continue };
            let Some(expr) = arg.children().find(|c| crate::infer::is_expr(c.kind())) else { continue };
            let found = self.sema.infer().expr_type(&expr);
            let declared = if param.default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case("null")) {
                // implicitly nullable
                Type::union(vec![declared.clone(), Type::Null])
            } else {
                declared.clone()
            };
            if !self.accepts(&declared, &found, strict) {
                self.push(Diagnostic::new(
                    expr.text_range(),
                    Severity::Error,
                    DiagnosticCode::TypeMismatch,
                    format!("Expected type '{declared}'. Found '{found}'."),
                ));
            }
        }
    }
}

impl Checker<'_, '_> {
    /// P1131: a `@param` / `@return` type that cannot be a value of the
    /// declared type (e.g. `@param string $x` on `int $x`).
    pub(crate) fn check_doc_compatibility(&mut self, func: &SyntaxNode) {
        if !self.config.type_errors {
            return;
        }
        let Some(doc_tok) = php_syntax::ast::doc_comment(func) else { return };
        let doc = php_hir::DocBlock::parse(doc_tok.text());
        let infer = self.sema.infer();
        let mut checks: Vec<(Type, Type, String)> = Vec::new();
        if let Some(list) = func.children().find_map(php_syntax::ast::ParamList::cast) {
            for p in list.params() {
                let (Some(name), Some(ty)) = (p.name(), p.ty()) else { continue };
                let Some(tag) = doc.param(&name).filter(|t| !t.type_text.is_empty()) else { continue };
                let declared = infer.syntax_type(&ty, func);
                let declared = if p
                    .default_value()
                    .is_some_and(|d| php_syntax::ast::text_without_trivia(&d).eq_ignore_ascii_case("null"))
                {
                    Type::union(vec![declared, Type::Null])
                } else {
                    declared
                };
                checks.push((declared, infer.doc_type(&tag.type_text, func), format!("${name}")));
            }
        }
        if let (Some(ret), Some(tag)) = (
            func.children().find(|c| c.kind() == RETURN_TYPE).and_then(|r| r.children().last()),
            doc.return_tag().filter(|t| !t.type_text.is_empty()),
        ) {
            checks.push((
                infer.syntax_type(&ret, func),
                infer.doc_type(&tag.type_text, func),
                "@return".into(),
            ));
        }
        for (declared, documented, what) in checks {
            // every documented alternative must be possible for the declared type
            let ok = documented.members().into_iter().all(|m| self.accepts(&declared, m, true));
            if ok {
                continue;
            }
            // the tag's line in the doc comment
            let text = doc_tok.text();
            let needle = if what == "@return" { "@return".to_string() } else { what.clone() };
            let range = text
                .match_indices(needle.as_str())
                .find(|(i, _)| {
                    what == "@return"
                        || text[..*i].rfind('@').is_some_and(|at| {
                            text[at..].starts_with("@param")
                                || text[at..].starts_with("@phpstan-param")
                                || text[at..].starts_with("@psalm-param")
                        })
                })
                .map(|(i, _)| {
                    let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
                    let line_end = text[i..].find('\n').map_or(text.len(), |n| i + n);
                    let start = line_start + text[line_start..line_end].find('@').unwrap_or(0);
                    php_syntax::TextRange::new(
                        doc_tok.text_range().start() + php_syntax::TextSize::from(start as u32),
                        doc_tok.text_range().start()
                            + php_syntax::TextSize::from(line_end.trim_end_matches_len(text) as u32),
                    )
                })
                .unwrap_or(doc_tok.text_range());
            self.push(Diagnostic::new(
                range,
                Severity::Information,
                DiagnosticCode::DocTypeIncompatible,
                "Documented type is not compatible with the declared type.",
            ));
        }
    }
}

trait TrimLen {
    fn trim_end_matches_len(self, text: &str) -> usize;
}

impl TrimLen for usize {
    /// End offset without trailing whitespace / `*/`.
    fn trim_end_matches_len(self, text: &str) -> usize {
        let line = &text[..self];
        line.trim_end().trim_end_matches("*/").trim_end().len()
    }
}

/// `Closure` parameters accept closures and first-class callables.
fn is_closure_like(param: &Fqn, arg: &Type) -> bool {
    param.short().eq_ignore_ascii_case("Closure") && matches!(arg, Type::Closure(..))
}
