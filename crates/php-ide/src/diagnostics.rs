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
    NoTypeInformation,
    UndefinedDocType,
    UnreachableCode,
    TooManyArguments,
    LoopVariableIsParameter,
    OverrideWithoutParent,
    MissingReturn,
    PossiblyUndefinedVariable,
    DuplicateSymbol,
    MultipleVisibility,
    FinalNotAllowed,
    PrivateAbstract,
    AbstractNotAllowed,
    InterfaceMethodNotPublic,
    MethodBodyExpected,
    MethodBodyNotAllowed,
    AbstractInNonAbstract,
    ThisInStatic,
    ParentWithoutParent,
    PromotionOutsideConstructor,
    PositionalAfterNamed,
    NamedArgumentOverwrites,
    UnknownNamedArgument,
    ReadonlyWithoutType,
    ReadonlyWithDefault,
    StaticReadonly,
    ImplicitlyNullable,
    MemberAccess,
    OptionalBeforeRequired,
    BreakOutsideLoop,
    ConstructorReturnType,
    VariadicDefault,
    DuplicateArrayKey,
    VariadicNotLast,
    StringConcatOperator,
    AssignmentInCondition,
    DuplicateBranch,
    VoidAssignment,
    UnknownParamName,
    TypeMismatch,
    DocTypeIncompatible,
    IncompatibleMethod,
    FinalOverride,
    /// `class A extends FinalClass`
    FinalExtend,
    NotAnAttribute,
    AttributeRepeated,
    CollidingTraitMethods,
    EnumCaseValue,
    EnumCaseUnexpectedValue,
    DuplicateEnumValue,
    EnumMagicMethod,
    EnumProperty,
    HookedReadonly,
    UndefinedConstant,
    UndefinedVariable,
    UndefinedMethod,
    UndefinedProperty,
    UndefinedClassConstant,
    AbstractInstantiation,
    MissingImplementation,
    /// A catalog code without a dedicated variant (`"P1084"`).
    Catalog(&'static str),
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::Catalog(code) => code,
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
            DiagnosticCode::AbstractInstantiation => "P1098",
            DiagnosticCode::MissingImplementation => "P1037",
            DiagnosticCode::NoTypeInformation => "P1132",
            DiagnosticCode::UndefinedDocType => "P1133",
            DiagnosticCode::UnreachableCode => "P1108",
            DiagnosticCode::TooManyArguments => "P1119",
            DiagnosticCode::LoopVariableIsParameter => "P1124",
            DiagnosticCode::OverrideWithoutParent => "P1128",
            DiagnosticCode::MissingReturn => "P1075",
            DiagnosticCode::PossiblyUndefinedVariable => "P1116",
            DiagnosticCode::DuplicateSymbol => "P1004",
            DiagnosticCode::MultipleVisibility => "P1015",
            DiagnosticCode::FinalNotAllowed => "P1016",
            DiagnosticCode::PrivateAbstract => "P1017",
            DiagnosticCode::AbstractNotAllowed => "P1018",
            DiagnosticCode::InterfaceMethodNotPublic => "P1019",
            DiagnosticCode::MethodBodyExpected => "P1020",
            DiagnosticCode::MethodBodyNotAllowed => "P1021",
            DiagnosticCode::AbstractInNonAbstract => "P1029",
            DiagnosticCode::ThisInStatic => "P1030",
            DiagnosticCode::ParentWithoutParent => "P1035",
            DiagnosticCode::PromotionOutsideConstructor => "P1041",
            DiagnosticCode::PositionalAfterNamed => "P1042",
            DiagnosticCode::NamedArgumentOverwrites => "P1043",
            DiagnosticCode::UnknownNamedArgument => "P1044",
            DiagnosticCode::ReadonlyWithoutType => "P1046",
            DiagnosticCode::ReadonlyWithDefault => "P1047",
            DiagnosticCode::StaticReadonly => "P1057",
            DiagnosticCode::ImplicitlyNullable => "P1078",
            DiagnosticCode::MemberAccess => "P1080",
            DiagnosticCode::OptionalBeforeRequired => "P1101",
            DiagnosticCode::BreakOutsideLoop => "P1104",
            DiagnosticCode::ConstructorReturnType => "P1109",
            DiagnosticCode::VariadicDefault => "P1113",
            DiagnosticCode::DuplicateArrayKey => "P1118",
            DiagnosticCode::VariadicNotLast => "P1120",
            DiagnosticCode::StringConcatOperator => "P1121",
            DiagnosticCode::AssignmentInCondition => "P1122",
            DiagnosticCode::DuplicateBranch => "P1123",
            DiagnosticCode::VoidAssignment => "P1125",
            DiagnosticCode::UnknownParamName => "P1130",
            DiagnosticCode::TypeMismatch => "P1006",
            DiagnosticCode::DocTypeIncompatible => "P1131",
            DiagnosticCode::IncompatibleMethod => "P1038",
            DiagnosticCode::FinalOverride => "P1039",
            DiagnosticCode::FinalExtend => "P1094",
            DiagnosticCode::NotAnAttribute => "P1086",
            DiagnosticCode::AttributeRepeated => "P1088",
            DiagnosticCode::CollidingTraitMethods => "P1093",
            DiagnosticCode::EnumCaseValue => "P1050",
            DiagnosticCode::EnumCaseUnexpectedValue => "P1052",
            DiagnosticCode::DuplicateEnumValue => "P1103",
            DiagnosticCode::EnumMagicMethod => "P1096",
            DiagnosticCode::EnumProperty => "P1097",
            DiagnosticCode::HookedReadonly => "P1079",
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
    pub(crate) fn new(
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
    /// `diagnostics.typeErrors` (also covers "no type information").
    pub type_errors: bool,
    /// `diagnostics.unreachableCode`.
    pub unreachable_code: bool,
    /// `diagnostics.languageConstraints`: modifiers, bodies, `$this`, ...
    pub language_constraints: bool,
    /// `diagnostics.memberAccess`: private / protected members.
    pub member_access: bool,
    /// `diagnostics.suspectCode`: duplicate keys, assignments in conditions, ...
    pub suspect_code: bool,
    /// `diagnostics.duplicateSymbols`.
    pub duplicate_symbols: bool,
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
            type_errors: true,
            unreachable_code: true,
            language_constraints: true,
            member_access: true,
            suspect_code: true,
            duplicate_symbols: true,
        }
    }
}

pub(crate) fn syntax_diagnostics(config: &Config, parse: &php_syntax::Parse) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if config.diagnostics.enable && config.diagnostics.syntax {
        for e in parse.errors() {
            out.push(match e.required {
                Some(v) => Diagnostic::new(
                    e.range,
                    Severity::Error,
                    DiagnosticCode::Catalog("P1022"),
                    format!("PHP {v}+ syntax found. Targeting PHP {}.", config.php_version),
                ),
                None => {
                    Diagnostic::new(e.range, Severity::Error, DiagnosticCode::SyntaxError, e.message.clone())
                }
            });
        }
    }
    out
}

pub(crate) fn semantic_diagnostics(sema: &Sema) -> Vec<Diagnostic> {
    let config = &sema.ws.config().diagnostics;
    if !config.enable {
        return Vec::new();
    }
    let blade = sema
        .ws
        .file(sema.file)
        .filter(|f| crate::blade::is_blade(&f.path))
        .map(|f| crate::blade::virtual_php(&f.text));
    let mut cx = Checker { sema, config, out: Vec::new(), blade };
    cx.run();
    let mut out = cx.out;
    out.sort_by_key(|d| (d.range.start(), d.range.end()));
    out.dedup_by(|a, b| a.range == b.range && a.code == b.code);
    out
}

/// The parameter declaring the variable at `var` (without attributes).
fn param_range(root: &SyntaxNode, var: TextRange) -> TextRange {
    let Some(param) = root.covering_element(var).ancestors().find(|n| n.kind() == PARAM) else { return var };
    let start = param
        .children_with_tokens()
        .find(|e| !e.kind().is_trivia() && e.kind() != ATTRIBUTE_GROUP)
        .map_or(var.start(), |e| e.text_range().start());
    TextRange::new(start, param.text_range().end())
}

fn is_statement(kind: php_syntax::SyntaxKind) -> bool {
    format!("{kind:?}").ends_with("_STMT")
        || matches!(
            kind,
            BLOCK | FUNCTION_DECL | CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | CONST_DECL
        )
}

/// `return`, `throw`, `exit()`, `break`, `continue` (as a whole statement).
fn terminates(stmt: &SyntaxNode) -> bool {
    match stmt.kind() {
        RETURN_STMT | BREAK_STMT | CONTINUE_STMT | THROW_STMT | GOTO_STMT => true,
        EXPR_STMT => stmt.children().next().is_some_and(|e| matches!(e.kind(), THROW_EXPR | EXIT_EXPR)),
        _ => false,
    }
}

/// `$x->prop ?? default`: undefined properties are fine on the left of `??`.
fn coalesced(node: &SyntaxNode) -> bool {
    let mut cur = node.clone();
    while let Some(p) = cur.parent() {
        match p.kind() {
            BIN_EXPR if p.children_with_tokens().any(|e| e.kind() == QUESTION2) => {
                return p.first_child().as_ref() == Some(&cur);
            }
            PROPERTY_FETCH_EXPR | INDEX_EXPR | PAREN_EXPR => cur = p,
            _ => return false,
        }
    }
    false
}

/// A class reference only evaluated after `class_exists(Foo::class)` (or
/// `interface_exists` / `enum_exists` / `trait_exists`).
fn class_guarded(node: &SyntaxNode, fqn: &Fqn) -> bool {
    const CHECKS: [&str; 4] = ["class_exists", "interface_exists", "enum_exists", "trait_exists"];
    // the argument of the check itself
    let in_check =
        node.ancestors().find(|a| a.kind() == CALL_EXPR).and_then(|c| c.first_child()).is_some_and(
            |callee| {
                let name = ast::text_without_trivia(&callee);
                CHECKS.iter().any(|c| name.trim_start_matches('\\').eq_ignore_ascii_case(c))
            },
        );
    if in_check {
        return true;
    }
    let short = fqn.short().to_string();
    let names = [format!("{short}::class"), format!("'{}'", fqn.as_str()), format!("'{short}'")];
    node.ancestors().skip(1).any(|anc| {
        let cond = match anc.kind() {
            IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT | BIN_EXPR | TERNARY_EXPR => {
                anc.children().find(|c| crate::infer::is_expr(c.kind()))
            }
            _ => None,
        };
        cond.is_some_and(|c| {
            let text = c.text().to_string();
            !c.text_range().contains_range(node.text_range())
                && CHECKS.iter().any(|f| text.contains(f))
                && names.iter().any(|n| text.contains(n.as_str()))
        })
    })
}

/// Whether the definition at `def` is executed on every path to `read`:
/// none of the conditionally executed parts it is in (if / else branches,
/// loop bodies, switch cases, catch blocks, `&&` / `||` / `??` / `?:`
/// operands, match arms) leaves out `read`. An `if` with `else` where every
/// branch defines the variable counts as a definition.
fn def_reaches(
    sema: &Sema,
    scope: &SyntaxNode,
    name: &str,
    def: TextSize,
    read: TextSize,
    defs: &[TextSize],
) -> bool {
    let Some(tok) = scope.token_at_offset(def).right_biased() else { return true };
    let mut prev: Option<SyntaxNode> = None;
    for anc in tok.parent_ancestors() {
        if &anc == scope {
            return true;
        }
        if matches!(anc.kind(), CLOSURE_EXPR | ARROW_FN_EXPR) {
            return true;
        }
        let child = prev.replace(anc.clone());
        let Some(child) = child else { continue };
        let cond_child = || anc.children().find(|c| crate::infer::is_expr(c.kind())).as_ref() == Some(&child);
        let conditional = match anc.kind() {
            // the branch taken depends on the condition
            IF_STMT => !cond_child(),
            ELSE_IF_CLAUSE => !cond_child(),
            // decided at the `if` (with the else clause as branch)
            ELSE_CLAUSE => false,
            CASE_CLAUSE | DEFAULT_CLAUSE | CATCH_CLAUSE | MATCH_ARM => true,
            // the body only (`for` initializers and the first condition always run)
            // `while (true)` runs its body at least once
            WHILE_STMT => !cond_child() && !always_true(&anc),
            FOREACH_STMT => !cond_child() && child.kind() != FOREACH_VALUE && child.kind() != FOREACH_KEY,
            FOR_STMT => is_statement(child.kind()) || child.kind() == STMT_LIST,
            BIN_EXPR => {
                let short_circuit = anc
                    .children_with_tokens()
                    .any(|e| matches!(e.kind(), AMP2 | PIPE2 | QUESTION2 | AND_KW | OR_KW));
                short_circuit
                    && anc.first_child().as_ref() != Some(&child)
                    && !short_circuit_reaches(sema, &anc, read)
            }
            TERNARY_EXPR => anc.first_child().as_ref() != Some(&child),
            _ => false,
        };
        if !conditional || anc.text_range().contains(read) {
            continue;
        }
        // `if (empty($x)) { $x = ...; }`: set either way afterwards
        if matches!(anc.kind(), IF_STMT | ELSE_IF_CLAUSE) && fills_if_missing(&anc, name) {
            continue;
        }
        // `switch` with `default` where every clause defines it (or falls through / exits)
        if matches!(anc.kind(), CASE_CLAUSE | DEFAULT_CLAUSE) && switch_defines(sema, &anc, defs, read) {
            continue;
        }
        // `if (...) { $x = 1; } else { $x = 2; }`
        let if_stmt = anc.ancestors().find(|a| a.kind() == IF_STMT);
        if let Some(if_stmt) = if_stmt.filter(|i| {
            matches!(anc.kind(), IF_STMT | ELSE_IF_CLAUSE | ELSE_CLAUSE) && !i.text_range().contains(read)
        }) {
            if exhaustive_if_defines(sema, &if_stmt, defs) || correlated_condition(&anc, &child, read) {
                // defined either way; outer conditions still matter
                continue;
            }
        }
        return false;
    }
    true
}

/// The condition of a `while` is `true` / `1`.
fn always_true(stmt: &SyntaxNode) -> bool {
    stmt.children().find(|c| crate::infer::is_expr(c.kind())).is_some_and(|c| {
        let t = ast::text_without_trivia(&c);
        t.eq_ignore_ascii_case("true") || t == "1"
    })
}

/// The `switch` around `clause` has a `default` and each clause defines the
/// variable (at one of `defs`), falls through (empty) or always exits.
fn switch_defines(sema: &Sema, clause: &SyntaxNode, defs: &[TextSize], read: TextSize) -> bool {
    let Some(switch) = clause.ancestors().find(|a| a.kind() == SWITCH_STMT) else { return false };
    if switch.text_range().contains(read) {
        return false;
    }
    let clauses: Vec<SyntaxNode> = switch
        .descendants()
        .filter(|n| matches!(n.kind(), CASE_CLAUSE | DEFAULT_CLAUSE))
        .filter(|c| c.ancestors().find(|a| a.kind() == SWITCH_STMT).as_ref() == Some(&switch))
        .collect();
    clauses.iter().any(|c| c.kind() == DEFAULT_CLAUSE)
        && clauses.iter().all(|c| {
            let stmts: Vec<SyntaxNode> = c.children().filter(|s| is_statement(s.kind())).collect();
            stmts.is_empty()
                || defs.iter().any(|d| c.text_range().contains(*d))
                || stmts.iter().any(|s| always_exits(sema, s))
        })
}

/// A read inside `if (isset($x) ...) { ... }` / `!empty($x)` / `$x ?? ...`.
fn isset_guarded(scope: &SyntaxNode, read: TextRange, name: &str) -> bool {
    let Some(tok) = scope.token_at_offset(read.start()).right_biased() else { return false };
    let checks = [format!("isset(${name}"), format!("empty(${name}"), format!("${name}??")];
    tok.parent_ancestors().take_while(|a| a != scope).any(|a| {
        let cond = match a.kind() {
            IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT | TERNARY_EXPR | BIN_EXPR => {
                a.children().find(|c| crate::infer::is_expr(c.kind()))
            }
            _ => None,
        };
        cond.is_some_and(|c| {
            let t = ast::text_without_trivia(&c);
            !c.text_range().contains_range(read) && checks.iter().any(|k| t.contains(k.as_str()))
        })
    })
}

/// `if (a && $x = f()) { ...$x... }`: in the "then" branch every operand
/// The condition of `if_stmt` tests that `$name` is missing: `empty($x)`,
/// `!isset($x)`, `is_null($x)`, `$x === null`, `!$x`.
fn fills_if_missing(if_stmt: &SyntaxNode, name: &str) -> bool {
    let Some(cond) = if_stmt.children().find(|c| crate::infer::is_expr(c.kind())) else { return false };
    let t = ast::text_without_trivia(&cond).to_ascii_lowercase();
    let v = format!("${}", name.to_ascii_lowercase());
    [
        format!("empty({v})"),
        format!("!isset({v})"),
        format!("is_null({v})"),
        format!("{v}===null"),
        format!("null==={v}"),
        format!("{v}==null"),
        format!("!{v}"),
    ]
    .iter()
    .any(|c| t == *c || t == format!("({c})"))
}

fn is_and_op(n: &SyntaxNode) -> bool {
    n.kind() == BIN_EXPR && n.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW))
}

fn is_or_op(n: &SyntaxNode) -> bool {
    n.kind() == BIN_EXPR && n.children_with_tokens().any(|e| matches!(e.kind(), PIPE2 | OR_KW))
}

/// The right operand of `bin` (`&&` / `||`, containing a definition) is
/// evaluated whenever `read` executes. The value `bin` must have for that
/// (true for `&&`, false for `||`) is followed through `!`, parentheses and
/// enclosing `&&` / `||` up to an `if` / `while` / `?:` condition:
/// `if (a && $x = f()) { $x }`, `if (!(a && is_string($x = f()))) { return; } $x`,
/// `a && ($x = f()) && g($x)`.
fn short_circuit_reaches(sema: &Sema, bin: &SyntaxNode, read: TextSize) -> bool {
    let mut need = is_and_op(bin);
    let mut cur = bin.clone();
    loop {
        let Some(parent) = cur.parent() else { return false };
        let later_operand = parent.first_child().as_ref() == Some(&cur)
            && parent.text_range().contains(read)
            && !cur.text_range().contains(read);
        match parent.kind() {
            PAREN_EXPR => {}
            PREFIX_EXPR if parent.children_with_tokens().any(|e| e.kind() == BANG) => need = !need,
            BIN_EXPR if is_and_op(&parent) => {
                if later_operand && need {
                    return true;
                }
                if !need {
                    return false;
                }
            }
            BIN_EXPR if is_or_op(&parent) => {
                if later_operand && !need {
                    return true;
                }
                if need {
                    return false;
                }
            }
            IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT => {
                let cond = parent.children().find(|c| crate::infer::is_expr(c.kind()));
                if cond.as_ref() != Some(&cur) {
                    return false;
                }
                let body = parent
                    .children()
                    .find(|c| c != &cur && (is_statement(c.kind()) || c.kind() == STMT_LIST));
                if need {
                    return body.is_some_and(|b| b.text_range().contains(read));
                }
                // condition false: after an `if` whose body never completes
                return parent.kind() == IF_STMT
                    && !parent.children().any(|c| matches!(c.kind(), ELSE_CLAUSE | ELSE_IF_CLAUSE))
                    && read >= parent.text_range().end()
                    && body.is_some_and(|b| always_exits(sema, &b));
            }
            TERNARY_EXPR => {
                let parts: Vec<SyntaxNode> =
                    parent.children().filter(|c| crate::infer::is_expr(c.kind())).collect();
                if parts.len() != 3 || parts[0] != cur {
                    return false;
                }
                return parts[if need { 1 } else { 2 }].text_range().contains(read);
            }
            _ => return false,
        }
        cur = parent;
    }
}

/// Every branch of the `if` (which needs an `else`) defines the variable or
/// never completes.
fn exhaustive_if_defines(sema: &Sema, if_stmt: &SyntaxNode, defs: &[TextSize]) -> bool {
    let body = |n: &SyntaxNode| n.children().find(|c| is_statement(c.kind()) || c.kind() == STMT_LIST);
    let defines = |n: &SyntaxNode| defs.iter().any(|d| n.text_range().contains(*d));
    let covers = |b: Option<SyntaxNode>| b.is_some_and(|b| defines(&b) || always_exits(sema, &b));
    if !covers(body(if_stmt)) {
        return false;
    }
    for clause in if_stmt.children().filter(|c| c.kind() == ELSE_IF_CLAUSE) {
        // `elseif (false === $x = f())`: every later path has `$x`
        if clause.children().find(|c| crate::infer::is_expr(c.kind())).is_some_and(|c| defines(&c)) {
            return true;
        }
        if !covers(body(&clause)) {
            return false;
        }
    }
    let Some(else_clause) = if_stmt.children().find(|c| c.kind() == ELSE_CLAUSE) else { return false };
    covers(body(&else_clause))
}

/// `if (C) { $x = ...; } ... if (C) { ...$x... }`: the same condition guards
/// definition and use.
fn correlated_condition(branch_owner: &SyntaxNode, branch: &SyntaxNode, read: TextSize) -> bool {
    if !matches!(branch_owner.kind(), IF_STMT | ELSE_IF_CLAUSE) {
        return false;
    }
    let cond = |n: &SyntaxNode| n.children().find(|c| crate::infer::is_expr(c.kind()));
    let Some(c) = cond(branch_owner) else { return false };
    if &c == branch {
        return false;
    }
    let text = ast::text_without_trivia(&c);
    let Some(tok) =
        branch_owner.ancestors().last().and_then(|root| root.token_at_offset(read).right_biased())
    else {
        return false;
    };
    // defined in the `else` of `if (C)`: used in the `else` of another `if (C)`
    if branch.kind() == ELSE_CLAUSE {
        return tok.parent_ancestors().any(|a| {
            a.kind() == IF_STMT
                && &a != branch_owner
                && cond(&a).is_some_and(|ac| ast::text_without_trivia(&ac) == text)
                && !a.children().any(|c| c.kind() == ELSE_IF_CLAUSE)
                && a.children()
                    .find(|c| c.kind() == ELSE_CLAUSE)
                    .is_some_and(|e| e.text_range().contains(read))
        });
    }
    tok.parent_ancestors().any(|a| match a.kind() {
        IF_STMT | ELSE_IF_CLAUSE => {
            cond(&a)
                .is_some_and(|ac| ast::text_without_trivia(&ac) == text && !ac.text_range().contains(read))
                && a.children()
                    .find(|c| is_statement(c.kind()) || c.kind() == STMT_LIST)
                    .is_some_and(|b| b.text_range().contains(read))
        }
        // `C ? ...$x... : ...`
        TERNARY_EXPR => {
            let parts: Vec<SyntaxNode> = a.children().filter(|c| crate::infer::is_expr(c.kind())).collect();
            parts.len() == 3
                && ast::text_without_trivia(&parts[0]) == text
                && parts[1].text_range().contains(read)
        }
        // `C && ...$x...`
        BIN_EXPR if a.children_with_tokens().any(|e| matches!(e.kind(), AMP2 | AND_KW)) => {
            let parts: Vec<SyntaxNode> = a.children().filter(|c| crate::infer::is_expr(c.kind())).collect();
            parts.len() == 2
                && ast::text_without_trivia(&parts[0]) == text
                && parts[1].text_range().contains(read)
        }
        _ => false,
    })
}

/// Whether control never reaches the end of `stmt` (return, throw, exit,
/// calls returning `never`, exhaustive if / switch / try, infinite loops).
fn always_exits(sema: &Sema, stmt: &SyntaxNode) -> bool {
    let body_of =
        |n: &SyntaxNode| n.children().find(|c| is_statement(c.kind()) || matches!(c.kind(), STMT_LIST));
    match stmt.kind() {
        RETURN_STMT | THROW_STMT | GOTO_STMT => true,
        EXPR_STMT => stmt.children().next().is_some_and(|e| match e.kind() {
            THROW_EXPR | EXIT_EXPR => true,
            CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
                matches!(sema.infer().expr_type(&e), Type::Never)
            }
            _ => false,
        }),
        BLOCK | STMT_LIST => {
            stmt.children().filter(|c| is_statement(c.kind())).any(|c| always_exits(sema, &c))
        }
        IF_STMT => {
            let Some(else_clause) = stmt.children().find(|c| c.kind() == ELSE_CLAUSE) else { return false };
            body_of(stmt).is_some_and(|b| always_exits(sema, &b))
                && stmt
                    .children()
                    .filter(|c| c.kind() == ELSE_IF_CLAUSE)
                    .all(|c| body_of(&c).is_some_and(|b| always_exits(sema, &b)))
                && body_of(&else_clause).is_some_and(|b| always_exits(sema, &b))
        }
        SWITCH_STMT => {
            let clauses: Vec<SyntaxNode> = stmt
                .descendants()
                .filter(|n| matches!(n.kind(), CASE_CLAUSE | DEFAULT_CLAUSE))
                .filter(|c| c.ancestors().find(|a| a.kind() == SWITCH_STMT).as_ref() == Some(stmt))
                .collect();
            if !clauses.iter().any(|c| c.kind() == DEFAULT_CLAUSE) {
                return false;
            }
            // every clause exits before a `break` (empty clauses fall through)
            clauses.iter().all(|c| {
                let stmts: Vec<SyntaxNode> = c.children().filter(|s| is_statement(s.kind())).collect();
                if stmts.is_empty() {
                    return true;
                }
                for s in &stmts {
                    if s.kind() == BREAK_STMT {
                        return false;
                    }
                    if always_exits(sema, s) {
                        return true;
                    }
                }
                false
            })
        }
        TRY_STMT => {
            let finally = stmt
                .children()
                .find(|c| c.kind() == FINALLY_CLAUSE)
                .and_then(|f| f.children().find(|c| c.kind() == BLOCK));
            if finally.is_some_and(|f| always_exits(sema, &f)) {
                return true;
            }
            stmt.children().find(|c| c.kind() == BLOCK).is_some_and(|b| always_exits(sema, &b))
                && stmt
                    .children()
                    .filter(|c| c.kind() == CATCH_CLAUSE)
                    .all(|c| c.children().find(|b| b.kind() == BLOCK).is_some_and(|b| always_exits(sema, &b)))
        }
        // `while (true)` / `for (;;)` / `do { } while (true)` without `break`
        WHILE_STMT | DO_WHILE_STMT | FOR_STMT => {
            let text = stmt.text().to_string();
            let infinite = match stmt.kind() {
                FOR_STMT => text.replace(char::is_whitespace, "").starts_with("for(;;)"),
                _ => {
                    let cond = stmt.children().find(|c| crate::infer::is_expr(c.kind()));
                    cond.is_some_and(|c| {
                        ast::text_without_trivia(&c).eq_ignore_ascii_case("true")
                            || ast::text_without_trivia(&c) == "1"
                    })
                }
            };
            // a `break` of this loop (not of a nested loop / switch)
            infinite
                && !stmt.descendants().filter(|n| n.kind() == BREAK_STMT).any(|b| {
                    // `break N` leaves the N-th enclosing loop / switch
                    let levels = b
                        .children()
                        .find(|c| crate::infer::is_expr(c.kind()))
                        .map_or(Some(1), |n| ast::text_without_trivia(&n).parse::<usize>().ok());
                    let Some(levels) = levels.filter(|l| *l >= 1) else { return true };
                    b.ancestors()
                        .skip(1)
                        .filter(|a| {
                            matches!(
                                a.kind(),
                                WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT | SWITCH_STMT
                            )
                        })
                        .nth(levels - 1)
                        .as_ref()
                        == Some(stmt)
                })
        }
        _ => false,
    }
}

/// Whether `node` is only evaluated after a check like
/// `method_exists($x, 'name')` (in an `if` / `&&` / `?:` condition).
fn guarded_by(node: &SyntaxNode, checks: &[&str], name: &str) -> bool {
    let quoted = [format!("'{name}'"), format!("\"{name}\"")];
    let mentions = |cond: &SyntaxNode| {
        let text = cond.text().to_string();
        checks.iter().any(|c| text.contains(c)) && quoted.iter().any(|q| text.contains(q.as_str()))
    };
    node.ancestors().skip(1).any(|anc| {
        let cond = match anc.kind() {
            IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT => anc.children().find(|c| crate::infer::is_expr(c.kind())),
            BIN_EXPR | TERNARY_EXPR => anc.children().find(|c| crate::infer::is_expr(c.kind())),
            _ => None,
        };
        cond.is_some_and(|c| !c.text_range().contains_range(node.text_range()) && mentions(&c))
    })
}

const BUILTIN_TYPES: &[&str] = &[
    "int", "integer", "float", "double", "string", "bool", "boolean", "array", "callable", "iterable",
    "object", "mixed", "void", "null", "never", "false", "true", "resource", "self", "static", "parent",
];

pub(crate) struct Checker<'s, 'a> {
    pub(crate) sema: &'s Sema<'a>,
    pub(crate) config: &'s DiagnosticsConfig,
    pub(crate) out: Vec<Diagnostic>,
    /// Set for Blade templates.
    pub(crate) blade: Option<crate::blade::Blade>,
}

impl Checker<'_, '_> {
    pub(crate) fn push(&mut self, d: Diagnostic) {
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
                self.check_function_body(&node, index_ready);
            }
            if self.blade.is_none() {
                self.lint(&node, index_ready);
            }
            if node.kind() == ARROW_FN_EXPR && self.config.unused_symbols && self.blade.is_none() {
                self.check_arrow_params(&node);
            }
            if matches!(node.kind(), BLOCK | CASE_CLAUSE)
                && self.config.unreachable_code
                && self.blade.is_none()
            {
                self.check_unreachable(&node);
            }
            if index_ready && matches!(node.kind(), FUNCTION_DECL | METHOD_DECL | PROPERTY_DECL) {
                self.check_type_information(&node);
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

    /// Parameters / properties without declared, documented or inferable
    /// type (Intelephense P1132).
    fn check_type_information(&mut self, node: &SyntaxNode) {
        if !self.config.type_errors || self.blade.is_some() {
            return;
        }
        // members of anonymous classes usually implement an interface
        let class_like = node
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS));
        if class_like.is_some_and(|c| c.kind() == ANON_CLASS) {
            return;
        }
        let doc = ast::doc_comment(node).map(|d| php_hir::DocBlock::parse(d.text()));
        let documented = |name: &str| {
            doc.as_ref().is_some_and(|d| {
                d.param(name).is_some_and(|t| !t.type_text.is_empty())
                    || d.var(Some(name)).or_else(|| d.var(None)).is_some_and(|t| !t.type_text.is_empty())
            })
        };
        if node.kind() == PROPERTY_DECL {
            let Some(decl) = ast::PropertyDecl::cast(node.clone()) else { return };
            // asymmetric visibility without a type is P1067
            let asymmetric = node
                .children()
                .filter(|c| c.kind() == MODIFIER_LIST)
                .any(|m| m.children_with_tokens().any(|e| e.kind() == L_PAREN));
            if decl.ty().is_some() || asymmetric {
                return;
            }
            for el in decl.elements() {
                let (Some(var), Some(name)) = (el.variable(), el.name()) else { continue };
                let has_default = el.syntax().children_with_tokens().any(|e| e.kind() == EQ);
                if has_default || documented(&name) || self.inherited_member_type(node, &name, None) {
                    continue;
                }
                self.push(Diagnostic::new(
                    var.text_range(),
                    Severity::Information,
                    DiagnosticCode::NoTypeInformation,
                    format!("Property ${name} has no type information available."),
                ));
            }
            return;
        }
        let Some(params) = node.children().find_map(ast::ParamList::cast) else { return };
        // magic methods have fixed signatures
        let magic = node.kind() == METHOD_DECL
            && node.children().find(|c| c.kind() == MEMBER_NAME).is_some_and(|n| {
                let n = ast::text_without_trivia(&n);
                n.starts_with("__") && !n.eq_ignore_ascii_case("__construct")
            });
        if magic {
            return;
        }
        for p in params.params() {
            let (Some(var), Some(name)) = (p.variable(), p.name()) else { continue };
            if p.ty().is_some() || p.default_value().is_some() || documented(&name) {
                continue;
            }
            if node.kind() == METHOD_DECL && self.inherited_member_type(node, &name, Some(node)) {
                continue;
            }
            self.push(Diagnostic::new(
                var.text_range(),
                Severity::Information,
                DiagnosticCode::NoTypeInformation,
                format!("Parameter ${name} has no type information available."),
            ));
        }
    }

    /// Loop variables shadowing parameters (P1124), `#[\Override]` without a
    /// parent method (P1128) and missing returns (P1075).
    fn check_function_body(&mut self, func: &SyntaxNode, index_ready: bool) {
        let params: Vec<String> = func
            .children()
            .find_map(ast::ParamList::cast)
            .map(|pl| pl.params().filter_map(|p| p.name()).collect())
            .unwrap_or_default();
        let own = |n: &SyntaxNode| {
            n.ancestors()
                .find(|a| matches!(a.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR))
                .as_ref()
                == Some(func)
        };
        if !params.is_empty() {
            for binding in func.descendants().filter(|n| matches!(n.kind(), FOREACH_VALUE | FOREACH_KEY)) {
                if !own(&binding) {
                    continue;
                }
                for tok in binding
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| t.kind() == VARIABLE)
                {
                    let name = &tok.text()[1..];
                    if params.iter().any(|p| p == name) {
                        self.push(Diagnostic::new(
                            tok.text_range(),
                            Severity::Information,
                            DiagnosticCode::LoopVariableIsParameter,
                            format!("Loop variable ${name} is also declared as a parameter."),
                        ));
                    }
                }
            }
        }
        if index_ready && func.kind() == METHOD_DECL && self.config.implementation_errors {
            self.check_override(func);
        }
        self.check_missing_return(func, &own);
    }

    /// Unused parameters of `fn (...) => ...`.
    fn check_arrow_params(&mut self, arrow: &SyntaxNode) {
        let Some(params) = arrow.children().find_map(ast::ParamList::cast) else { return };
        let body: Vec<SyntaxNode> =
            arrow.children().filter(|c| c.kind() != PARAM_LIST && crate::infer::is_expr(c.kind())).collect();
        for p in params.params() {
            let (Some(var), Some(name)) = (p.variable(), p.name()) else { continue };
            if p.by_ref() {
                continue;
            }
            let used = body.iter().any(|b| {
                b.descendants_with_tokens().filter_map(|e| e.into_token()).any(|t| {
                    t.kind() == VARIABLE && t.text()[1..] == name
                        || t.kind() == STRING && t.text().contains(&format!("${name}"))
                })
            });
            if !used {
                let mut d = Diagnostic::new(
                    param_range(&self.sema.root, var.text_range()),
                    Severity::Hint,
                    DiagnosticCode::UnusedSymbol,
                    format!("Symbol '${name}' is declared but not used."),
                );
                d.unnecessary = true;
                self.push(d);
            }
        }
    }

    fn check_override(&mut self, method: &SyntaxNode) {
        let has_override = method
            .children()
            .filter(|c| c.kind() == ATTRIBUTE_GROUP)
            .flat_map(|g| g.descendants().filter(|n| n.kind() == ATTRIBUTE).collect::<Vec<_>>())
            .filter_map(|a| a.children().find(|c| c.kind() == QUALIFIED_NAME))
            .any(|q| {
                self.sema
                    .resolve_class_name(&ast::text_without_trivia(&q), &q)
                    .is_some_and(|f| f.as_str().eq_ignore_ascii_case("Override"))
            });
        if !has_override {
            return;
        }
        let Some(class) = method
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | ENUM_DECL | ANON_CLASS | INTERFACE_DECL | TRAIT_DECL))
        else {
            return;
        };
        if class.kind() == TRAIT_DECL {
            return;
        }
        let (Some(fqn), Some(name)) =
            (self.sema.class_fqn(&class), method.children().find_map(ast::MemberName::cast))
        else {
            return;
        };
        let Some(mname) = name.text() else { return };
        let index = self.sema.index();
        if index.has_unknown_ancestor(&fqn) {
            return;
        }
        // traits used by the class itself do not count, those of parents do
        let own_traits: Vec<Fqn> = index.class(&fqn).map(|(_, c)| c.traits.clone()).unwrap_or_default();
        let parent_has = index
            .ancestors(&fqn)
            .into_iter()
            .filter(|(_, c)| c.fqn != fqn && !own_traits.contains(&c.fqn))
            .any(|(_, c)| c.method(&mname).is_some());
        if !parent_has {
            self.push(Diagnostic::new(
                name.syntax().text_range(),
                Severity::Error,
                DiagnosticCode::OverrideWithoutParent,
                format!(
                    "{}::{mname}() has #[\\Override] attribute, but no matching parent method exists.",
                    fqn.short()
                ),
            ));
        }
    }

    /// A declared return type (other than void / never / mixed / null) but a
    /// body whose end is reachable.
    fn check_missing_return(&mut self, func: &SyntaxNode, own: &dyn Fn(&SyntaxNode) -> bool) {
        let Some(ret) = func.children().find(|c| c.kind() == RETURN_TYPE) else { return };
        let Some(body) = func.children().find(|c| c.kind() == BLOCK) else { return };
        let ty = ast::text_without_trivia(&ret).trim_start_matches(':').to_ascii_lowercase();
        if matches!(ty.as_str(), "void" | "never" | "mixed" | "null" | "?mixed") {
            return;
        }
        // generators
        if body.descendants().any(|n| matches!(n.kind(), YIELD_EXPR | YIELD_FROM_EXPR) && own(&n)) {
            return;
        }
        if always_exits(self.sema, &body) {
            return;
        }
        // a trailing call we cannot resolve may not return (`$this->fail()`
        // without PHPUnit installed)
        let last = body.children().filter(|c| is_statement(c.kind())).last();
        if let Some(call) = last.filter(|l| l.kind() == EXPR_STMT).and_then(|l| l.children().next()) {
            if matches!(call.kind(), CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR)
                && self.unresolved_call(&call)
            {
                return;
            }
        }
        let range = ret.children().last().map_or(ret.text_range(), |t| t.text_range());
        self.push(Diagnostic::new(
            range,
            Severity::Error,
            DiagnosticCode::MissingReturn,
            "Not all paths return a value.".to_string(),
        ));
    }

    fn unresolved_call(&self, call: &SyntaxNode) -> bool {
        match call.kind() {
            CALL_EXPR => call.first_child().filter(|c| c.kind() == NAME_EXPR).is_none_or(|c| {
                self.sema.resolve_function(&ast::text_without_trivia(&c), c.text_range().start()).is_none()
            }),
            _ => {
                let Some(name) = call.children().find_map(ast::MemberName::cast).and_then(|m| m.text())
                else {
                    return true;
                };
                let Some(lhs) = call.first_child() else { return true };
                let classes = if call.kind() == STATIC_CALL_EXPR {
                    crate::classify::static_lhs_classes(self.sema, &lhs)
                } else {
                    self.sema.infer().expr_type(&lhs).class_names()
                };
                let index = self.sema.index();
                classes.is_empty()
                    || classes
                        .iter()
                        .any(|c| index.has_unknown_ancestor(c) || index.find_method(c, &name).is_none())
            }
        }
    }

    /// Statements after `return` / `throw` / `exit` / `break` / `continue`
    /// in the same block.
    fn check_unreachable(&mut self, block: &SyntaxNode) {
        let stmts: Vec<SyntaxNode> = block.children().filter(|c| is_statement(c.kind())).collect();
        let Some(pos) = stmts.iter().position(terminates) else { return };
        // declarations are hoisted, labels can be jumped to
        let rest: Vec<&SyntaxNode> = stmts[pos + 1..]
            .iter()
            .take_while(|s| s.kind() != LABEL_STMT)
            .filter(|s| {
                !matches!(s.kind(), FUNCTION_DECL | CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL)
            })
            .collect();
        let (Some(first), Some(last)) = (rest.first(), rest.last()) else { return };
        let mut d = Diagnostic::new(
            TextRange::new(first.text_range().start(), last.text_range().end()),
            Severity::Hint,
            DiagnosticCode::UnreachableCode,
            "Unreachable code detected.".to_string(),
        );
        d.unnecessary = true;
        self.push(d);
    }

    /// Methods whose parameters are given by an ancestor (overrides,
    /// implementations) or that have no body.
    fn signature_fixed(&self, scope: &SyntaxNode) -> bool {
        if scope.kind() != METHOD_DECL {
            return false;
        }
        if !scope.children().any(|c| c.kind() == BLOCK) {
            return true;
        }
        let Some(class) = scope
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS | INTERFACE_DECL))
        else {
            return true;
        };
        if matches!(class.kind(), TRAIT_DECL | INTERFACE_DECL) {
            return true;
        }
        let Some(fqn) = self.sema.class_fqn(&class) else { return true };
        let Some(name) = scope.children().find_map(ast::MemberName::cast).and_then(|m| m.text()) else {
            return true;
        };
        if name.starts_with("__") {
            return true;
        }
        let index = self.sema.index();
        if index.has_unknown_ancestor(&fqn) {
            return true;
        }
        index
            .ancestors(&fqn)
            .into_iter()
            .filter(|(_, c)| c.fqn != fqn)
            .any(|(_, c)| c.method(&name).is_some())
    }

    /// Whether an ancestor declares a type for property `name` or (with
    /// `method`) for the parameter `name` of the overridden method.
    fn inherited_member_type(&self, node: &SyntaxNode, name: &str, method: Option<&SyntaxNode>) -> bool {
        let Some(class) = node
            .ancestors()
            .find(|n| matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL))
        else {
            return false;
        };
        let Some(fqn) = self.sema.class_fqn(&class) else { return false };
        let method_name =
            method.and_then(|m| m.children().find_map(ast::MemberName::cast)).and_then(|m| m.text());
        let index = self.sema.index();
        index.ancestors(&fqn).into_iter().filter(|(_, c)| c.fqn != fqn).any(|(_, c)| match &method_name {
            Some(m) => c
                .method(m)
                .and_then(|f| f.params.iter().find(|p| p.name == name))
                .is_some_and(|p| p.ty.is_some() || p.doc_ty.is_some()),
            None => c.properties.iter().any(|p| p.name == name && p.effective_type().is_some()),
        })
    }

    /// `use Deprecated\Class;` is reported like Intelephense.
    fn check_import_deprecation(&mut self, qn: &SyntaxNode, clause: &SyntaxNode) {
        let decl = clause.ancestors().find(|a| a.kind() == USE_DECL);
        let non_class = decl
            .is_some_and(|d| d.children_with_tokens().any(|e| matches!(e.kind(), FUNCTION_KW | CONST_KW)));
        if non_class || clause.first_child().as_ref() != Some(qn) {
            return;
        }
        let mut name = ast::text_without_trivia(qn).trim_start_matches('\\').to_string();
        if let Some(group) = clause.parent().filter(|p| p.kind() == USE_GROUP) {
            if let Some(prefix) =
                group.parent().and_then(|g| g.children().find(|c| c.kind() == QUALIFIED_NAME))
            {
                name = format!(
                    "{}\\{name}",
                    ast::text_without_trivia(&prefix).trim_start_matches('\\').trim_end_matches('\\')
                );
            }
        }
        let fqn = Fqn::new(&name);
        if let Some((_, c)) = self.sema.index().class(&fqn) {
            let deprecated = c.deprecated.clone();
            self.deprecated(qn.text_range(), fqn.as_str(), deprecated.as_ref());
        }
    }

    fn check_class_ref(&mut self, qn: &SyntaxNode) {
        let Some(ctx) = qn.parent() else { return };
        if ctx.kind() == USE_CLAUSE {
            self.check_import_deprecation(qn, &ctx);
            return;
        }
        let is_class_ref = match ctx.kind() {
            NAMED_TYPE | EXTENDS_CLAUSE | IMPLEMENTS_CLAUSE | CATCH_TYPES | TRAIT_USE | ATTRIBUTE => true,
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
                if self.config.undefined_types && !class_guarded(qn, &fqn) {
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
                format!("Not enough arguments. Expected {required}. Found {count}."),
            ));
        } else if !variadic && count > max {
            self.push(Diagnostic::new(
                range,
                Severity::Information,
                DiagnosticCode::TooManyArguments,
                format!("Too many arguments. Expected {max}. Found {count}."),
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
                let directive = self
                    .blade
                    .as_ref()
                    .is_some_and(|b| b.directives.iter().any(|(r, _)| *r == callee.text_range()));
                if self.config.undefined_functions && !directive {
                    // as Intelephense: the namespaced name
                    let name = self.sema.scope_at(callee.text_range().start()).resolve_function(&text);
                    self.push(Diagnostic::new(
                        callee.text_range(),
                        Severity::Error,
                        DiagnosticCode::UndefinedFunction,
                        format!("Undefined function '{}'.", name.name),
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
        // closures may be bound to other objects (`Closure::bind`, `->call()`)
        let this = lhs.kind() == VARIABLE_EXPR && ast::text_without_trivia(&lhs) == "$this";
        if this && node.ancestors().any(|a| matches!(a.kind(), CLOSURE_EXPR | ARROW_FN_EXPR)) {
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
        if guarded_by(node, &["method_exists", "is_callable"], &name) {
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
        if !self.config.undefined_properties
            || self.has_magic(&classes, "__get")
            || guarded_by(node, &["property_exists"], &name)
            || coalesced(node)
        {
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
        // interfaces declare no properties; implementations may (as Intelephense)
        let interface =
            classes.iter().any(|c| index.class(c).is_some_and(|(_, d)| d.kind == ClassLikeKind::Interface));
        if dynamic || interface {
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
                    format!("Cannot instantiate {what} {fqn}."),
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
                        Severity::Information,
                        DiagnosticCode::TooManyArguments,
                        format!("Too many arguments. Expected 0. Found {n}."),
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
            // P1129: types that do not parse
            for (start, end) in php_hir::phpdoc_check::syntax_errors(tok.text()) {
                let base = tok.text_range().start();
                self.push(Diagnostic::new(
                    TextRange::new(base + TextSize::from(start as u32), base + TextSize::from(end as u32)),
                    Severity::Information,
                    DiagnosticCode::Catalog("P1129"),
                    "PHPDoc parse error. Unexpected token.",
                ));
            }
            for r in crate::doc_types::doc_type_refs(&tok) {
                if templates.contains(&r.name) {
                    continue;
                }
                let Some(anchor) = tok.parent() else { continue };
                let Some(fqn) = self.sema.resolve_class_name(&r.name, &anchor) else { continue };
                if self.sema.index().class(&fqn).is_none() {
                    self.push(Diagnostic::new(
                        r.range,
                        Severity::Information,
                        DiagnosticCode::UndefinedDocType,
                        format!("PHPDoc undefined type '{fqn}'."),
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
                    // `foreach ($a as $k => $v)` with only `$k` used: `$v` cannot be left out
                    let value_with_key =
                        parent.ancestors().find(|a| a.kind() == FOREACH_VALUE).is_some_and(|fv| {
                            fv.parent().is_some_and(|f| f.children().any(|c| c.kind() == FOREACH_KEY))
                        });
                    if !value_with_key {
                        v.def_ranges.push(tok.text_range());
                    }
                }
                Occ::Param => {
                    v.defs.push(scope.text_range().start());
                    v.param = true;
                    // by-reference parameters are outputs
                    let by_ref = parent
                        .ancestors()
                        .find(|a| a.kind() == PARAM)
                        .is_some_and(|p| p.children_with_tokens().any(|e| e.kind() == AMP));
                    // optional parameters are kept for compatibility
                    let optional = parent
                        .ancestors()
                        .find(|a| a.kind() == PARAM)
                        .is_some_and(|p| p.children_with_tokens().any(|e| e.kind() == EQ));
                    if by_ref {
                        // written for the caller
                        v.reads.push((tok.text_range(), true));
                    } else if !optional {
                        v.def_ranges.push(tok.text_range());
                    }
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
                    if !defined_before && !defined_in_loop && !isset_guarded(scope, *range, name) {
                        diags.push(Diagnostic::new(
                            *range,
                            Severity::Error,
                            DiagnosticCode::UndefinedVariable,
                            format!("Undefined variable '${name}'."),
                        ));
                    } else if defined_before
                        && !defined_in_loop
                        && !v.param
                        && !isset_guarded(scope, *range, name)
                    {
                        let before: Vec<TextSize> = v.defs.iter().copied().filter(|d| *d < pos).collect();
                        if !before.iter().any(|d| def_reaches(self.sema, scope, name, *d, pos, &before)) {
                            diags.push(Diagnostic::new(
                                *range,
                                Severity::Warning,
                                DiagnosticCode::PossiblyUndefinedVariable,
                                format!("Possible undefined variable '${name}'."),
                            ));
                        }
                    }
                }
            }
            // template variables are often only used by included views
            let template = self.blade.is_some() && scope.kind() == SOURCE_FILE;
            // unused parameters, except where a signature is given by an ancestor
            let reportable = (!v.param || !self.signature_fixed(scope))
                // closures (callbacks): only parameters after the last used one
                && !(v.param && scope.kind() == CLOSURE_EXPR && followed_by_used_param(scope, name, &vars));
            if self.config.unused_symbols && v.reads.is_empty() && reportable && !template {
                for r in &v.def_ranges {
                    // a parameter is reported with its type (like Intelephense)
                    let r = if v.param { param_range(&self.sema.root, *r) } else { *r };
                    let mut d = Diagnostic::new(
                        r,
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
                    FOREACH_VALUE | FOREACH_KEY | STATIC_VAR => Occ::Def,
                    // writes to globals are visible outside
                    GLOBAL_STMT => Occ::DefAndRead,
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

/// Whether a parameter after `name` (in the closure's parameter list) is used.
fn followed_by_used_param(scope: &SyntaxNode, name: &str, vars: &HashMap<String, VarInfo>) -> bool {
    let Some(list) = scope.children().find_map(ast::ParamList::cast) else { return false };
    let names: Vec<String> = list.params().filter_map(|p| p.name()).collect();
    let Some(pos) = names.iter().position(|n| n == name) else { return false };
    names[pos + 1..].iter().any(|n| vars.get(n).is_some_and(|v| !v.reads.is_empty()))
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
