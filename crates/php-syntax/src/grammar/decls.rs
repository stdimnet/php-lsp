//! Declarations: namespaces, use, functions, classes, interfaces, traits,
//! enums and their members.

use rowan::TextRange;

use super::exprs::{arg_list, expr};
use super::stmts::{block, stmt_end};
use super::types::{at_type_start, type_decl};
use super::{
    at_identifier_like, at_name_start, attributes, member_name, name, qualified_name, skip_attributes,
};
use crate::parser::Parser;
use crate::{PhpVersion, SyntaxKind::*};

/// Tries to parse a function / class-like declaration at the current
/// position. Returns `false` if there is none (e.g. a closure expression).
pub(crate) fn try_declaration(p: &mut Parser) -> bool {
    let n = skip_attributes(p);
    let mut m = n;
    while matches!(p.nth(m), ABSTRACT_KW | FINAL_KW | READONLY_KW) {
        m += 1;
    }
    match p.nth(m) {
        CLASS_KW if !p.nth_at(m + 1, COLON2) => class_decl(p),
        INTERFACE_KW if m == n => interface_decl(p),
        TRAIT_KW if m == n => trait_decl(p),
        FUNCTION_KW if m == n && is_named_function(p, m + 1) => function_decl(p),
        IDENT
            if m == n
                && p.nth_text(m).eq_ignore_ascii_case("enum")
                && super::nth_identifier_like(p, m + 1)
                && p.nth(m + 1) != EXTENDS_KW
                && p.nth(m + 1) != IMPLEMENTS_KW =>
        {
            enum_decl(p)
        }
        _ => return false,
    }
    true
}

fn is_named_function(p: &mut Parser, n: usize) -> bool {
    let n = if p.nth_at(n, AMP) { n + 1 } else { n };
    super::nth_identifier_like(p, n) && p.nth_at(n + 1, L_PAREN)
}

fn modifiers(p: &mut Parser) {
    if !p.current().is_modifier() {
        return;
    }
    p.start(MODIFIER_LIST);
    while p.current().is_modifier() {
        let is_visibility = matches!(p.current(), PUBLIC_KW | PROTECTED_KW | PRIVATE_KW);
        let start = p.current_start();
        p.bump();
        if is_visibility && p.at(L_PAREN) && p.nth_at_contextual(1, "set") && p.nth_at(2, R_PAREN) {
            p.bump();
            p.bump();
            p.bump();
            let range = TextRange::new(start, p.prev_end());
            p.require_version(PhpVersion::PHP_8_4, "asymmetric visibility", range);
        }
    }
    p.finish_node();
}

pub(crate) fn namespace_decl(p: &mut Parser) {
    p.start(NAMESPACE_DECL);
    p.bump();
    if at_name_start(p) {
        qualified_name(p);
    }
    if p.at(L_CURLY) {
        block(p);
    } else {
        stmt_end(p);
    }
    p.finish_node();
}

pub(crate) fn use_decl(p: &mut Parser) {
    p.start(USE_DECL);
    p.bump();
    if p.at(FUNCTION_KW) || p.at(CONST_KW) {
        p.bump();
    }
    loop {
        use_clause(p, false);
        if !p.eat(COMMA) {
            break;
        }
    }
    stmt_end(p);
    p.finish_node();
}

fn use_clause(p: &mut Parser, in_group: bool) {
    p.start(USE_CLAUSE);
    if in_group && (p.at(FUNCTION_KW) || p.at(CONST_KW)) {
        p.bump();
    }
    if at_name_start(p) {
        qualified_name(p);
    } else if !(p.at(BACKSLASH) && p.nth_at(1, L_CURLY)) {
        p.error("expected a name");
    }
    if !in_group && p.at(BACKSLASH) && p.nth_at(1, L_CURLY) {
        p.bump();
        p.start(USE_GROUP);
        p.bump();
        while !p.at(R_CURLY) && !p.at(EOF) {
            let pos = p.pos();
            use_clause(p, true);
            if p.pos() == pos || !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_CURLY);
        p.finish_node();
    } else if p.at(AS_KW) {
        p.start(USE_ALIAS);
        p.bump();
        name(p);
        p.finish_node();
    }
    p.finish_node();
}

pub(crate) fn const_decl(p: &mut Parser) {
    p.start(CONST_DECL);
    attributes(p);
    p.expect(CONST_KW);
    loop {
        p.start(CONST_ELEMENT);
        name(p);
        p.expect(EQ);
        if !expr(p) {
            p.error("expected expression");
        }
        p.finish_node();
        if !p.eat(COMMA) {
            break;
        }
    }
    stmt_end(p);
    p.finish_node();
}

fn function_decl(p: &mut Parser) {
    p.start(FUNCTION_DECL);
    attributes(p);
    p.bump();
    p.eat(AMP);
    // `function exit()` is reported by the checks ("not allowed")
    if matches!(p.current(), EXIT_KW | DIE_KW) {
        p.start(NAME);
        p.bump_remap(IDENT);
        p.finish_node();
    } else {
        name(p);
    }
    param_list(p);
    opt_return_type(p);
    block(p);
    p.finish_node();
}

fn class_decl(p: &mut Parser) {
    p.start(CLASS_DECL);
    attributes(p);
    let has_readonly = {
        let mut i = 0;
        let mut found = false;
        while matches!(p.nth(i), ABSTRACT_KW | FINAL_KW | READONLY_KW) {
            found |= p.nth_at(i, READONLY_KW);
            i += 1;
        }
        found
    };
    if has_readonly {
        p.require_version_here(PhpVersion::PHP_8_2, "readonly classes");
    }
    modifiers(p);
    p.expect(CLASS_KW);
    name(p);
    class_tail(p);
    p.finish_node();
}

fn class_tail(p: &mut Parser) {
    if p.at(EXTENDS_KW) {
        p.start(EXTENDS_CLAUSE);
        p.bump();
        name_list(p);
        p.finish_node();
    }
    if p.at(IMPLEMENTS_KW) {
        p.start(IMPLEMENTS_CLAUSE);
        p.bump();
        name_list(p);
        p.finish_node();
    }
    member_list(p);
}

pub(crate) fn anonymous_class(p: &mut Parser) {
    p.start(ANON_CLASS);
    attributes(p);
    modifiers(p);
    p.expect(CLASS_KW);
    p.require_version(PhpVersion::PHP_7_0, "anonymous classes", TextRange::empty(p.prev_end()));
    if p.at(L_PAREN) {
        arg_list(p);
    }
    class_tail(p);
    p.finish_node();
}

fn interface_decl(p: &mut Parser) {
    p.start(INTERFACE_DECL);
    attributes(p);
    p.bump();
    name(p);
    if p.at(EXTENDS_KW) {
        p.start(EXTENDS_CLAUSE);
        p.bump();
        name_list(p);
        p.finish_node();
    }
    member_list(p);
    p.finish_node();
}

fn trait_decl(p: &mut Parser) {
    p.start(TRAIT_DECL);
    attributes(p);
    p.require_version_here(PhpVersion::PHP_5_4, "traits");
    p.bump();
    name(p);
    member_list(p);
    p.finish_node();
}

fn enum_decl(p: &mut Parser) {
    p.start(ENUM_DECL);
    attributes(p);
    p.require_version_here(PhpVersion::PHP_8_1, "enums");
    p.bump();
    name(p);
    if p.at(COLON) {
        p.start(ENUM_BACKING_TYPE);
        p.bump();
        type_decl(p);
        p.finish_node();
    }
    if p.at(IMPLEMENTS_KW) {
        p.start(IMPLEMENTS_CLAUSE);
        p.bump();
        name_list(p);
        p.finish_node();
    }
    member_list(p);
    p.finish_node();
}

fn name_list(p: &mut Parser) {
    loop {
        if at_name_start(p) {
            qualified_name(p);
        } else {
            p.error("expected a name");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
}

fn member_list(p: &mut Parser) {
    p.start(MEMBER_LIST);
    if p.expect(L_CURLY) {
        while !p.at(R_CURLY) && !p.at(EOF) {
            let pos = p.pos();
            member(p);
            if p.pos() == pos {
                p.err_and_bump("expected a class member");
            }
        }
        p.expect(R_CURLY);
    }
    p.finish_node();
}

fn member(p: &mut Parser) {
    let n = skip_attributes(p);
    let mut m = n;
    loop {
        let k = p.nth(m);
        if !k.is_modifier() {
            break;
        }
        m += 1;
        if matches!(k, PUBLIC_KW | PROTECTED_KW | PRIVATE_KW)
            && p.nth_at(m, L_PAREN)
            && p.nth_at_contextual(m + 1, "set")
            && p.nth_at(m + 2, R_PAREN)
        {
            m += 3;
        }
    }
    match p.nth(m) {
        USE_KW => trait_use(p),
        CASE_KW => enum_case(p),
        CONST_KW => class_const(p),
        FUNCTION_KW => method(p),
        VARIABLE => property(p),
        _ if m > 0 && is_type_start_at(p, m) => property(p),
        _ if m > 0 => {
            // attributes / modifiers without a member
            p.start(ERROR);
            attributes(p);
            modifiers(p);
            p.finish_node();
            p.error("expected a class member");
        }
        _ => {}
    }
}

fn is_type_start_at(p: &mut Parser, n: usize) -> bool {
    matches!(p.nth(n), QUESTION | L_PAREN | ARRAY_KW | CALLABLE_KW | STATIC_KW | IDENT | BACKSLASH)
        || (p.nth_at(n, NAMESPACE_KW) && p.nth_at(n + 1, BACKSLASH))
}

fn method(p: &mut Parser) {
    p.start(METHOD_DECL);
    attributes(p);
    modifiers(p);
    p.bump();
    p.eat(AMP);
    member_name(p);
    param_list(p);
    opt_return_type(p);
    if p.at(L_CURLY) {
        block(p);
    } else {
        stmt_end(p);
    }
    p.finish_node();
}

fn class_const(p: &mut Parser) {
    p.start(CLASS_CONST_DECL);
    attributes(p);
    modifiers(p);
    p.bump();
    if !p.nth_at(1, EQ) && at_type_start(p) {
        p.require_version_here(PhpVersion::PHP_8_3, "typed class constants");
        type_decl(p);
    }
    loop {
        p.start(CONST_ELEMENT);
        member_name(p);
        p.expect(EQ);
        if !expr(p) {
            p.error("expected expression");
        }
        p.finish_node();
        if !p.eat(COMMA) {
            break;
        }
    }
    stmt_end(p);
    p.finish_node();
}

fn property(p: &mut Parser) {
    p.start(PROPERTY_DECL);
    attributes(p);
    modifiers(p);
    if !p.at(VARIABLE) {
        p.require_version_here(PhpVersion::PHP_7_4, "typed properties");
        type_decl(p);
    }
    let mut hooks = false;
    loop {
        p.start(PROPERTY_ELEMENT);
        p.expect(VARIABLE);
        if p.eat(EQ) && !expr(p) {
            p.error("expected expression");
        }
        if p.at(L_CURLY) {
            property_hooks(p);
            hooks = true;
        }
        p.finish_node();
        if hooks || !p.eat(COMMA) {
            break;
        }
    }
    if !hooks {
        stmt_end(p);
    }
    p.finish_node();
}

fn property_hooks(p: &mut Parser) {
    let start = p.current_start();
    p.start(PROPERTY_HOOK_LIST);
    p.bump();
    while !p.at(R_CURLY) && !p.at(EOF) {
        let pos = p.pos();
        p.start(PROPERTY_HOOK);
        attributes(p);
        modifiers(p);
        p.eat(AMP);
        member_name(p);
        if p.at(L_PAREN) {
            param_list(p);
        }
        match p.current() {
            L_CURLY => block(p),
            FAT_ARROW => {
                p.bump();
                if !expr(p) {
                    p.error("expected expression");
                }
                p.expect(SEMICOLON);
            }
            _ => {
                p.expect(SEMICOLON);
            }
        }
        p.finish_node();
        if p.pos() == pos {
            p.err_and_bump("expected property hook");
        }
    }
    p.expect(R_CURLY);
    let range = TextRange::new(start, p.prev_end().max(start));
    p.require_version(PhpVersion::PHP_8_4, "property hooks", range);
    p.finish_node();
}

fn enum_case(p: &mut Parser) {
    p.start(ENUM_CASE);
    attributes(p);
    p.bump();
    member_name(p);
    if p.eat(EQ) && !expr(p) {
        p.error("expected expression");
    }
    stmt_end(p);
    p.finish_node();
}

fn trait_use(p: &mut Parser) {
    p.start(TRAIT_USE);
    attributes(p);
    p.bump();
    name_list(p);
    if p.at(L_CURLY) {
        p.start(TRAIT_ADAPTATION_LIST);
        p.bump();
        while !p.at(R_CURLY) && !p.at(EOF) {
            let pos = p.pos();
            trait_adaptation(p);
            if p.pos() == pos {
                p.err_and_bump("expected trait adaptation");
            }
        }
        p.expect(R_CURLY);
        p.finish_node();
    } else {
        stmt_end(p);
    }
    p.finish_node();
}

fn trait_adaptation(p: &mut Parser) {
    let cp = p.checkpoint();
    let qualified = p.at(BACKSLASH) || p.nth_at(1, COLON2) || (p.at(IDENT) && p.nth_at(1, BACKSLASH));
    if qualified {
        qualified_name(p);
        p.expect(COLON2);
    }
    if !at_identifier_like(p) {
        p.error("expected method name");
        return;
    }
    member_name(p);
    match p.current() {
        INSTEADOF_KW => {
            p.start_at(cp, TRAIT_PRECEDENCE);
            p.bump();
            name_list(p);
            p.finish_node();
        }
        AS_KW => {
            p.start_at(cp, TRAIT_ALIAS);
            p.bump();
            modifiers(p);
            if at_identifier_like(p) {
                member_name(p);
            }
            p.finish_node();
        }
        _ => p.error("expected `as` or `insteadof`"),
    }
    stmt_end(p);
}

pub(crate) fn param_list(p: &mut Parser) {
    p.start(PARAM_LIST);
    if !p.expect(L_PAREN) {
        p.finish_node();
        return;
    }
    while !p.at(R_PAREN) && !p.at(EOF) {
        let pos = p.pos();
        p.start(PARAM);
        attributes(p);
        if p.current().is_modifier() {
            p.require_version_here(PhpVersion::PHP_8_0, "constructor property promotion");
        }
        modifiers(p);
        if at_type_start(p) {
            type_decl(p);
        }
        p.eat(AMP);
        if p.at(DOT3) {
            p.require_version_here(PhpVersion::PHP_5_6, "variadic parameters");
            p.bump();
        }
        p.expect(VARIABLE);
        if p.eat(EQ) && !expr(p) {
            p.error("expected default value");
        }
        if p.at(L_CURLY) {
            property_hooks(p);
        }
        p.finish_node();
        if p.pos() == pos {
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
        if p.at(R_PAREN) {
            p.require_version(PhpVersion::PHP_8_0, "trailing commas in parameter lists", p.current_range());
        }
    }
    p.expect(R_PAREN);
    p.finish_node();
}

pub(crate) fn opt_return_type(p: &mut Parser) {
    if p.at(COLON) {
        p.start(RETURN_TYPE);
        p.require_version_here(PhpVersion::PHP_7_0, "return types");
        p.bump();
        type_decl(p);
        p.finish_node();
    }
}
