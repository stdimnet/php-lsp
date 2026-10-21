//! Argument type checks (Intelephense P1006), relaxed like Intelephense's
//! default `diagnostics.relaxedTypeCheck`: a union is accepted when one of
//! its members fits, scalars convert into each other (unless the file
//! declares `strict_types=1`), and unknown classes are always accepted.

use php_hir::{Fqn, Type};
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
            (Type::Array(..) | Type::List(_) | Type::Shape(_), a) => is_arrayish(a),
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

/// `Closure` parameters accept closures and first-class callables.
fn is_closure_like(param: &Fqn, arg: &Type) -> bool {
    param.short().eq_ignore_ascii_case("Closure") && matches!(arg, Type::Closure(..))
}
