//! Type declarations: `?int`, `A|B`, `A&B`, `(A&B)|null`.

use super::{at_name_start, qualified_name};
use crate::parser::Parser;
use crate::{PhpVersion, SyntaxKind::*};

pub(crate) fn at_type_start(p: &mut Parser) -> bool {
    matches!(p.current(), QUESTION | L_PAREN | ARRAY_KW | CALLABLE_KW | STATIC_KW) || at_name_start(p)
}

pub(crate) fn type_decl(p: &mut Parser) {
    if p.at(QUESTION) {
        let start = p.current_start();
        p.start(NULLABLE_TYPE);
        p.bump();
        atom(p);
        p.finish_node();
        let range = rowan::TextRange::new(start, p.prev_end());
        p.require_version(PhpVersion::PHP_7_1, "nullable types", range);
        return;
    }
    let start = p.current_start();
    let cp = p.checkpoint();
    intersection_or_atom(p);
    if p.at(PIPE) {
        p.start_at(cp, UNION_TYPE);
        while p.eat(PIPE) {
            intersection_or_atom(p);
        }
        p.finish_node();
        let range = rowan::TextRange::new(start, p.prev_end());
        p.require_version(PhpVersion::PHP_8_0, "union types", range);
    }
}

fn at_intersection_amp(p: &mut Parser) -> bool {
    // `A & $x` is a by-reference parameter, `A&B $x` an intersection type.
    p.at(AMP) && !matches!(p.nth(1), VARIABLE | DOT3 | AMP | EQ | R_PAREN | COMMA)
}

fn intersection_or_atom(p: &mut Parser) {
    if p.at(L_PAREN) {
        let start = p.current_start();
        p.start(PAREN_TYPE);
        p.bump();
        let cp = p.checkpoint();
        atom(p);
        if p.at(AMP) {
            p.start_at(cp, INTERSECTION_TYPE);
            while p.eat(AMP) {
                atom(p);
            }
            p.finish_node();
        }
        p.expect(R_PAREN);
        p.finish_node();
        let range = rowan::TextRange::new(start, p.prev_end());
        p.require_version(PhpVersion::PHP_8_2, "DNF types", range);
        return;
    }
    let start = p.current_start();
    let cp = p.checkpoint();
    atom(p);
    if at_intersection_amp(p) {
        p.start_at(cp, INTERSECTION_TYPE);
        while at_intersection_amp(p) {
            p.bump();
            atom(p);
        }
        p.finish_node();
        let range = rowan::TextRange::new(start, p.prev_end());
        p.require_version(PhpVersion::PHP_8_1, "intersection types", range);
    }
}

fn atom(p: &mut Parser) {
    match p.current() {
        ARRAY_KW | CALLABLE_KW | STATIC_KW => {
            p.start(NAMED_TYPE);
            p.bump();
            p.finish_node();
        }
        _ if at_name_start(p) => {
            p.start(NAMED_TYPE);
            qualified_name(p);
            p.finish_node();
        }
        _ => p.error("expected a type"),
    }
}
