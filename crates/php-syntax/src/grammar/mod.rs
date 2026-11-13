//! The PHP grammar, implemented as a hand written recursive descent parser
//! with a Pratt parser for expressions.

mod decls;
mod exprs;
mod stmts;
mod types;

use crate::parser::Parser;
use crate::SyntaxKind::*;

pub(crate) fn source_file(p: &mut Parser) {
    p.start_root(SOURCE_FILE);
    while !p.at(EOF) {
        if p.at(R_CURLY) {
            p.err_and_bump("unmatched `}`");
            continue;
        }
        let pos = p.pos();
        stmts::statement(p);
        if p.pos() == pos {
            p.err_and_bump("expected statement");
        }
    }
    p.flush_rest();
    p.finish_node();
}

/// Tokens that may be used as identifiers in member names (`$a->list`,
/// `Foo::class`, `function print()` in classes, named arguments, ...).
pub(crate) fn at_identifier_like(p: &mut Parser) -> bool {
    let k = p.current();
    k == IDENT || k.is_keyword()
}

pub(crate) fn nth_identifier_like(p: &mut Parser, n: usize) -> bool {
    let k = p.nth(n);
    k == IDENT || k.is_keyword()
}

/// A declaration name: `class Foo`, `function bar`. Reserved keywords are
/// accepted (with an error) for better recovery.
pub(crate) fn name(p: &mut Parser) {
    if p.at(IDENT) || p.at(READONLY_KW) {
        p.start(NAME);
        p.bump_remap(IDENT);
        p.finish_node();
    } else if p.current().is_keyword() && !matches!(p.current(), EXTENDS_KW | IMPLEMENTS_KW) {
        p.error_at_current("reserved keyword cannot be used as a name");
        p.start(NAME);
        p.bump_remap(IDENT);
        p.finish_node();
    } else {
        p.error("expected a name");
    }
}

/// A member name (method, constant, property hook, enum case, `->name`).
pub(crate) fn member_name(p: &mut Parser) {
    if at_identifier_like(p) {
        p.start(MEMBER_NAME);
        p.bump_remap(IDENT);
        p.finish_node();
    } else {
        p.error("expected a member name");
    }
}

pub(crate) fn at_name_start(p: &mut Parser) -> bool {
    match p.current() {
        IDENT => true,
        BACKSLASH => nth_identifier_like(p, 1),
        NAMESPACE_KW => p.nth_at(1, BACKSLASH),
        _ => false,
    }
}

/// A (possibly qualified) name reference such as `Foo`, `\Foo\Bar` or
/// `namespace\Foo`. Stops before `\{` so group use statements work.
pub(crate) fn qualified_name(p: &mut Parser) {
    p.start(QUALIFIED_NAME);
    qualified_name_inner(p);
    p.finish_node();
}

fn qualified_name_inner(p: &mut Parser) {
    if p.at(NAMESPACE_KW) && p.nth_at(1, BACKSLASH) {
        p.bump();
    }
    if p.at(BACKSLASH) {
        p.bump();
        if !at_identifier_like(p) {
            p.error("expected a name after `\\`");
            return;
        }
    }
    if at_identifier_like(p) {
        p.bump_remap(IDENT);
    } else {
        p.error("expected a name");
        return;
    }
    while p.at(BACKSLASH) && nth_identifier_like(p, 1) && !p.trivia_after_current() {
        p.bump();
        p.bump_remap(IDENT);
    }
}

/// `#[A, B(1)] #[C]`
pub(crate) fn attributes(p: &mut Parser) {
    while p.at(HASH_BRACK) {
        p.start(ATTRIBUTE_GROUP);
        p.bump();
        while !p.at(R_BRACK) && !p.at(EOF) {
            if !at_name_start(p) {
                p.error("expected attribute name");
                break;
            }
            p.start(ATTRIBUTE);
            qualified_name(p);
            if p.at(L_PAREN) {
                exprs::arg_list(p);
            }
            p.finish_node();
            if !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_BRACK);
        p.finish_node();
    }
}

/// Returns the lookahead index of the first token after any attribute groups
/// starting at the current position.
pub(crate) fn skip_attributes(p: &mut Parser) -> usize {
    let mut n = 0;
    while p.nth_at(n, HASH_BRACK) {
        let mut depth = 0i32;
        loop {
            match p.nth(n) {
                HASH_BRACK | L_BRACK => depth += 1,
                R_BRACK => {
                    depth -= 1;
                    if depth == 0 {
                        n += 1;
                        break;
                    }
                }
                EOF => return n,
                _ => {}
            }
            n += 1;
        }
    }
    n
}
