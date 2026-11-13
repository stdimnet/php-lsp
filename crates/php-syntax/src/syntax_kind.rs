//! All token and node kinds of the PHP syntax tree.

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u16)]
pub enum SyntaxKind {
    // ---- special ----
    TOMBSTONE = 0,
    EOF,
    ERROR_TOKEN,

    // ---- trivia ----
    WHITESPACE,
    COMMENT,
    DOC_COMMENT,
    /// `<?php` or `<?` including one following whitespace character.
    OPEN_TAG,

    // ---- template / html ----
    INLINE_HTML,
    /// `<?=`
    OPEN_TAG_WITH_ECHO,
    /// `?>` including one following newline.
    CLOSE_TAG,
    /// Everything after `__halt_compiler();`
    HALT_COMPILER_DATA,

    // ---- literals ----
    IDENT,
    VARIABLE,
    INT_NUMBER,
    FLOAT_NUMBER,
    /// A complete string without interpolation (`'..'` or `".."`).
    STRING,
    /// A literal part inside an interpolated string / heredoc / shell command.
    ENCAPSED_TEXT,
    /// `<<<LABEL\n`, `<<<"LABEL"\n` or `<<<'LABEL'\n`
    HEREDOC_START,
    /// The closing label of a heredoc / nowdoc (including indentation).
    HEREDOC_END,
    /// `(int)`, `(string)`, ...
    CAST,

    // ---- punctuation ----
    SEMICOLON,
    COMMA,
    L_PAREN,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_CURLY,
    R_CURLY,
    /// `${` inside strings
    DOLLAR_CURLY,
    DOT,
    DOT3,
    ARROW,
    NULLSAFE_ARROW,
    FAT_ARROW,
    COLON2,
    COLON,
    QUESTION,
    QUESTION2,
    QUESTION2_EQ,
    EQ,
    PLUS_EQ,
    MINUS_EQ,
    STAR_EQ,
    SLASH_EQ,
    DOT_EQ,
    PERCENT_EQ,
    STAR2_EQ,
    AMP_EQ,
    PIPE_EQ,
    CARET_EQ,
    SHL_EQ,
    SHR_EQ,
    EQ2,
    EQ3,
    NEQ,
    NEQ2,
    /// `<>`
    LT_GT,
    LT,
    GT,
    LT_EQ,
    GT_EQ,
    SPACESHIP,
    PLUS,
    MINUS,
    STAR,
    SLASH,
    PERCENT,
    STAR2,
    PLUS2,
    MINUS2,
    BANG,
    TILDE,
    AMP,
    AMP2,
    PIPE,
    PIPE2,
    /// `|>` pipe operator (PHP 8.5)
    PIPE_GT,
    CARET,
    SHL,
    SHR,
    AT,
    DOLLAR,
    BACKSLASH,
    BACKTICK,
    DOUBLE_QUOTE,
    /// `#[`
    HASH_BRACK,

    // ---- keywords ----
    ABSTRACT_KW,
    AND_KW,
    ARRAY_KW,
    AS_KW,
    BREAK_KW,
    CALLABLE_KW,
    CASE_KW,
    CATCH_KW,
    CLASS_KW,
    CLONE_KW,
    CONST_KW,
    CONTINUE_KW,
    DECLARE_KW,
    DEFAULT_KW,
    DIE_KW,
    DO_KW,
    ECHO_KW,
    ELSE_KW,
    ELSEIF_KW,
    EMPTY_KW,
    ENDDECLARE_KW,
    ENDFOR_KW,
    ENDFOREACH_KW,
    ENDIF_KW,
    ENDSWITCH_KW,
    ENDWHILE_KW,
    EVAL_KW,
    EXIT_KW,
    EXTENDS_KW,
    FINAL_KW,
    FINALLY_KW,
    FN_KW,
    FOR_KW,
    FOREACH_KW,
    FUNCTION_KW,
    GLOBAL_KW,
    GOTO_KW,
    HALT_COMPILER_KW,
    IF_KW,
    IMPLEMENTS_KW,
    INCLUDE_KW,
    INCLUDE_ONCE_KW,
    INSTANCEOF_KW,
    INSTEADOF_KW,
    INTERFACE_KW,
    ISSET_KW,
    LIST_KW,
    MATCH_KW,
    NAMESPACE_KW,
    NEW_KW,
    OR_KW,
    PRINT_KW,
    PRIVATE_KW,
    PROTECTED_KW,
    PUBLIC_KW,
    READONLY_KW,
    REQUIRE_KW,
    REQUIRE_ONCE_KW,
    RETURN_KW,
    STATIC_KW,
    SWITCH_KW,
    THROW_KW,
    TRAIT_KW,
    TRY_KW,
    UNSET_KW,
    USE_KW,
    VAR_KW,
    WHILE_KW,
    XOR_KW,
    YIELD_KW,
    // magic constants
    CLASS_C_KW,
    DIR_C_KW,
    FILE_C_KW,
    FUNCTION_C_KW,
    LINE_C_KW,
    METHOD_C_KW,
    NAMESPACE_C_KW,
    TRAIT_C_KW,
    PROPERTY_C_KW,

    // ================= nodes =================
    SOURCE_FILE,
    ERROR,

    // names
    /// Name of a declaration (single identifier).
    NAME,
    /// A (possibly qualified) name reference: `Foo`, `\Foo\Bar`, `namespace\Foo`.
    QUALIFIED_NAME,
    /// Member name after `->`, `::`, or in declarations of members (identifier or keyword).
    MEMBER_NAME,

    // statements
    INLINE_HTML_STMT,
    ECHO_STMT,
    EXPR_STMT,
    EMPTY_STMT,
    BLOCK,
    IF_STMT,
    ELSE_IF_CLAUSE,
    ELSE_CLAUSE,
    WHILE_STMT,
    DO_WHILE_STMT,
    FOR_STMT,
    FOR_INIT,
    FOR_COND,
    FOR_STEP,
    FOREACH_STMT,
    FOREACH_KEY,
    FOREACH_VALUE,
    SWITCH_STMT,
    SWITCH_BODY,
    CASE_CLAUSE,
    DEFAULT_CLAUSE,
    BREAK_STMT,
    CONTINUE_STMT,
    RETURN_STMT,
    GLOBAL_STMT,
    STATIC_STMT,
    STATIC_VAR,
    UNSET_STMT,
    THROW_STMT,
    TRY_STMT,
    CATCH_CLAUSE,
    CATCH_TYPES,
    FINALLY_CLAUSE,
    LABEL_STMT,
    GOTO_STMT,
    DECLARE_STMT,
    DECLARE_DIRECTIVE,
    HALT_COMPILER_STMT,
    /// Alternative syntax body: `: ... endif;`
    STMT_LIST,

    // declarations
    NAMESPACE_DECL,
    USE_DECL,
    USE_GROUP,
    USE_CLAUSE,
    USE_ALIAS,
    CONST_DECL,
    CONST_ELEMENT,
    FUNCTION_DECL,
    CLASS_DECL,
    INTERFACE_DECL,
    TRAIT_DECL,
    ENUM_DECL,
    ENUM_BACKING_TYPE,
    MODIFIER_LIST,
    EXTENDS_CLAUSE,
    IMPLEMENTS_CLAUSE,
    MEMBER_LIST,
    METHOD_DECL,
    PROPERTY_DECL,
    PROPERTY_ELEMENT,
    PROPERTY_HOOK_LIST,
    PROPERTY_HOOK,
    CLASS_CONST_DECL,
    ENUM_CASE,
    TRAIT_USE,
    TRAIT_ADAPTATION_LIST,
    TRAIT_ALIAS,
    TRAIT_PRECEDENCE,
    PARAM_LIST,
    PARAM,
    RETURN_TYPE,
    CLOSURE_USE,
    CLOSURE_USE_VAR,
    ATTRIBUTE_GROUP,
    ATTRIBUTE,

    // types
    NAMED_TYPE,
    NULLABLE_TYPE,
    UNION_TYPE,
    INTERSECTION_TYPE,
    PAREN_TYPE,

    // expressions
    LITERAL,
    INTERPOLATED_STRING,
    HEREDOC,
    SHELL_EXEC,
    INTERPOLATION,
    VARIABLE_EXPR,
    DYNAMIC_VARIABLE_EXPR,
    NAME_EXPR,
    ARRAY_EXPR,
    ARRAY_ELEMENT,
    LIST_EXPR,
    CALL_EXPR,
    ARG_LIST,
    ARG,
    NEW_EXPR,
    ANON_CLASS,
    PROPERTY_FETCH_EXPR,
    METHOD_CALL_EXPR,
    STATIC_PROPERTY_FETCH_EXPR,
    STATIC_CALL_EXPR,
    CLASS_CONST_FETCH_EXPR,
    INDEX_EXPR,
    BIN_EXPR,
    PREFIX_EXPR,
    POSTFIX_EXPR,
    ASSIGN_EXPR,
    TERNARY_EXPR,
    CAST_EXPR,
    PAREN_EXPR,
    CLOSURE_EXPR,
    ARROW_FN_EXPR,
    MATCH_EXPR,
    MATCH_ARM,
    MATCH_CONDITIONS,
    ISSET_EXPR,
    EMPTY_EXPR,
    EVAL_EXPR,
    INCLUDE_EXPR,
    EXIT_EXPR,
    PRINT_EXPR,
    CLONE_EXPR,
    INSTANCEOF_EXPR,
    YIELD_EXPR,
    YIELD_FROM_EXPR,
    THROW_EXPR,
    MAGIC_CONST_EXPR,

    #[doc(hidden)]
    __LAST,
}

use SyntaxKind::*;

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, WHITESPACE | COMMENT | DOC_COMMENT | OPEN_TAG)
    }

    pub fn is_keyword(self) -> bool {
        (ABSTRACT_KW as u16..=PROPERTY_C_KW as u16).contains(&(self as u16))
    }

    pub fn is_magic_constant(self) -> bool {
        (CLASS_C_KW as u16..=PROPERTY_C_KW as u16).contains(&(self as u16))
    }

    pub fn is_token(self) -> bool {
        (self as u16) < SOURCE_FILE as u16
    }

    pub fn is_node(self) -> bool {
        !self.is_token()
    }

    pub fn is_literal_token(self) -> bool {
        matches!(self, INT_NUMBER | FLOAT_NUMBER | STRING)
    }

    pub fn is_assign_op(self) -> bool {
        matches!(
            self,
            EQ | PLUS_EQ
                | MINUS_EQ
                | STAR_EQ
                | SLASH_EQ
                | DOT_EQ
                | PERCENT_EQ
                | STAR2_EQ
                | AMP_EQ
                | PIPE_EQ
                | CARET_EQ
                | SHL_EQ
                | SHR_EQ
                | QUESTION2_EQ
        )
    }

    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            PUBLIC_KW | PROTECTED_KW | PRIVATE_KW | STATIC_KW | ABSTRACT_KW | FINAL_KW | READONLY_KW | VAR_KW
        )
    }

    /// Looks up a (case insensitive) keyword.
    pub fn from_keyword(ident: &str) -> Option<SyntaxKind> {
        let mut buf = [0u8; 20];
        if ident.len() > buf.len() {
            return None;
        }
        let lower = &mut buf[..ident.len()];
        lower.copy_from_slice(ident.as_bytes());
        lower.make_ascii_lowercase();
        let kw = match &*lower {
            b"abstract" => ABSTRACT_KW,
            b"and" => AND_KW,
            b"array" => ARRAY_KW,
            b"as" => AS_KW,
            b"break" => BREAK_KW,
            b"callable" => CALLABLE_KW,
            b"case" => CASE_KW,
            b"catch" => CATCH_KW,
            b"class" => CLASS_KW,
            b"clone" => CLONE_KW,
            b"const" => CONST_KW,
            b"continue" => CONTINUE_KW,
            b"declare" => DECLARE_KW,
            b"default" => DEFAULT_KW,
            b"die" => DIE_KW,
            b"do" => DO_KW,
            b"echo" => ECHO_KW,
            b"else" => ELSE_KW,
            b"elseif" => ELSEIF_KW,
            b"empty" => EMPTY_KW,
            b"enddeclare" => ENDDECLARE_KW,
            b"endfor" => ENDFOR_KW,
            b"endforeach" => ENDFOREACH_KW,
            b"endif" => ENDIF_KW,
            b"endswitch" => ENDSWITCH_KW,
            b"endwhile" => ENDWHILE_KW,
            b"eval" => EVAL_KW,
            b"exit" => EXIT_KW,
            b"extends" => EXTENDS_KW,
            b"final" => FINAL_KW,
            b"finally" => FINALLY_KW,
            b"fn" => FN_KW,
            b"for" => FOR_KW,
            b"foreach" => FOREACH_KW,
            b"function" => FUNCTION_KW,
            b"global" => GLOBAL_KW,
            b"goto" => GOTO_KW,
            b"__halt_compiler" => HALT_COMPILER_KW,
            b"if" => IF_KW,
            b"implements" => IMPLEMENTS_KW,
            b"include" => INCLUDE_KW,
            b"include_once" => INCLUDE_ONCE_KW,
            b"instanceof" => INSTANCEOF_KW,
            b"insteadof" => INSTEADOF_KW,
            b"interface" => INTERFACE_KW,
            b"isset" => ISSET_KW,
            b"list" => LIST_KW,
            b"match" => MATCH_KW,
            b"namespace" => NAMESPACE_KW,
            b"new" => NEW_KW,
            b"or" => OR_KW,
            b"print" => PRINT_KW,
            b"private" => PRIVATE_KW,
            b"protected" => PROTECTED_KW,
            b"public" => PUBLIC_KW,
            b"readonly" => READONLY_KW,
            b"require" => REQUIRE_KW,
            b"require_once" => REQUIRE_ONCE_KW,
            b"return" => RETURN_KW,
            b"static" => STATIC_KW,
            b"switch" => SWITCH_KW,
            b"throw" => THROW_KW,
            b"trait" => TRAIT_KW,
            b"try" => TRY_KW,
            b"unset" => UNSET_KW,
            b"use" => USE_KW,
            b"var" => VAR_KW,
            b"while" => WHILE_KW,
            b"xor" => XOR_KW,
            b"yield" => YIELD_KW,
            b"__class__" => CLASS_C_KW,
            b"__dir__" => DIR_C_KW,
            b"__file__" => FILE_C_KW,
            b"__function__" => FUNCTION_C_KW,
            b"__line__" => LINE_C_KW,
            b"__method__" => METHOD_C_KW,
            b"__namespace__" => NAMESPACE_C_KW,
            b"__trait__" => TRAIT_C_KW,
            b"__property__" => PROPERTY_C_KW,
            _ => return None,
        };
        Some(kw)
    }
}

impl From<u16> for SyntaxKind {
    fn from(d: u16) -> SyntaxKind {
        assert!(d < (__LAST as u16));
        // SAFETY: SyntaxKind is repr(u16) and `d` is in range.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(d) }
    }
}

impl From<SyntaxKind> for u16 {
    fn from(k: SyntaxKind) -> u16 {
        k as u16
    }
}

/// Shorthand for token kinds: `T![;]`, `T![function]`, ...
#[macro_export]
macro_rules! T {
    [;] => { $crate::SyntaxKind::SEMICOLON };
    [,] => { $crate::SyntaxKind::COMMA };
    ['('] => { $crate::SyntaxKind::L_PAREN };
    [')'] => { $crate::SyntaxKind::R_PAREN };
    ['['] => { $crate::SyntaxKind::L_BRACK };
    [']'] => { $crate::SyntaxKind::R_BRACK };
    ['{'] => { $crate::SyntaxKind::L_CURLY };
    ['}'] => { $crate::SyntaxKind::R_CURLY };
    [.] => { $crate::SyntaxKind::DOT };
    [...] => { $crate::SyntaxKind::DOT3 };
    [->] => { $crate::SyntaxKind::ARROW };
    [?->] => { $crate::SyntaxKind::NULLSAFE_ARROW };
    [=>] => { $crate::SyntaxKind::FAT_ARROW };
    [::] => { $crate::SyntaxKind::COLON2 };
    [:] => { $crate::SyntaxKind::COLON };
    [?] => { $crate::SyntaxKind::QUESTION };
    [??] => { $crate::SyntaxKind::QUESTION2 };
    [=] => { $crate::SyntaxKind::EQ };
    [==] => { $crate::SyntaxKind::EQ2 };
    [<] => { $crate::SyntaxKind::LT };
    [>] => { $crate::SyntaxKind::GT };
    [+] => { $crate::SyntaxKind::PLUS };
    [-] => { $crate::SyntaxKind::MINUS };
    [*] => { $crate::SyntaxKind::STAR };
    [/] => { $crate::SyntaxKind::SLASH };
    [%] => { $crate::SyntaxKind::PERCENT };
    [++] => { $crate::SyntaxKind::PLUS2 };
    [--] => { $crate::SyntaxKind::MINUS2 };
    [!] => { $crate::SyntaxKind::BANG };
    [~] => { $crate::SyntaxKind::TILDE };
    [&] => { $crate::SyntaxKind::AMP };
    [|] => { $crate::SyntaxKind::PIPE };
    [@] => { $crate::SyntaxKind::AT };
    [$] => { $crate::SyntaxKind::DOLLAR };
    [ '\\' ] => { $crate::SyntaxKind::BACKSLASH };
    ['"'] => { $crate::SyntaxKind::DOUBLE_QUOTE };
    ['`'] => { $crate::SyntaxKind::BACKTICK };
    [abstract] => { $crate::SyntaxKind::ABSTRACT_KW };
    [and] => { $crate::SyntaxKind::AND_KW };
    [array] => { $crate::SyntaxKind::ARRAY_KW };
    [as] => { $crate::SyntaxKind::AS_KW };
    [break] => { $crate::SyntaxKind::BREAK_KW };
    [callable] => { $crate::SyntaxKind::CALLABLE_KW };
    [case] => { $crate::SyntaxKind::CASE_KW };
    [catch] => { $crate::SyntaxKind::CATCH_KW };
    [class] => { $crate::SyntaxKind::CLASS_KW };
    [clone] => { $crate::SyntaxKind::CLONE_KW };
    [const] => { $crate::SyntaxKind::CONST_KW };
    [continue] => { $crate::SyntaxKind::CONTINUE_KW };
    [declare] => { $crate::SyntaxKind::DECLARE_KW };
    [default] => { $crate::SyntaxKind::DEFAULT_KW };
    [do] => { $crate::SyntaxKind::DO_KW };
    [echo] => { $crate::SyntaxKind::ECHO_KW };
    [else] => { $crate::SyntaxKind::ELSE_KW };
    [elseif] => { $crate::SyntaxKind::ELSEIF_KW };
    [extends] => { $crate::SyntaxKind::EXTENDS_KW };
    [final] => { $crate::SyntaxKind::FINAL_KW };
    [finally] => { $crate::SyntaxKind::FINALLY_KW };
    [fn] => { $crate::SyntaxKind::FN_KW };
    [for] => { $crate::SyntaxKind::FOR_KW };
    [foreach] => { $crate::SyntaxKind::FOREACH_KW };
    [function] => { $crate::SyntaxKind::FUNCTION_KW };
    [if] => { $crate::SyntaxKind::IF_KW };
    [implements] => { $crate::SyntaxKind::IMPLEMENTS_KW };
    [instanceof] => { $crate::SyntaxKind::INSTANCEOF_KW };
    [insteadof] => { $crate::SyntaxKind::INSTEADOF_KW };
    [interface] => { $crate::SyntaxKind::INTERFACE_KW };
    [list] => { $crate::SyntaxKind::LIST_KW };
    [match] => { $crate::SyntaxKind::MATCH_KW };
    [namespace] => { $crate::SyntaxKind::NAMESPACE_KW };
    [new] => { $crate::SyntaxKind::NEW_KW };
    [or] => { $crate::SyntaxKind::OR_KW };
    [private] => { $crate::SyntaxKind::PRIVATE_KW };
    [protected] => { $crate::SyntaxKind::PROTECTED_KW };
    [public] => { $crate::SyntaxKind::PUBLIC_KW };
    [readonly] => { $crate::SyntaxKind::READONLY_KW };
    [return] => { $crate::SyntaxKind::RETURN_KW };
    [static] => { $crate::SyntaxKind::STATIC_KW };
    [switch] => { $crate::SyntaxKind::SWITCH_KW };
    [throw] => { $crate::SyntaxKind::THROW_KW };
    [trait] => { $crate::SyntaxKind::TRAIT_KW };
    [try] => { $crate::SyntaxKind::TRY_KW };
    [use] => { $crate::SyntaxKind::USE_KW };
    [var] => { $crate::SyntaxKind::VAR_KW };
    [while] => { $crate::SyntaxKind::WHILE_KW };
    [xor] => { $crate::SyntaxKind::XOR_KW };
    [yield] => { $crate::SyntaxKind::YIELD_KW };
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(k: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(k as u16)
    }
}
