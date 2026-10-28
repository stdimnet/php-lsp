//! Expressions (Pratt parser).

use rowan::{Checkpoint, TextRange};

use super::{at_identifier_like, at_name_start, attributes, decls, member_name, qualified_name};
use crate::parser::Parser;
use crate::{PhpVersion, SyntaxKind, SyntaxKind::*};

/// Parses an expression. Returns `false` if no expression could be parsed
/// (and nothing was consumed).
pub(crate) fn expr(p: &mut Parser) -> bool {
    expr_bp(p, 0).is_some()
}

pub(crate) fn at_expr_start(p: &mut Parser) -> bool {
    match p.current() {
        VARIABLE | DOLLAR | INT_NUMBER | FLOAT_NUMBER | STRING | DOUBLE_QUOTE | HEREDOC_START | BACKTICK
        | L_BRACK | L_PAREN | BANG | MINUS | PLUS | TILDE | AT | PLUS2 | MINUS2 | CAST | AMP | NEW_KW
        | CLONE_KW | PRINT_KW | YIELD_KW | THROW_KW | INCLUDE_KW | INCLUDE_ONCE_KW | REQUIRE_KW
        | REQUIRE_ONCE_KW | ARRAY_KW | LIST_KW | ISSET_KW | EMPTY_KW | EVAL_KW | EXIT_KW | DIE_KW
        | FUNCTION_KW | FN_KW | STATIC_KW | HASH_BRACK | MATCH_KW | IDENT | BACKSLASH | NAMESPACE_KW
        | READONLY_KW | DOT3 => true,
        k => k.is_magic_constant(),
    }
}

fn infix_bp(p: &mut Parser, kind: SyntaxKind) -> Option<(u8, u8)> {
    let php8 = p.version >= PhpVersion::PHP_8_0;
    let bp = match kind {
        OR_KW => (2, 3),
        XOR_KW => (4, 5),
        AND_KW => (6, 7),
        QUESTION2 => (14, 14),
        PIPE2 => (16, 17),
        AMP2 => (18, 19),
        PIPE => (20, 21),
        CARET => (22, 23),
        AMP => (24, 25),
        EQ2 | NEQ | EQ3 | NEQ2 | LT_GT | SPACESHIP => (26, 27),
        LT | GT | LT_EQ | GT_EQ => (28, 29),
        PIPE_GT => (29, 30),
        DOT if php8 => (30, 31),
        SHL | SHR => (32, 33),
        PLUS | MINUS | DOT => (34, 35),
        STAR | SLASH | PERCENT => (36, 37),
        INSTANCEOF_KW => (40, 41),
        STAR2 => (44, 44),
        _ => return None,
    };
    Some(bp)
}

const TERNARY_BP: u8 = 12;
const ASSIGN_BP: u8 = 10;
const LOW_PREFIX_BP: u8 = 8;
const BANG_BP: u8 = 38;
const UNARY_BP: u8 = 42;
const CLONE_BP: u8 = 46;

/// Left-hand sides parsed as assignments. Calls, constants and literals are
/// not writable: the checks report them (Intelephense P1024).
fn is_assignable(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        VARIABLE_EXPR
            | DYNAMIC_VARIABLE_EXPR
            | INDEX_EXPR
            | PROPERTY_FETCH_EXPR
            | STATIC_PROPERTY_FETCH_EXPR
            | LIST_EXPR
            | ARRAY_EXPR
            | METHOD_CALL_EXPR
            | CALL_EXPR
            | STATIC_CALL_EXPR
            | NAME_EXPR
            | CLASS_CONST_FETCH_EXPR
            | LITERAL
    )
}

pub(crate) fn expr_bp(p: &mut Parser, min_bp: u8) -> Option<SyntaxKind> {
    let cp = p.checkpoint();
    let expr_start = p.current_start();
    let mut lhs = unary(p)?;
    loop {
        let op = p.current();
        if op.is_assign_op() && is_assignable(lhs) {
            if op == QUESTION2_EQ {
                p.require_version_here(PhpVersion::PHP_7_4, "the `??=` operator");
            }
            p.start_at(cp, ASSIGN_EXPR);
            p.bump();
            if op == EQ && p.at(AMP) {
                p.bump();
            }
            if expr_bp(p, ASSIGN_BP).is_none() {
                p.error("expected expression");
            }
            p.finish_node();
            lhs = ASSIGN_EXPR;
            continue;
        }
        if op == QUESTION {
            if TERNARY_BP < min_bp {
                break;
            }
            p.start_at(cp, TERNARY_EXPR);
            p.bump();
            if !p.eat(COLON) {
                if expr_bp(p, 0).is_none() {
                    p.error("expected expression");
                }
                p.expect(COLON);
            }
            if expr_bp(p, TERNARY_BP + 1).is_none() {
                p.error("expected expression");
            }
            p.finish_node();
            lhs = TERNARY_EXPR;
            continue;
        }
        let Some((lbp, rbp)) = infix_bp(p, op) else {
            break;
        };
        if lbp < min_bp {
            break;
        }
        let node = if op == INSTANCEOF_KW { INSTANCEOF_EXPR } else { BIN_EXPR };
        match op {
            QUESTION2 => p.require_version_here(PhpVersion::PHP_7_0, "the `??` operator"),
            SPACESHIP => p.require_version_here(PhpVersion::PHP_7_0, "the `<=>` operator"),
            STAR2 => p.require_version_here(PhpVersion::PHP_5_6, "the `**` operator"),
            _ => {}
        }
        p.start_at(cp, node);
        p.bump();
        if expr_bp(p, rbp).is_none() {
            p.error("expected expression");
        }
        if op == PIPE_GT {
            // the whole pipe, like Intelephense
            let range = rowan::TextRange::new(expr_start, p.prev_end().max(expr_start));
            p.require_version(PhpVersion::PHP_8_5, "the pipe operator", range);
        }
        p.finish_node();
        lhs = node;
    }
    Some(lhs)
}

fn prefix(p: &mut Parser, kind: SyntaxKind, bp: u8) -> Option<SyntaxKind> {
    p.start(kind);
    p.bump();
    if expr_bp(p, bp).is_none() {
        p.error("expected expression");
    }
    p.finish_node();
    Some(kind)
}

fn unary(p: &mut Parser) -> Option<SyntaxKind> {
    match p.current() {
        BANG => prefix(p, PREFIX_EXPR, BANG_BP),
        MINUS | PLUS | TILDE | AT => prefix(p, PREFIX_EXPR, UNARY_BP),
        PLUS2 | MINUS2 => prefix(p, PREFIX_EXPR, CLONE_BP),
        AMP => {
            // Only valid in a few places (`=&`, `=> &`), which consume the `&`
            // themselves. Recover by treating it as a prefix.
            p.error_at_current("unexpected `&`");
            prefix(p, PREFIX_EXPR, UNARY_BP)
        }
        CAST => {
            // `(unset)` is reported as deprecated, like Intelephense does
            if p.current_text().to_ascii_lowercase().contains("void") {
                p.require_version_here(PhpVersion::PHP_8_5, "(void) casts");
            }
            prefix(p, CAST_EXPR, UNARY_BP)
        }
        CLONE_KW if !p.nth_at(1, L_PAREN) || p.version < PhpVersion::PHP_8_5 => {
            prefix(p, CLONE_EXPR, CLONE_BP)
        }
        PRINT_KW => prefix(p, PRINT_EXPR, LOW_PREFIX_BP),
        INCLUDE_KW | INCLUDE_ONCE_KW | REQUIRE_KW | REQUIRE_ONCE_KW => prefix(p, INCLUDE_EXPR, LOW_PREFIX_BP),
        THROW_KW => {
            p.require_version_here(PhpVersion::PHP_8_0, "throw expressions");
            throw_expr(p)
        }
        YIELD_KW => Some(yield_expr(p)),
        NEW_KW => {
            let cp = p.checkpoint();
            let had_args = new_expr(p);
            if had_args && matches!(p.current(), ARROW | NULLSAFE_ARROW | COLON2 | L_BRACK | L_PAREN) {
                p.require_version_here(PhpVersion::PHP_8_4, "member access on `new` without parentheses");
                Some(postfix(p, cp, NEW_EXPR))
            } else {
                Some(NEW_EXPR)
            }
        }
        _ => {
            let cp = p.checkpoint();
            let kind = primary(p)?;
            Some(postfix(p, cp, kind))
        }
    }
}

/// `throw expr` – used both as statement and (PHP 8) as expression.
pub(crate) fn throw_expr(p: &mut Parser) -> Option<SyntaxKind> {
    p.start(THROW_EXPR);
    p.bump();
    if expr_bp(p, 0).is_none() {
        p.error("expected expression");
    }
    p.finish_node();
    Some(THROW_EXPR)
}

fn yield_expr(p: &mut Parser) -> SyntaxKind {
    let start = p.current_start();
    if p.nth_at_contextual(1, "from") {
        p.start(YIELD_FROM_EXPR);
        p.bump();
        p.bump();
        if expr_bp(p, LOW_PREFIX_BP).is_none() {
            p.error("expected expression");
        }
        p.finish_node();
        p.require_version(PhpVersion::PHP_7_0, "`yield from`", TextRange::new(start, p.prev_end()));
        return YIELD_FROM_EXPR;
    }
    p.require_version_here(PhpVersion::PHP_5_5, "generators");
    p.start(YIELD_EXPR);
    p.bump();
    if at_expr_start(p) && !p.at(AMP) {
        expr_bp(p, LOW_PREFIX_BP);
        if p.eat(FAT_ARROW) && expr_bp(p, LOW_PREFIX_BP).is_none() {
            p.error("expected expression");
        }
    }
    p.finish_node();
    YIELD_EXPR
}

fn primary(p: &mut Parser) -> Option<SyntaxKind> {
    let kind = match p.current() {
        VARIABLE => {
            p.start(VARIABLE_EXPR);
            p.bump();
            p.finish_node();
            VARIABLE_EXPR
        }
        DOLLAR => dynamic_variable(p),
        INT_NUMBER | FLOAT_NUMBER | STRING => {
            p.start(LITERAL);
            p.bump();
            p.finish_node();
            LITERAL
        }
        k if k.is_magic_constant() => {
            p.start(MAGIC_CONST_EXPR);
            p.bump();
            p.finish_node();
            MAGIC_CONST_EXPR
        }
        DOUBLE_QUOTE => interpolated(p, INTERPOLATED_STRING, DOUBLE_QUOTE),
        BACKTICK => interpolated(p, SHELL_EXEC, BACKTICK),
        HEREDOC_START => interpolated(p, HEREDOC, HEREDOC_END),
        L_BRACK => {
            p.require_version_here(PhpVersion::PHP_5_4, "short array syntax");
            array_like(p, ARRAY_EXPR, R_BRACK)
        }
        ARRAY_KW if p.nth_at(1, L_PAREN) => array_like(p, ARRAY_EXPR, R_PAREN),
        LIST_KW if p.nth_at(1, L_PAREN) => array_like(p, LIST_EXPR, R_PAREN),
        ISSET_KW => {
            p.start(ISSET_EXPR);
            p.bump();
            paren_expr_list(p);
            p.finish_node();
            ISSET_EXPR
        }
        EMPTY_KW | EVAL_KW => {
            let kind = if p.at(EMPTY_KW) { EMPTY_EXPR } else { EVAL_EXPR };
            p.start(kind);
            p.bump();
            if p.expect(L_PAREN) {
                if !expr(p) {
                    p.error("expected expression");
                }
                p.expect(R_PAREN);
            }
            p.finish_node();
            kind
        }
        EXIT_KW | DIE_KW => {
            p.start(EXIT_EXPR);
            p.bump();
            if p.at(L_PAREN) {
                arg_list(p);
            }
            p.finish_node();
            EXIT_EXPR
        }
        L_PAREN => {
            p.start(PAREN_EXPR);
            p.bump();
            if !expr(p) {
                p.error("expected expression");
            }
            p.expect(R_PAREN);
            p.finish_node();
            PAREN_EXPR
        }
        FUNCTION_KW | FN_KW | HASH_BRACK => closure(p),
        STATIC_KW if matches!(p.nth(1), FUNCTION_KW | FN_KW) => closure(p),
        MATCH_KW if p.nth_at(1, L_PAREN) && (p.version >= PhpVersion::PHP_8_0 || match_follows(p)) => {
            match_expr(p)
        }
        // contextual keywords used as plain names on older versions
        MATCH_KW | READONLY_KW => {
            p.start(NAME_EXPR);
            p.start(QUALIFIED_NAME);
            p.bump_remap(IDENT);
            p.finish_node();
            p.finish_node();
            NAME_EXPR
        }
        STATIC_KW => {
            // `static::foo()`
            p.start(NAME_EXPR);
            p.start(QUALIFIED_NAME);
            p.bump_remap(IDENT);
            p.finish_node();
            p.finish_node();
            NAME_EXPR
        }
        CLONE_KW => {
            // PHP 8.5: `clone($obj, [...])`
            p.start(CALL_EXPR);
            p.start(NAME_EXPR);
            p.start(QUALIFIED_NAME);
            p.bump_remap(IDENT);
            p.finish_node();
            p.finish_node();
            arg_list(p);
            p.finish_node();
            CALL_EXPR
        }
        _ if at_name_start(p) => {
            p.start(NAME_EXPR);
            qualified_name(p);
            p.finish_node();
            NAME_EXPR
        }
        _ => return None,
    };
    Some(kind)
}

/// `$$a`, `${'a' . 'b'}`
fn dynamic_variable(p: &mut Parser) -> SyntaxKind {
    p.start(DYNAMIC_VARIABLE_EXPR);
    p.bump();
    match p.current() {
        L_CURLY => {
            p.bump();
            if !expr(p) {
                p.error("expected expression");
            }
            p.expect(R_CURLY);
        }
        VARIABLE => {
            p.start(VARIABLE_EXPR);
            p.bump();
            p.finish_node();
        }
        DOLLAR => {
            dynamic_variable(p);
        }
        _ => p.error("expected variable"),
    }
    p.finish_node();
    DYNAMIC_VARIABLE_EXPR
}

fn postfix(p: &mut Parser, cp: Checkpoint, mut lhs: SyntaxKind) -> SyntaxKind {
    loop {
        match p.current() {
            L_BRACK => {
                p.start_at(cp, INDEX_EXPR);
                p.bump();
                if !p.at(R_BRACK) && !expr(p) {
                    p.error("expected expression");
                }
                p.expect(R_BRACK);
                p.finish_node();
                lhs = INDEX_EXPR;
            }
            // in PHP 8 only directly after the expression (reported as deprecated)
            L_CURLY
                if (p.version < PhpVersion::PHP_8_0 || p.current_range().start() == p.prev_end())
                    && matches!(
                        lhs,
                        VARIABLE_EXPR | INDEX_EXPR | PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR
                    ) =>
            {
                // legacy `$str{0}` offset
                p.start_at(cp, INDEX_EXPR);
                p.bump();
                expr(p);
                p.expect(R_CURLY);
                p.finish_node();
                lhs = INDEX_EXPR;
            }
            ARROW | NULLSAFE_ARROW => {
                if p.at(NULLSAFE_ARROW) {
                    p.require_version_here(PhpVersion::PHP_8_0, "the nullsafe operator");
                }
                p.bump();
                dynamic_member_name(p);
                lhs = if p.at(L_PAREN) {
                    p.start_at(cp, METHOD_CALL_EXPR);
                    arg_list(p);
                    METHOD_CALL_EXPR
                } else {
                    p.start_at(cp, PROPERTY_FETCH_EXPR);
                    PROPERTY_FETCH_EXPR
                };
                p.finish_node();
            }
            COLON2 => {
                p.bump();
                let kind = match p.current() {
                    VARIABLE => {
                        p.start(MEMBER_NAME);
                        p.bump();
                        p.finish_node();
                        STATIC_PROPERTY_FETCH_EXPR
                    }
                    DOLLAR => {
                        p.start(MEMBER_NAME);
                        dynamic_variable(p);
                        p.finish_node();
                        STATIC_PROPERTY_FETCH_EXPR
                    }
                    L_CURLY => {
                        p.start(MEMBER_NAME);
                        p.bump();
                        expr(p);
                        p.expect(R_CURLY);
                        p.finish_node();
                        CLASS_CONST_FETCH_EXPR
                    }
                    _ => {
                        member_name(p);
                        CLASS_CONST_FETCH_EXPR
                    }
                };
                lhs = if p.at(L_PAREN) {
                    p.start_at(cp, STATIC_CALL_EXPR);
                    arg_list(p);
                    STATIC_CALL_EXPR
                } else {
                    p.start_at(cp, kind);
                    kind
                };
                p.finish_node();
            }
            L_PAREN => {
                p.start_at(cp, CALL_EXPR);
                arg_list(p);
                p.finish_node();
                lhs = CALL_EXPR;
            }
            PLUS2 | MINUS2 => {
                p.start_at(cp, POSTFIX_EXPR);
                p.bump();
                p.finish_node();
                lhs = POSTFIX_EXPR;
            }
            _ => break,
        }
    }
    lhs
}

/// Member name after `->`: identifier, keyword, `$var` or `{expr}`.
fn dynamic_member_name(p: &mut Parser) {
    match p.current() {
        VARIABLE => {
            p.start(MEMBER_NAME);
            p.start(VARIABLE_EXPR);
            p.bump();
            p.finish_node();
            p.finish_node();
        }
        DOLLAR => {
            p.start(MEMBER_NAME);
            dynamic_variable(p);
            p.finish_node();
        }
        L_CURLY => {
            p.start(MEMBER_NAME);
            p.bump();
            if !expr(p) {
                p.error("expected expression");
            }
            p.expect(R_CURLY);
            p.finish_node();
        }
        _ => member_name(p),
    }
}

pub(crate) fn arg_list(p: &mut Parser) {
    p.start(ARG_LIST);
    p.expect(L_PAREN);
    while !p.at(R_PAREN) && !p.at(EOF) {
        let pos = p.pos();
        p.start(ARG);
        if p.at(DOT3) {
            p.bump();
            if p.at(R_PAREN) {
                // first-class callable syntax `foo(...)`
                p.require_version(PhpVersion::PHP_8_1, "first-class callable syntax", p.current_range());
            } else if !expr(p) {
                p.error("expected expression");
            }
        } else if at_identifier_like(p) && p.nth_at(1, COLON) {
            p.require_version_here(PhpVersion::PHP_8_0, "named arguments");
            p.start(NAME);
            p.bump_remap(IDENT);
            p.finish_node();
            p.bump();
            if !expr(p) {
                p.error("expected expression");
            }
        } else if !expr(p) {
            p.error("expected argument");
        }
        p.finish_node();
        if p.pos() == pos {
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
        if p.at(R_PAREN) {
            p.require_version(PhpVersion::PHP_7_3, "trailing commas in calls", p.current_range());
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
}

fn paren_expr_list(p: &mut Parser) {
    if !p.expect(L_PAREN) {
        return;
    }
    while !p.at(R_PAREN) && !p.at(EOF) {
        if !expr(p) {
            p.error("expected expression");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    p.expect(R_PAREN);
}

/// `[...]`, `array(...)`, `list(...)`
fn array_like(p: &mut Parser, kind: SyntaxKind, close: SyntaxKind) -> SyntaxKind {
    p.start(kind);
    if close == R_PAREN {
        p.bump(); // array / list
    }
    p.bump(); // ( or [
    while !p.at(close) && !p.at(EOF) {
        if p.at(COMMA) {
            // skipped element in destructuring: `[, $b]`
            p.bump();
            continue;
        }
        let pos = p.pos();
        array_element(p);
        if p.pos() == pos {
            p.error("expected array element");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    p.expect(close);
    p.finish_node();
    kind
}

fn array_element(p: &mut Parser) {
    p.start(ARRAY_ELEMENT);
    if p.at(DOT3) {
        p.require_version_here(PhpVersion::PHP_7_4, "spread operator in arrays");
        p.bump();
        if !expr(p) {
            p.error("expected expression");
        }
    } else if p.at(AMP) {
        p.bump();
        expr(p);
    } else if expr(p) && p.eat(FAT_ARROW) {
        p.eat(AMP);
        if !expr(p) {
            p.error("expected expression");
        }
    }
    p.finish_node();
}

fn interpolated(p: &mut Parser, kind: SyntaxKind, close: SyntaxKind) -> SyntaxKind {
    p.start(kind);
    p.bump();
    loop {
        match p.current() {
            k if k == close => {
                p.bump();
                break;
            }
            EOF => {
                p.error("unterminated string");
                break;
            }
            ENCAPSED_TEXT => p.bump(),
            VARIABLE => {
                let cp = p.checkpoint();
                p.start(VARIABLE_EXPR);
                p.bump();
                p.finish_node();
                if p.at(L_BRACK) {
                    p.start_at(cp, INDEX_EXPR);
                    p.bump();
                    if p.at(MINUS) {
                        p.start(PREFIX_EXPR);
                        p.bump();
                        p.start(LITERAL);
                        p.expect(INT_NUMBER);
                        p.finish_node();
                        p.finish_node();
                    } else if p.at(VARIABLE) {
                        p.start(VARIABLE_EXPR);
                        p.bump();
                        p.finish_node();
                    } else if p.at(INT_NUMBER) || p.at(IDENT) {
                        p.start(LITERAL);
                        p.bump();
                        p.finish_node();
                    } else {
                        p.error("expected array offset");
                    }
                    p.expect(R_BRACK);
                    p.finish_node();
                } else if p.at(ARROW) || p.at(NULLSAFE_ARROW) {
                    p.start_at(cp, PROPERTY_FETCH_EXPR);
                    p.bump();
                    member_name(p);
                    p.finish_node();
                }
            }
            L_CURLY => {
                p.start(INTERPOLATION);
                p.bump();
                if !expr(p) {
                    p.error("expected expression");
                }
                p.expect(R_CURLY);
                p.finish_node();
            }
            DOLLAR_CURLY => {
                p.start(INTERPOLATION);
                p.bump();
                if p.at(IDENT) && matches!(p.nth(1), R_CURLY | L_BRACK) {
                    let cp = p.checkpoint();
                    p.start(VARIABLE_EXPR);
                    p.bump();
                    p.finish_node();
                    if p.at(L_BRACK) {
                        p.start_at(cp, INDEX_EXPR);
                        p.bump();
                        expr(p);
                        p.expect(R_BRACK);
                        p.finish_node();
                    }
                } else if !expr(p) {
                    p.error("expected expression");
                }
                p.expect(R_CURLY);
                p.finish_node();
            }
            _ => p.err_and_bump("unexpected token in string"),
        }
    }
    p.finish_node();
    kind
}

/// Closures and arrow functions, optionally `static` and with attributes.
fn closure(p: &mut Parser) -> SyntaxKind {
    let n = super::skip_attributes(p);
    let mut m = n;
    if p.nth_at(m, STATIC_KW) {
        m += 1;
    }
    let is_arrow = p.nth_at(m, FN_KW);
    let kind = if is_arrow { ARROW_FN_EXPR } else { CLOSURE_EXPR };
    p.start(kind);
    attributes(p);
    p.eat(STATIC_KW);
    if is_arrow {
        p.require_version_here(PhpVersion::PHP_7_4, "arrow functions");
        p.bump();
    } else if !p.expect(FUNCTION_KW) {
        p.finish_node();
        return kind;
    }
    p.eat(AMP);
    decls::param_list(p);
    if !is_arrow && p.at(USE_KW) {
        p.start(CLOSURE_USE);
        p.bump();
        p.expect(L_PAREN);
        while !p.at(R_PAREN) && !p.at(EOF) {
            p.start(CLOSURE_USE_VAR);
            p.eat(AMP);
            p.expect(VARIABLE);
            p.finish_node();
            if !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_PAREN);
        p.finish_node();
    }
    decls::opt_return_type(p);
    if is_arrow {
        p.expect(FAT_ARROW);
        if expr_bp(p, 0).is_none() {
            p.error("expected expression");
        }
    } else {
        super::stmts::block(p);
    }
    p.finish_node();
    kind
}

/// Before PHP 8 `match` is a name: `match (...) {` is still parsed as a
/// `match` expression (a call cannot be followed by `{`), so that the
/// version is reported instead of a cascade of syntax errors.
fn match_follows(p: &mut Parser) -> bool {
    let mut depth = 0usize;
    for i in 1..2000 {
        match p.nth(i) {
            L_PAREN => depth += 1,
            R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    return p.nth_at(i + 1, L_CURLY);
                }
            }
            EOF | SEMICOLON | L_CURLY | R_CURLY => return false,
            _ => {}
        }
    }
    false
}

fn match_expr(p: &mut Parser) -> SyntaxKind {
    p.require_version_here(PhpVersion::PHP_8_0, "match expressions");
    p.start(MATCH_EXPR);
    p.bump();
    p.expect(L_PAREN);
    if !expr(p) {
        p.error("expected expression");
    }
    p.expect(R_PAREN);
    if p.expect(L_CURLY) {
        while !p.at(R_CURLY) && !p.at(EOF) {
            let pos = p.pos();
            p.start(MATCH_ARM);
            if p.at(DEFAULT_KW) {
                p.bump();
                p.eat(COMMA);
            } else {
                p.start(MATCH_CONDITIONS);
                while !p.at(FAT_ARROW) && !p.at(EOF) {
                    if !expr(p) {
                        p.error("expected expression");
                        break;
                    }
                    if !p.eat(COMMA) {
                        break;
                    }
                }
                p.finish_node();
            }
            p.expect(FAT_ARROW);
            if !expr(p) {
                p.error("expected expression");
            }
            p.finish_node();
            if p.pos() == pos {
                p.err_and_bump("unexpected token in match");
                continue;
            }
            if !p.eat(COMMA) && !p.at(R_CURLY) {
                p.error("expected `,`");
                if !p.at(R_CURLY) && !at_expr_start(p) && !p.at(DEFAULT_KW) {
                    break;
                }
            }
        }
        p.expect(R_CURLY);
    }
    p.finish_node();
    MATCH_EXPR
}

/// `new Foo(...)`, `new class(...) {}`, `new $cls`, `new (expr)`.
/// Returns whether an argument list was present.
fn new_expr(p: &mut Parser) -> bool {
    p.start(NEW_EXPR);
    p.bump();
    let n = super::skip_attributes(p);
    let mut m = n;
    while p.nth_at(m, READONLY_KW) || p.nth_at(m, FINAL_KW) || p.nth_at(m, ABSTRACT_KW) {
        m += 1;
    }
    if p.nth_at(m, CLASS_KW) {
        decls::anonymous_class(p);
        p.finish_node();
        return true;
    }
    let mut has_args = false;
    match p.current() {
        L_PAREN => {
            p.require_version_here(PhpVersion::PHP_8_0, "arbitrary expressions in `new`");
            p.start(PAREN_EXPR);
            p.bump();
            if !expr(p) {
                p.error("expected expression");
            }
            p.expect(R_PAREN);
            p.finish_node();
        }
        VARIABLE | DOLLAR => class_name_variable(p),
        STATIC_KW => class_name_variable(p),
        _ if at_name_start(p) => class_name_variable(p),
        _ => p.error("expected class name"),
    }
    if p.at(L_PAREN) {
        arg_list(p);
        has_args = true;
    }
    p.finish_node();
    has_args
}

/// Restricted variable expression usable as class reference in `new`:
/// `$a`, `$a->b`, `$a::$b`, `$a['x']` – but no calls.
fn class_name_variable(p: &mut Parser) {
    let cp = p.checkpoint();
    let mut lhs = match p.current() {
        VARIABLE => {
            p.start(VARIABLE_EXPR);
            p.bump();
            p.finish_node();
            VARIABLE_EXPR
        }
        DOLLAR => dynamic_variable(p),
        STATIC_KW => {
            p.start(NAME_EXPR);
            p.start(QUALIFIED_NAME);
            p.bump_remap(IDENT);
            p.finish_node();
            p.finish_node();
            NAME_EXPR
        }
        _ => {
            p.start(NAME_EXPR);
            qualified_name(p);
            p.finish_node();
            NAME_EXPR
        }
    };
    loop {
        match p.current() {
            L_BRACK | ARROW | NULLSAFE_ARROW if lhs == NAME_EXPR => break,
            L_BRACK => {
                p.start_at(cp, INDEX_EXPR);
                p.bump();
                expr(p);
                p.expect(R_BRACK);
                p.finish_node();
                lhs = INDEX_EXPR;
            }
            ARROW | NULLSAFE_ARROW => {
                if p.at(NULLSAFE_ARROW) {
                    p.require_version_here(PhpVersion::PHP_8_0, "the nullsafe operator");
                }
                p.start_at(cp, PROPERTY_FETCH_EXPR);
                p.bump();
                dynamic_member_name(p);
                p.finish_node();
                lhs = PROPERTY_FETCH_EXPR;
            }
            COLON2 if p.nth_at(1, VARIABLE) => {
                p.start_at(cp, STATIC_PROPERTY_FETCH_EXPR);
                p.bump();
                p.start(MEMBER_NAME);
                p.bump();
                p.finish_node();
                p.finish_node();
                lhs = STATIC_PROPERTY_FETCH_EXPR;
            }
            _ => break,
        }
    }
}
