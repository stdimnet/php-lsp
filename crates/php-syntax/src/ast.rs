//! Typed AST layer on top of the untyped syntax tree.
//!
//! Every node type is a thin wrapper around a [`SyntaxNode`]; accessors
//! return `Option`s because the tree may be incomplete.

use crate::{SyntaxElement, SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

fn child<N: AstNode>(parent: &SyntaxNode) -> Option<N> {
    parent.children().find_map(N::cast)
}

fn children<N: AstNode>(parent: &SyntaxNode) -> impl Iterator<Item = N> {
    parent.children().filter_map(N::cast)
}

fn token(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    parent.children_with_tokens().filter_map(SyntaxElement::into_token).find(|t| t.kind() == kind)
}

macro_rules! ast_nodes {
    ($($name:ident => $kind:ident),* $(,)?) => {
        $(
            #[derive(Debug, Clone, PartialEq, Eq, Hash)]
            pub struct $name(SyntaxNode);

            impl AstNode for $name {
                fn can_cast(kind: SyntaxKind) -> bool {
                    kind == $kind
                }
                fn cast(node: SyntaxNode) -> Option<Self> {
                    if Self::can_cast(node.kind()) { Some(Self(node)) } else { None }
                }
                fn syntax(&self) -> &SyntaxNode {
                    &self.0
                }
            }
        )*
    };
}

ast_nodes! {
    SourceFile => SOURCE_FILE,
    Name => NAME,
    QualifiedName => QUALIFIED_NAME,
    MemberName => MEMBER_NAME,
    Block => BLOCK,
    NamespaceDecl => NAMESPACE_DECL,
    UseDecl => USE_DECL,
    UseGroup => USE_GROUP,
    UseClause => USE_CLAUSE,
    UseAlias => USE_ALIAS,
    ConstDecl => CONST_DECL,
    ConstElement => CONST_ELEMENT,
    FunctionDecl => FUNCTION_DECL,
    ClassDecl => CLASS_DECL,
    InterfaceDecl => INTERFACE_DECL,
    TraitDecl => TRAIT_DECL,
    EnumDecl => ENUM_DECL,
    AnonClass => ANON_CLASS,
    ModifierList => MODIFIER_LIST,
    ExtendsClause => EXTENDS_CLAUSE,
    ImplementsClause => IMPLEMENTS_CLAUSE,
    MemberList => MEMBER_LIST,
    MethodDecl => METHOD_DECL,
    PropertyDecl => PROPERTY_DECL,
    PropertyElement => PROPERTY_ELEMENT,
    ClassConstDecl => CLASS_CONST_DECL,
    EnumCase => ENUM_CASE,
    TraitUse => TRAIT_USE,
    ParamList => PARAM_LIST,
    Param => PARAM,
    ReturnType => RETURN_TYPE,
    ClosureExpr => CLOSURE_EXPR,
    ArrowFnExpr => ARROW_FN_EXPR,
    ArgList => ARG_LIST,
    Arg => ARG,
    AttributeGroup => ATTRIBUTE_GROUP,
    Attribute => ATTRIBUTE,
    VariableExpr => VARIABLE_EXPR,
    NameExpr => NAME_EXPR,
    CallExpr => CALL_EXPR,
    NewExpr => NEW_EXPR,
    MethodCallExpr => METHOD_CALL_EXPR,
    StaticCallExpr => STATIC_CALL_EXPR,
    PropertyFetchExpr => PROPERTY_FETCH_EXPR,
    StaticPropertyFetchExpr => STATIC_PROPERTY_FETCH_EXPR,
    ClassConstFetchExpr => CLASS_CONST_FETCH_EXPR,
    AssignExpr => ASSIGN_EXPR,
    StaticVar => STATIC_VAR,
    GlobalStmt => GLOBAL_STMT,
}

// ------------------------------------------------------------------ helpers

/// The doc comment (`/** ... */`) directly preceding `node`, if any.
pub fn doc_comment(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut el = node.prev_sibling_or_token();
    while let Some(e) = el {
        match e {
            rowan::NodeOrToken::Token(t) if t.kind() == WHITESPACE => el = t.prev_sibling_or_token(),
            rowan::NodeOrToken::Token(t) if t.kind() == DOC_COMMENT => return Some(t),
            _ => return None,
        }
    }
    None
}

/// Text of a node with all trivia removed (useful for names and types).
pub fn text_without_trivia(node: &SyntaxNode) -> String {
    node.descendants_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .filter(|t| !t.kind().is_trivia())
        .map(|t| t.text().to_string())
        .collect()
}

/// Kind of a class-like declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassLikeKind {
    Class,
    Interface,
    Trait,
    Enum,
}

/// Any class-like declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassLike {
    Class(ClassDecl),
    Interface(InterfaceDecl),
    Trait(TraitDecl),
    Enum(EnumDecl),
    /// `new class(...) extends ... { }` (not produced by `cast`, see
    /// [`ClassLike::anon`]).
    Anon(AnonClass),
}

impl AstNode for ClassLike {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL)
    }
    fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            CLASS_DECL => ClassLike::Class(ClassDecl(node)),
            INTERFACE_DECL => ClassLike::Interface(InterfaceDecl(node)),
            TRAIT_DECL => ClassLike::Trait(TraitDecl(node)),
            ENUM_DECL => ClassLike::Enum(EnumDecl(node)),
            _ => return None,
        })
    }
    fn syntax(&self) -> &SyntaxNode {
        match self {
            ClassLike::Class(n) => &n.0,
            ClassLike::Interface(n) => &n.0,
            ClassLike::Trait(n) => &n.0,
            ClassLike::Enum(n) => &n.0,
            ClassLike::Anon(n) => &n.0,
        }
    }
}

impl ClassLike {
    pub fn anon(node: SyntaxNode) -> Option<Self> {
        (node.kind() == ANON_CLASS).then(|| ClassLike::Anon(AnonClass(node)))
    }
    pub fn kind(&self) -> ClassLikeKind {
        match self {
            ClassLike::Class(_) | ClassLike::Anon(_) => ClassLikeKind::Class,
            ClassLike::Interface(_) => ClassLikeKind::Interface,
            ClassLike::Trait(_) => ClassLikeKind::Trait,
            ClassLike::Enum(_) => ClassLikeKind::Enum,
        }
    }
    pub fn name(&self) -> Option<Name> {
        child(self.syntax())
    }
    pub fn member_list(&self) -> Option<MemberList> {
        child(self.syntax())
    }
    pub fn modifiers(&self) -> Option<ModifierList> {
        child(self.syntax())
    }
    pub fn extends(&self) -> Option<ExtendsClause> {
        child(self.syntax())
    }
    pub fn implements(&self) -> Option<ImplementsClause> {
        child(self.syntax())
    }
    pub fn attributes(&self) -> impl Iterator<Item = AttributeGroup> {
        children(self.syntax())
    }
}

/// A member of a class-like body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Member {
    Method(MethodDecl),
    Property(PropertyDecl),
    Const(ClassConstDecl),
    EnumCase(EnumCase),
    TraitUse(TraitUse),
}

impl AstNode for Member {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, METHOD_DECL | PROPERTY_DECL | CLASS_CONST_DECL | ENUM_CASE | TRAIT_USE)
    }
    fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            METHOD_DECL => Member::Method(MethodDecl(node)),
            PROPERTY_DECL => Member::Property(PropertyDecl(node)),
            CLASS_CONST_DECL => Member::Const(ClassConstDecl(node)),
            ENUM_CASE => Member::EnumCase(EnumCase(node)),
            TRAIT_USE => Member::TraitUse(TraitUse(node)),
            _ => return None,
        })
    }
    fn syntax(&self) -> &SyntaxNode {
        match self {
            Member::Method(n) => &n.0,
            Member::Property(n) => &n.0,
            Member::Const(n) => &n.0,
            Member::EnumCase(n) => &n.0,
            Member::TraitUse(n) => &n.0,
        }
    }
}

// ------------------------------------------------------------------ accessors

impl SourceFile {
    pub fn statements(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children()
    }
}

impl Name {
    pub fn ident(&self) -> Option<SyntaxToken> {
        token(&self.0, IDENT)
    }
    pub fn text(&self) -> String {
        self.ident().map(|t| t.text().to_string()).unwrap_or_default()
    }
}

impl MemberName {
    pub fn ident(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| matches!(t.kind(), IDENT | VARIABLE))
    }
    /// The member name (without `$` for static properties).
    pub fn text(&self) -> Option<String> {
        let t = self.ident()?;
        Some(t.text().trim_start_matches('$').to_string())
    }
}

impl QualifiedName {
    /// The name as written, without whitespace/comments: `\Foo\Bar`.
    pub fn text(&self) -> String {
        text_without_trivia(&self.0)
    }
    pub fn is_fully_qualified(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| !t.kind().is_trivia())
            .is_some_and(|t| t.kind() == BACKSLASH)
    }
    pub fn is_relative(&self) -> bool {
        token(&self.0, NAMESPACE_KW).is_some()
    }
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        self.0.children_with_tokens().filter_map(SyntaxElement::into_token).filter(|t| t.kind() == IDENT)
    }
    pub fn last_segment(&self) -> Option<SyntaxToken> {
        self.segments().last()
    }
}

impl Block {
    pub fn statements(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children()
    }
}

impl NamespaceDecl {
    pub fn name(&self) -> Option<QualifiedName> {
        child(&self.0)
    }
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

/// `use function ...` / `use const ...`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UseKind {
    Class,
    Function,
    Const,
}

fn use_kind(node: &SyntaxNode) -> Option<UseKind> {
    for t in node.children_with_tokens().filter_map(SyntaxElement::into_token) {
        match t.kind() {
            FUNCTION_KW => return Some(UseKind::Function),
            CONST_KW => return Some(UseKind::Const),
            _ => {}
        }
    }
    None
}

impl UseDecl {
    pub fn kind(&self) -> UseKind {
        use_kind(&self.0).unwrap_or(UseKind::Class)
    }
    pub fn clauses(&self) -> impl Iterator<Item = UseClause> {
        children(&self.0)
    }
}

impl UseClause {
    /// Kind given inside a group (`use A\{function f}`).
    pub fn own_kind(&self) -> Option<UseKind> {
        use_kind(&self.0)
    }
    pub fn name(&self) -> Option<QualifiedName> {
        child(&self.0)
    }
    pub fn alias(&self) -> Option<Name> {
        child::<UseAlias>(&self.0).and_then(|a| child(&a.0))
    }
    pub fn group(&self) -> Option<UseGroup> {
        child(&self.0)
    }
}

impl UseGroup {
    pub fn clauses(&self) -> impl Iterator<Item = UseClause> {
        children(&self.0)
    }
}

impl ConstDecl {
    pub fn elements(&self) -> impl Iterator<Item = ConstElement> {
        children(&self.0)
    }
}

impl ConstElement {
    pub fn name_text(&self) -> Option<String> {
        if let Some(n) = child::<Name>(&self.0) {
            return Some(n.text());
        }
        child::<MemberName>(&self.0).and_then(|n| n.text())
    }
    pub fn name_node(&self) -> Option<SyntaxNode> {
        self.0.children().find(|c| matches!(c.kind(), NAME | MEMBER_NAME))
    }
    pub fn value(&self) -> Option<SyntaxNode> {
        self.0.children().find(|c| !matches!(c.kind(), NAME | MEMBER_NAME))
    }
}

impl FunctionDecl {
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }
    pub fn param_list(&self) -> Option<ParamList> {
        child(&self.0)
    }
    pub fn return_type(&self) -> Option<ReturnType> {
        child(&self.0)
    }
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
    pub fn by_ref(&self) -> bool {
        token(&self.0, AMP).is_some()
    }
}

impl ModifierList {
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        self.0.children_with_tokens().filter_map(SyntaxElement::into_token).filter(|t| t.kind().is_modifier())
    }
    pub fn has(&self, kind: SyntaxKind) -> bool {
        self.modifiers().any(|t| t.kind() == kind)
    }
}

impl ExtendsClause {
    pub fn names(&self) -> impl Iterator<Item = QualifiedName> {
        children(&self.0)
    }
}

impl ImplementsClause {
    pub fn names(&self) -> impl Iterator<Item = QualifiedName> {
        children(&self.0)
    }
}

impl MemberList {
    pub fn members(&self) -> impl Iterator<Item = Member> {
        children(&self.0)
    }
}

impl MethodDecl {
    pub fn name(&self) -> Option<MemberName> {
        child(&self.0)
    }
    pub fn modifiers(&self) -> Option<ModifierList> {
        child(&self.0)
    }
    pub fn param_list(&self) -> Option<ParamList> {
        child(&self.0)
    }
    pub fn return_type(&self) -> Option<ReturnType> {
        child(&self.0)
    }
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

impl PropertyDecl {
    pub fn modifiers(&self) -> Option<ModifierList> {
        child(&self.0)
    }
    /// The declared type node, if any.
    pub fn ty(&self) -> Option<SyntaxNode> {
        self.0.children().find(|c| is_type_kind(c.kind()))
    }
    pub fn elements(&self) -> impl Iterator<Item = PropertyElement> {
        children(&self.0)
    }
}

impl PropertyElement {
    pub fn variable(&self) -> Option<SyntaxToken> {
        token(&self.0, VARIABLE)
    }
    /// Property name without `$`.
    pub fn name(&self) -> Option<String> {
        self.variable().map(|t| t.text()[1..].to_string())
    }
    pub fn has_hooks(&self) -> bool {
        self.0.children().any(|c| c.kind() == PROPERTY_HOOK_LIST)
    }
}

impl ClassConstDecl {
    pub fn modifiers(&self) -> Option<ModifierList> {
        child(&self.0)
    }
    pub fn elements(&self) -> impl Iterator<Item = ConstElement> {
        children(&self.0)
    }
}

impl EnumCase {
    pub fn name(&self) -> Option<MemberName> {
        child(&self.0)
    }
}

impl TraitUse {
    pub fn names(&self) -> impl Iterator<Item = QualifiedName> {
        children(&self.0)
    }
}

impl ParamList {
    pub fn params(&self) -> impl Iterator<Item = Param> {
        children(&self.0)
    }
}

impl Param {
    pub fn variable(&self) -> Option<SyntaxToken> {
        token(&self.0, VARIABLE)
    }
    pub fn name(&self) -> Option<String> {
        self.variable().map(|t| t.text()[1..].to_string())
    }
    pub fn ty(&self) -> Option<SyntaxNode> {
        self.0.children().find(|c| is_type_kind(c.kind()))
    }
    pub fn modifiers(&self) -> Option<ModifierList> {
        child(&self.0)
    }
    pub fn is_variadic(&self) -> bool {
        token(&self.0, DOT3).is_some()
    }
    pub fn by_ref(&self) -> bool {
        token(&self.0, AMP).is_some()
    }
    pub fn default_value(&self) -> Option<SyntaxNode> {
        let mut after_eq = false;
        for el in self.0.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == EQ => after_eq = true,
                rowan::NodeOrToken::Node(n) if after_eq => return Some(n),
                _ => {}
            }
        }
        None
    }
}

impl ReturnType {
    pub fn ty(&self) -> Option<SyntaxNode> {
        self.0.children().find(|c| is_type_kind(c.kind()))
    }
}

impl Attribute {
    pub fn name(&self) -> Option<QualifiedName> {
        child(&self.0)
    }
}

impl VariableExpr {
    /// Variable name without `$`.
    pub fn name(&self) -> Option<String> {
        let t = self
            .0
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|t| matches!(t.kind(), VARIABLE | IDENT))?;
        Some(t.text().trim_start_matches('$').to_string())
    }
}

pub fn is_type_kind(kind: SyntaxKind) -> bool {
    matches!(kind, NAMED_TYPE | NULLABLE_TYPE | UNION_TYPE | INTERSECTION_TYPE | PAREN_TYPE)
}
