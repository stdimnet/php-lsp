//! Statements.

use super::exprs::{at_expr_start, expr, throw_expr};
use super::{decls, name};
use crate::parser::Parser;
use crate::{PhpVersion, SyntaxKind, SyntaxKind::*};

pub(crate) fn statement(p: &mut Parser) {
    match p.current() {
        INLINE_HTML => single(p, INLINE_HTML_STMT),
        CLOSE_TAG | SEMICOLON => single(p, EMPTY_STMT),
        OPEN_TAG_WITH_ECHO => {
            p.start(ECHO_STMT);
            p.bump();
            expr_list(p);
            stmt_end(p);
            p.finish_node();
        }
        L_CURLY => block(p),
        IF_KW => if_stmt(p),
        WHILE_KW => while_stmt(p),
        DO_KW => do_while_stmt(p),
        FOR_KW => for_stmt(p),
        FOREACH_KW => foreach_stmt(p),
        SWITCH_KW => switch_stmt(p),
        BREAK_KW => keyword_opt_expr(p, BREAK_STMT),
        CONTINUE_KW => keyword_opt_expr(p, CONTINUE_STMT),
        RETURN_KW => keyword_opt_expr(p, RETURN_STMT),
        GLOBAL_KW => global_stmt(p),
        STATIC_KW if p.nth_at(1, VARIABLE) => static_stmt(p),
        ECHO_KW => {
            p.start(ECHO_STMT);
            p.bump();
            expr_list(p);
            stmt_end(p);
            p.finish_node();
        }
        UNSET_KW => unset_stmt(p),
        THROW_KW => {
            p.start(EXPR_STMT);
            throw_expr(p);
            stmt_end(p);
            p.finish_node();
        }
        TRY_KW => try_stmt(p),
        GOTO_KW => {
            p.start(GOTO_STMT);
            p.bump();
            name(p);
            stmt_end(p);
            p.finish_node();
        }
        DECLARE_KW => declare_stmt(p),
        NAMESPACE_KW if !p.nth_at(1, BACKSLASH) => decls::namespace_decl(p),
        USE_KW => decls::use_decl(p),
        CONST_KW => decls::const_decl(p),
        HALT_COMPILER_KW => {
            p.start(HALT_COMPILER_STMT);
            p.bump();
            p.expect(L_PAREN);
            p.expect(R_PAREN);
            if !p.eat(SEMICOLON) {
                p.expect(CLOSE_TAG);
            }
            p.eat(HALT_COMPILER_DATA);
            p.finish_node();
        }
        IDENT if p.nth_at(1, COLON) => {
            p.start(LABEL_STMT);
            name(p);
            p.bump();
            p.finish_node();
        }
        _ => {
            if decls::try_declaration(p) {
                return;
            }
            expr_stmt(p);
        }
    }
}

fn single(p: &mut Parser, kind: SyntaxKind) {
    p.start(kind);
    p.bump();
    p.finish_node();
}

fn expr_stmt(p: &mut Parser) {
    if !at_expr_start(p) {
        return;
    }
    p.start(EXPR_STMT);
    expr(p);
    stmt_end(p);
    p.finish_node();
}

/// `;` or `?>` (which implies a semicolon).
pub(crate) fn stmt_end(p: &mut Parser) {
    if p.eat(SEMICOLON) || p.eat(CLOSE_TAG) {
        return;
    }
    p.error("expected `;`");
}

fn expr_list(p: &mut Parser) {
    loop {
        if !expr(p) {
            p.error("expected expression");
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
}

pub(crate) fn block(p: &mut Parser) {
    p.start(BLOCK);
    if p.expect(L_CURLY) {
        statements_until(p, &[R_CURLY]);
        p.expect(R_CURLY);
    }
    p.finish_node();
}

/// Parses statements until one of `stop` (or EOF) is reached.
pub(crate) fn statements_until(p: &mut Parser, stop: &[SyntaxKind]) {
    while !p.at(EOF) && !p.at_any(stop) {
        let pos = p.pos();
        statement(p);
        if p.pos() == pos {
            if p.at(R_CURLY) && !stop.contains(&R_CURLY) {
                // A `}` closing an outer block; let the caller handle it.
                p.error("expected statement");
                return;
            }
            p.err_and_bump("expected statement");
        }
    }
}

/// Body of a control structure: a statement or, with alternative syntax,
/// `: statements endXXX;`.
fn body(p: &mut Parser, end: SyntaxKind) {
    if p.at(COLON) {
        p.start(STMT_LIST);
        p.bump();
        statements_until(p, &[end]);
        p.finish_node();
        p.expect(end);
        stmt_end(p);
    } else {
        statement_or_error(p);
    }
}

fn statement_or_error(p: &mut Parser) {
    let pos = p.pos();
    statement(p);
    if p.pos() == pos {
        p.error("expected statement");
    }
}

fn paren_cond(p: &mut Parser) {
    if p.expect(L_PAREN) {
        if !expr(p) {
            p.error("expected expression");
        }
        p.expect(R_PAREN);
    }
}

fn if_stmt(p: &mut Parser) {
    p.start(IF_STMT);
    p.bump();
    paren_cond(p);
    if p.at(COLON) {
        p.start(STMT_LIST);
        p.bump();
        statements_until(p, &[ELSEIF_KW, ELSE_KW, ENDIF_KW]);
        p.finish_node();
        loop {
            match p.current() {
                ELSEIF_KW => {
                    p.start(ELSE_IF_CLAUSE);
                    p.bump();
                    paren_cond(p);
                    p.start(STMT_LIST);
                    p.expect(COLON);
                    statements_until(p, &[ELSEIF_KW, ELSE_KW, ENDIF_KW]);
                    p.finish_node();
                    p.finish_node();
                }
                ELSE_KW => {
                    p.start(ELSE_CLAUSE);
                    p.bump();
                    p.start(STMT_LIST);
                    p.expect(COLON);
                    statements_until(p, &[ELSEIF_KW, ELSE_KW, ENDIF_KW]);
                    p.finish_node();
                    p.finish_node();
                }
                _ => break,
            }
        }
        p.expect(ENDIF_KW);
        stmt_end(p);
    } else {
        statement_or_error(p);
        loop {
            match p.current() {
                ELSEIF_KW => {
                    p.start(ELSE_IF_CLAUSE);
                    p.bump();
                    paren_cond(p);
                    statement_or_error(p);
                    p.finish_node();
                }
                ELSE_KW => {
                    p.start(ELSE_CLAUSE);
                    p.bump();
                    statement_or_error(p);
                    p.finish_node();
                    break;
                }
                _ => break,
            }
        }
    }
    p.finish_node();
}

fn while_stmt(p: &mut Parser) {
    p.start(WHILE_STMT);
    p.bump();
    paren_cond(p);
    body(p, ENDWHILE_KW);
    p.finish_node();
}

fn do_while_stmt(p: &mut Parser) {
    p.start(DO_WHILE_STMT);
    p.bump();
    statement_or_error(p);
    p.expect(WHILE_KW);
    paren_cond(p);
    stmt_end(p);
    p.finish_node();
}

fn for_stmt(p: &mut Parser) {
    p.start(FOR_STMT);
    p.bump();
    if p.expect(L_PAREN) {
        for (kind, end) in [(FOR_INIT, SEMICOLON), (FOR_COND, SEMICOLON), (FOR_STEP, R_PAREN)] {
            p.start(kind);
            while !p.at(end) && !p.at(EOF) {
                if !expr(p) {
                    p.error("expected expression");
                    break;
                }
                if !p.eat(COMMA) {
                    break;
                }
            }
            p.finish_node();
            p.expect(end);
        }
    }
    body(p, ENDFOR_KW);
    p.finish_node();
}

fn foreach_stmt(p: &mut Parser) {
    p.start(FOREACH_STMT);
    p.bump();
    if p.expect(L_PAREN) {
        if !expr(p) {
            p.error("expected expression");
        }
        p.expect(AS_KW);
        let cp = p.checkpoint();
        p.eat(AMP);
        if !expr(p) {
            p.error("expected variable");
        }
        if p.at(FAT_ARROW) {
            p.start_at(cp, FOREACH_KEY);
            p.finish_node();
            p.bump();
            p.start(FOREACH_VALUE);
            p.eat(AMP);
            if !expr(p) {
                p.error("expected variable");
            }
            p.finish_node();
        } else {
            p.start_at(cp, FOREACH_VALUE);
            p.finish_node();
        }
        p.expect(R_PAREN);
    }
    body(p, ENDFOREACH_KW);
    p.finish_node();
}

fn switch_stmt(p: &mut Parser) {
    p.start(SWITCH_STMT);
    p.bump();
    paren_cond(p);
    p.start(SWITCH_BODY);
    let alt = p.at(COLON);
    let end = if alt { ENDSWITCH_KW } else { R_CURLY };
    if alt || p.at(L_CURLY) {
        p.bump();
        p.eat(SEMICOLON);
        while !p.at(end) && !p.at(EOF) {
            match p.current() {
                CASE_KW | DEFAULT_KW => {
                    let kind = if p.at(CASE_KW) { CASE_CLAUSE } else { DEFAULT_CLAUSE };
                    p.start(kind);
                    p.bump();
                    if kind == CASE_CLAUSE && !expr(p) {
                        p.error("expected expression");
                    }
                    if !p.eat(COLON) && !p.eat(SEMICOLON) {
                        p.error("expected `:`");
                    }
                    statements_until(p, &[CASE_KW, DEFAULT_KW, end]);
                    p.finish_node();
                }
                INLINE_HTML | CLOSE_TAG => statement(p),
                _ => p.err_and_bump("expected `case` or `default`"),
            }
        }
        p.expect(end);
    } else {
        p.error("expected `{`");
    }
    p.finish_node();
    if alt {
        stmt_end(p);
    }
    p.finish_node();
}

fn keyword_opt_expr(p: &mut Parser, kind: SyntaxKind) {
    p.start(kind);
    p.bump();
    if at_expr_start(p) {
        expr(p);
    }
    stmt_end(p);
    p.finish_node();
}

fn global_stmt(p: &mut Parser) {
    p.start(GLOBAL_STMT);
    p.bump();
    expr_list(p);
    stmt_end(p);
    p.finish_node();
}

fn static_stmt(p: &mut Parser) {
    p.start(STATIC_STMT);
    p.bump();
    loop {
        p.start(STATIC_VAR);
        p.start(VARIABLE_EXPR);
        p.expect(VARIABLE);
        p.finish_node();
        if p.eat(EQ) && !expr(p) {
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

fn unset_stmt(p: &mut Parser) {
    p.start(UNSET_STMT);
    p.bump();
    if p.expect(L_PAREN) {
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
    stmt_end(p);
    p.finish_node();
}

fn try_stmt(p: &mut Parser) {
    p.start(TRY_STMT);
    p.bump();
    block(p);
    let mut has_clause = false;
    while p.at(CATCH_KW) {
        has_clause = true;
        p.start(CATCH_CLAUSE);
        p.bump();
        if p.expect(L_PAREN) {
            p.start(CATCH_TYPES);
            loop {
                if super::at_name_start(p) {
                    super::qualified_name(p);
                } else {
                    p.error("expected exception type");
                    break;
                }
                if p.at(PIPE) {
                    p.require_version_here(PhpVersion::PHP_7_1, "multi-catch");
                    p.bump();
                } else {
                    break;
                }
            }
            p.finish_node();
            if !p.eat(VARIABLE) {
                if p.at(R_PAREN) {
                    p.require_version(PhpVersion::PHP_8_0, "catch without variable", p.current_range());
                } else {
                    p.error("expected variable");
                }
            }
            p.expect(R_PAREN);
        }
        block(p);
        p.finish_node();
    }
    if p.at(FINALLY_KW) {
        has_clause = true;
        p.start(FINALLY_CLAUSE);
        p.bump();
        block(p);
        p.finish_node();
    }
    if !has_clause {
        p.error("expected `catch` or `finally`");
    }
    p.finish_node();
}

fn declare_stmt(p: &mut Parser) {
    p.start(DECLARE_STMT);
    p.bump();
    if p.expect(L_PAREN) {
        while !p.at(R_PAREN) && !p.at(EOF) {
            p.start(DECLARE_DIRECTIVE);
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
        p.expect(R_PAREN);
    }
    if p.at(SEMICOLON) || p.at(CLOSE_TAG) {
        p.bump();
    } else {
        body(p, ENDDECLARE_KW);
    }
    p.finish_node();
}
