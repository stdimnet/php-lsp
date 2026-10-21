//! Code completion.

use std::collections::HashSet;

use php_hir::symbols::{FunctionDef, Visibility};
use php_hir::{Fqn, NamespaceScope};
use php_syntax::ast::ClassLikeKind;
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::render;
use crate::sema::{in_variable_scope, Sema};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    Class,
    Interface,
    Trait,
    Enum,
    EnumMember,
    Method,
    Function,
    Property,
    Constant,
    Variable,
    Keyword,
    Namespace,
    Snippet,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// Markdown documentation.
    pub documentation: Option<String>,
    /// Text inserted for the item (snippet syntax if `is_snippet`).
    pub insert_text: String,
    pub is_snippet: bool,
    /// The range replaced by `insert_text`.
    pub replace_range: TextRange,
    pub filter_text: Option<String>,
    pub sort_text: Option<String>,
    /// e.g. `use` statements for auto import.
    pub additional_edits: Vec<TextEdit>,
    pub deprecated: bool,
    /// Re-trigger signature help after insertion.
    pub trigger_signature_help: bool,
}

impl CompletionItem {
    pub(crate) fn new(label: impl Into<String>, kind: CompletionKind, range: TextRange) -> CompletionItem {
        let label = label.into();
        CompletionItem {
            insert_text: label.clone(),
            label,
            kind,
            detail: None,
            documentation: None,
            is_snippet: false,
            replace_range: range,
            filter_text: None,
            sort_text: None,
            additional_edits: Vec::new(),
            deprecated: false,
            trigger_signature_help: false,
        }
    }
}

/// Result of a completion request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionList {
    pub items: Vec<CompletionItem>,
    /// More items exist; the client should ask again when the prefix changes.
    pub incomplete: bool,
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "array",
    "as",
    "break",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enum",
    "extends",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "try",
    "unset",
    "use",
    "while",
    "yield",
    "yield from",
    "true",
    "false",
    "null",
    "self",
    "parent",
    "__CLASS__",
    "__DIR__",
    "__FILE__",
    "__FUNCTION__",
    "__LINE__",
    "__METHOD__",
    "__NAMESPACE__",
    "__TRAIT__",
];

const DOC_TAGS: &[&str] = &[
    "param",
    "return",
    "var",
    "throws",
    "deprecated",
    "see",
    "since",
    "link",
    "property",
    "property-read",
    "property-write",
    "method",
    "template",
    "template-covariant",
    "extends",
    "implements",
    "use",
    "mixin",
    "inheritDoc",
    "internal",
    "todo",
    "example",
    "author",
    "version",
    "api",
    "final",
    "phpstan-param",
    "phpstan-return",
    "phpstan-var",
    "phpstan-template",
    "psalm-param",
    "psalm-return",
    "psalm-var",
    "psalm-template",
    "noinspection",
    "codeCoverageIgnore",
    "dataProvider",
    "test",
    "covers",
    "group",
];

const SUPERGLOBALS: &[&str] =
    &["$_GET", "$_POST", "$_SERVER", "$_REQUEST", "$_COOKIE", "$_FILES", "$_ENV", "$_SESSION", "$GLOBALS"];

enum NameContext {
    /// Anywhere an expression or statement may start.
    Expression,
    /// After `new`.
    New,
    /// Only class-like names (types, extends, catch, ...).
    Type,
    /// Inside a `use` statement: fully qualified names.
    Use,
    /// Attribute name after `#[`.
    Attribute,
}

pub(crate) fn completions(sema: &Sema, offset: TextSize) -> CompletionList {
    let mut cx = Cx { sema, offset, items: Vec::new(), incomplete: false };
    cx.run();
    let max = sema.ws.config().max_completion_items.max(1);
    if cx.items.len() > max {
        cx.items.truncate(max);
        cx.incomplete = true;
    }
    CompletionList { items: cx.items, incomplete: cx.incomplete }
}

struct Cx<'s, 'a> {
    sema: &'s Sema<'a>,
    offset: TextSize,
    items: Vec<CompletionItem>,
    incomplete: bool,
}

fn matches_prefix(name: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    // case insensitive subsequence match starting with the first character
    let mut chars = name.chars().map(|c| c.to_ascii_lowercase());
    let mut p = prefix.chars().map(|c| c.to_ascii_lowercase());
    let first = p.next().unwrap();
    let Some(n0) = chars.next() else { return false };
    if n0 != first {
        return false;
    }
    let mut want = p.next();
    for c in chars {
        match want {
            Some(w) if w == c => want = p.next(),
            None => return true,
            _ => {}
        }
    }
    want.is_none()
}

impl Cx<'_, '_> {
    fn run(&mut self) {
        let sema = self.sema;
        let root = &sema.root;
        let Some(left) = root.token_at_offset(self.offset).left_biased() else {
            self.names(NameContext::Expression, "", TextRange::empty(self.offset));
            return;
        };
        match left.kind() {
            DOC_COMMENT | COMMENT if left.text().starts_with("/**") => {
                if !self.docblock(&left) {
                    self.doc_tags(&left);
                }
                return;
            }
            STRING => {
                self.string_values(&left);
                return;
            }
            INLINE_HTML => {
                self.items = crate::html::completions(root, self.offset).unwrap_or_default();
                return;
            }
            // `<` / `/` are trigger characters for HTML only
            ENCAPSED_TEXT | COMMENT | HEREDOC_START | SLASH | LT => return,
            _ => {}
        }
        // the identifier being typed (prefix) and its range
        let (prefix, range, before) =
            if (left.kind() == IDENT || left.kind().is_keyword() || left.kind() == VARIABLE)
                && left.text_range().end() >= self.offset
            {
                let start = left.text_range().start();
                let prefix = left.text()[..usize::from(self.offset - start)].to_string();
                (prefix, TextRange::new(start, self.offset), prev_significant(&left))
            } else if left.kind() == DOLLAR {
                ("$".to_string(), left.text_range(), prev_significant(&left))
            } else {
                (
                    String::new(),
                    TextRange::empty(self.offset),
                    if left.kind().is_trivia() { prev_significant(&left) } else { Some(left.clone()) },
                )
            };

        if prefix.starts_with('$') {
            let in_static_access = before.as_ref().is_some_and(|b| b.kind() == COLON2);
            if in_static_access {
                let lhs = before.as_ref().and_then(|b| b.parent()).and_then(|p| p.first_child());
                if let Some(lhs) = lhs {
                    self.static_members(&lhs, &prefix, range);
                }
                return;
            }
            self.variables(&prefix, range);
            return;
        }

        if let Some(before) = &before {
            match before.kind() {
                ARROW | NULLSAFE_ARROW => {
                    if let Some(recv) = before.parent().and_then(|p| p.first_child()) {
                        self.instance_members(&recv, &prefix, range);
                    }
                    return;
                }
                COLON2 => {
                    if let Some(lhs) = before.parent().and_then(|p| p.first_child()) {
                        self.static_members(&lhs, &prefix, range);
                    }
                    return;
                }
                NEW_KW => {
                    self.names(NameContext::New, &prefix, range);
                    return;
                }
                HASH_BRACK => {
                    self.names(NameContext::Attribute, &prefix, range);
                    return;
                }
                EXTENDS_KW | IMPLEMENTS_KW | INSTANCEOF_KW | INSTEADOF_KW => {
                    self.names(NameContext::Type, &prefix, range);
                    return;
                }
                VARIABLE | INT_NUMBER | FLOAT_NUMBER | STRING if prefix.is_empty() => return,
                _ => {}
            }
        }
        // look at the syntactic context of the token being completed
        let anchor = if prefix.is_empty() { before.clone() } else { Some(left.clone()) };
        let ctx_node = anchor.and_then(|t| t.parent());
        let ctx = ctx_node
            .as_ref()
            .and_then(|n| {
                n.ancestors().find_map(|a| match a.kind() {
                    USE_DECL => Some(NameContext::Use),
                    ATTRIBUTE => Some(NameContext::Attribute),
                    NAMED_TYPE | EXTENDS_CLAUSE | IMPLEMENTS_CLAUSE | CATCH_TYPES | TRAIT_USE => {
                        Some(NameContext::Type)
                    }
                    _ => None,
                })
            })
            .unwrap_or(NameContext::Expression);
        // member name in a declaration (`function fo|`) – nothing to offer
        if let Some(n) = &ctx_node {
            if matches!(n.kind(), NAME | MEMBER_NAME)
                && n.parent().is_some_and(|p| {
                    matches!(
                        p.kind(),
                        FUNCTION_DECL
                            | METHOD_DECL
                            | CLASS_DECL
                            | INTERFACE_DECL
                            | TRAIT_DECL
                            | ENUM_DECL
                            | CONST_ELEMENT
                    )
                })
            {
                return;
            }
        }
        // named arguments
        if prefix.chars().all(|c| c.is_alphanumeric() || c == '_') {
            self.named_args(&left, &prefix, range);
        }
        self.names(ctx, &prefix, range);
    }

    // ------------------------------------------------------------------ members

    fn caller_class(&self) -> Option<Fqn> {
        let token = self.sema.token_at(self.offset)?;
        self.sema.enclosing_class(&token.parent()?)
    }

    fn visible(&self, owner: &Fqn, vis: Visibility, caller: Option<&Fqn>) -> bool {
        match vis {
            Visibility::Public => true,
            Visibility::Private => caller.is_some_and(|c| c.eq_ignore_case(owner)),
            Visibility::Protected => caller.is_some_and(|c| {
                self.sema.index().is_subtype(c, owner) || self.sema.index().is_subtype(owner, c)
            }),
        }
    }

    fn method_item(&self, f: &FunctionDef, owner: &Fqn, range: TextRange) -> CompletionItem {
        let mut item = CompletionItem::new(f.name.clone(), CompletionKind::Method, range);
        item.detail = Some(render::function_signature(f, true));
        item.documentation = f.doc.as_ref().map(|d| render::doc_markdown(d, false)).filter(|d| !d.is_empty());
        if f.params.is_empty() {
            item.insert_text = format!("{}()", f.name);
        } else {
            item.insert_text = format!("{}($0)", f.name);
            item.is_snippet = true;
            item.trigger_signature_help = true;
        }
        item.deprecated = f.deprecated.is_some();
        // own members first, then inherited, magic last
        let rank = if f.is_magic {
            2
        } else if self.caller_class().is_some_and(|c| c.eq_ignore_case(owner)) {
            0
        } else {
            1
        };
        item.sort_text = Some(format!("{rank}{}", f.name.to_ascii_lowercase()));
        item
    }

    fn instance_members(&mut self, receiver: &SyntaxNode, prefix: &str, range: TextRange) {
        let ty = self.sema.infer().expr_type(receiver);
        let caller = self.caller_class();
        let mut seen = HashSet::new();
        for class in ty.class_names() {
            let members = self.sema.index().members(&class);
            for p in &members.properties {
                if p.def.is_static || !self.visible(&p.owner.fqn, p.def.visibility, caller.as_ref()) {
                    continue;
                }
                if !matches_prefix(&p.def.name, prefix) || !seen.insert(format!("p{}", p.def.name)) {
                    continue;
                }
                let mut item = CompletionItem::new(p.def.name.clone(), CompletionKind::Property, range);
                item.detail = p.def.effective_type().map(|t| t.to_string());
                item.documentation =
                    p.def.doc.as_ref().map(|d| render::doc_markdown(d, false)).filter(|d| !d.is_empty());
                item.deprecated = p.def.deprecated.is_some();
                item.sort_text = Some(format!("0{}", p.def.name.to_ascii_lowercase()));
                self.items.push(item);
            }
            for m in &members.methods {
                if !self.visible(&m.owner.fqn, m.def.visibility, caller.as_ref())
                    || m.def.name.starts_with("__") && !prefix.starts_with("__")
                {
                    continue;
                }
                if !matches_prefix(&m.def.name, prefix)
                    || !seen.insert(format!("m{}", m.def.name.to_ascii_lowercase()))
                {
                    continue;
                }
                let item = self.method_item(m.def, &m.owner.fqn, range);
                self.items.push(item);
            }
            // enum instances
            if let Some((_, c)) = self.sema.index().class(&class) {
                if c.kind == ClassLikeKind::Enum {
                    for (name, detail) in [
                        ("name", Some("string".to_string())),
                        ("value", c.backing_type.as_ref().map(|t| t.to_string())),
                    ] {
                        if name == "value" && c.backing_type.is_none() {
                            continue;
                        }
                        if matches_prefix(name, prefix) && seen.insert(format!("p{name}")) {
                            let mut item = CompletionItem::new(name, CompletionKind::Property, range);
                            item.detail = detail;
                            self.items.push(item);
                        }
                    }
                }
            }
        }
    }

    fn static_members(&mut self, lhs: &SyntaxNode, prefix: &str, range: TextRange) {
        let lhs_text = php_syntax::ast::text_without_trivia(lhs).to_ascii_lowercase();
        let relative = matches!(lhs_text.as_str(), "self" | "static" | "parent");
        let classes = crate::classify::static_lhs_classes(self.sema, lhs);
        let caller = self.caller_class();
        let mut seen = HashSet::new();
        let want_var = prefix.starts_with('$');
        let bare = prefix.trim_start_matches('$');
        for class in &classes {
            let members = self.sema.index().members(class);
            if !want_var {
                for c in &members.cases {
                    if matches_prefix(&c.def.name, bare) && seen.insert(c.def.name.clone()) {
                        let mut item =
                            CompletionItem::new(c.def.name.clone(), CompletionKind::EnumMember, range);
                        item.detail = Some(render::case_signature(c.def));
                        item.sort_text = Some(format!("0{}", c.def.name));
                        self.items.push(item);
                    }
                }
                for c in &members.constants {
                    if !self.visible(&c.owner.fqn, c.def.visibility, caller.as_ref())
                        || !matches_prefix(&c.def.name, bare)
                    {
                        continue;
                    }
                    if seen.insert(c.def.name.clone()) {
                        let mut item =
                            CompletionItem::new(c.def.name.clone(), CompletionKind::Constant, range);
                        item.detail = Some(render::class_const_signature(c.def));
                        item.deprecated = c.def.deprecated.is_some();
                        item.sort_text = Some(format!("1{}", c.def.name));
                        self.items.push(item);
                    }
                }
                for m in &members.methods {
                    if !(m.def.is_static || relative)
                        || !self.visible(&m.owner.fqn, m.def.visibility, caller.as_ref())
                    {
                        continue;
                    }
                    if !matches_prefix(&m.def.name, bare) || !seen.insert(m.def.name.to_ascii_lowercase()) {
                        continue;
                    }
                    let item = self.method_item(m.def, &m.owner.fqn, range);
                    self.items.push(item);
                }
                if matches_prefix("class", bare) {
                    let mut item = CompletionItem::new("class", CompletionKind::Keyword, range);
                    item.sort_text = Some("3class".into());
                    self.items.push(item);
                }
            }
            for p in &members.properties {
                if !p.def.is_static || !self.visible(&p.owner.fqn, p.def.visibility, caller.as_ref()) {
                    continue;
                }
                if !matches_prefix(&p.def.name, bare) || !seen.insert(format!("${}", p.def.name)) {
                    continue;
                }
                let mut item =
                    CompletionItem::new(format!("${}", p.def.name), CompletionKind::Property, range);
                item.detail = p.def.effective_type().map(|t| t.to_string());
                item.sort_text = Some(format!("2{}", p.def.name));
                self.items.push(item);
            }
        }
    }

    // ------------------------------------------------------------------ variables

    fn variables(&mut self, prefix: &str, range: TextRange) {
        let Some(token) = self.sema.token_at(self.offset) else { return };
        let Some(node) = token.parent() else { return };
        let scope = self.sema.variable_scope(&node);
        let mut seen: HashSet<String> = HashSet::new();
        let bare = prefix.trim_start_matches('$');
        let mut names: Vec<(String, TextSize)> = Vec::new();
        for t in scope.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if t.kind() != VARIABLE
                || t.text_range() == range
                || t.text_range().contains_inclusive(self.offset)
            {
                continue;
            }
            // variables of nested closures are not visible (arrow fns are)
            if !in_variable_scope(&t, &scope) && !in_arrow_fn_of(&t, &scope) {
                continue;
            }
            // skip property declarations and static property names
            if t.parent().is_some_and(|p| matches!(p.kind(), PROPERTY_ELEMENT | MEMBER_NAME)) {
                continue;
            }
            let name = t.text().to_string();
            if seen.insert(name.clone()) {
                names.push((name, t.text_range().start()));
            }
        }
        let is_method = scope.kind() == METHOD_DECL
            && !scope.children().any(|c| {
                c.kind() == MODIFIER_LIST && c.children_with_tokens().any(|e| e.kind() == STATIC_KW)
            });
        if (is_method || scope.kind() == CLOSURE_EXPR && self.sema.enclosing_class(&scope).is_some())
            && seen.insert("$this".into())
        {
            names.push(("$this".into(), scope.text_range().start()));
        }
        for (name, pos) in names {
            if !matches_prefix(&name[1..], bare) && !bare.is_empty() {
                continue;
            }
            let mut item = CompletionItem::new(name.clone(), CompletionKind::Variable, range);
            let ty = self.sema.infer().variable_type(&name[1..], &node);
            if !ty.is_mixed() {
                item.detail = Some(ty.to_string());
            }
            // nearer declarations first
            let distance = u32::from(self.offset.checked_sub(pos).unwrap_or_default());
            item.sort_text = Some(format!("0{distance:010}"));
            self.items.push(item);
        }
        for g in SUPERGLOBALS {
            if matches_prefix(&g[1..], bare) || bare.is_empty() {
                let mut item = CompletionItem::new(*g, CompletionKind::Variable, range);
                item.detail = Some("array".into());
                item.sort_text = Some(format!("9{g}"));
                self.items.push(item);
            }
        }
    }

    // ------------------------------------------------------------------ names

    fn names(&mut self, ctx: NameContext, prefix: &str, range: TextRange) {
        let scope = self.sema.scope_at(self.offset);
        let qualified = prefix.contains('\\');
        let short_prefix = prefix.rsplit('\\').next().unwrap_or(prefix);
        let index = self.sema.index();
        let mut count = 0usize;
        let limit = self.sema.ws.config().max_completion_items * 3;

        if matches!(ctx, NameContext::Expression) && !qualified {
            for kw in KEYWORDS {
                if matches_prefix(kw, prefix) && !prefix.is_empty() {
                    let mut item = CompletionItem::new(*kw, CompletionKind::Keyword, range);
                    item.sort_text = Some(format!("5{kw}"));
                    self.items.push(item);
                }
            }
        }
        if prefix.is_empty() && matches!(ctx, NameContext::Expression) {
            // without a prefix there are too many names; let the client re-request
            self.incomplete = true;
            return;
        }

        // classes
        let mut seen = HashSet::new();
        for (_, c) in index.all_classes() {
            let name = c.fqn.short();
            if !matches_prefix(name, short_prefix) {
                continue;
            }
            if qualified
                && !c
                    .fqn
                    .as_str()
                    .to_ascii_lowercase()
                    .contains(&prefix.trim_start_matches('\\').to_ascii_lowercase())
            {
                continue;
            }
            match ctx {
                NameContext::New if c.kind != ClassLikeKind::Class || c.is_abstract => continue,
                NameContext::Attribute
                    if !c.attributes.iter().any(|a| a.short().eq_ignore_ascii_case("Attribute")) =>
                {
                    continue
                }
                _ => {}
            }
            if !seen.insert(c.fqn.key()) {
                continue;
            }
            count += 1;
            if count > limit {
                self.incomplete = true;
                break;
            }
            let kind = match c.kind {
                ClassLikeKind::Class => CompletionKind::Class,
                ClassLikeKind::Interface => CompletionKind::Interface,
                ClassLikeKind::Trait => CompletionKind::Trait,
                ClassLikeKind::Enum => CompletionKind::Enum,
            };
            let mut item = CompletionItem::new(name.to_string(), kind, range);
            item.detail = Some(c.fqn.to_string());
            item.documentation =
                c.doc.as_ref().map(|d| render::doc_markdown(d, false)).filter(|d| !d.is_empty());
            item.deprecated = c.deprecated.is_some();
            item.filter_text = Some(if qualified { c.fqn.to_string() } else { name.to_string() });
            let exact = name.eq_ignore_ascii_case(short_prefix) as u8;
            item.sort_text = Some(format!("{}{}", 2 - exact, name.to_ascii_lowercase()));
            self.apply_class_insert(&mut item, &c.fqn, scope, &ctx, prefix);
            if matches!(ctx, NameContext::New) {
                item.insert_text.push_str("($0)");
                item.is_snippet = true;
                item.trigger_signature_help = true;
            }
            self.items.push(item);
        }
        if !matches!(ctx, NameContext::Expression | NameContext::Use) {
            return;
        }
        // functions
        let use_fn = matches!(ctx, NameContext::Use);
        for (_, f) in index.all_functions() {
            if !matches_prefix(f.fqn.short(), short_prefix) || !seen.insert(format!("f{}", f.fqn.key())) {
                continue;
            }
            count += 1;
            if count > limit {
                self.incomplete = true;
                break;
            }
            let mut item = CompletionItem::new(f.fqn.short().to_string(), CompletionKind::Function, range);
            item.detail = Some(render::function_signature(f, false));
            item.documentation =
                f.doc.as_ref().map(|d| render::doc_markdown(d, false)).filter(|d| !d.is_empty());
            item.deprecated = f.deprecated.is_some();
            item.sort_text = Some(format!("3{}", f.fqn.short().to_ascii_lowercase()));
            if use_fn {
                item.insert_text = f.fqn.to_string();
            } else {
                let ns = scope.namespace_str();
                let fns = f.fqn.namespace().unwrap_or("");
                if !fns.is_empty() && !fns.eq_ignore_ascii_case(ns) && !qualified {
                    item.additional_edits.push(use_edit(self.sema, scope, &format!("function {}", f.fqn)));
                }
                if f.params.is_empty() {
                    item.insert_text = format!("{}()", f.fqn.short());
                } else {
                    item.insert_text = format!("{}($0)", f.fqn.short());
                    item.is_snippet = true;
                    item.trigger_signature_help = true;
                }
            }
            self.items.push(item);
        }
        // constants
        for (_, c) in index.all_constants() {
            if !matches_prefix(c.fqn.short(), short_prefix) || !seen.insert(format!("c{}", c.fqn)) {
                continue;
            }
            count += 1;
            if count > limit {
                self.incomplete = true;
                break;
            }
            let mut item = CompletionItem::new(c.fqn.short().to_string(), CompletionKind::Constant, range);
            item.detail = Some(render::const_signature(c));
            item.sort_text = Some(format!("4{}", c.fqn.short()));
            self.items.push(item);
        }
    }

    fn apply_class_insert(
        &self,
        item: &mut CompletionItem,
        fqn: &Fqn,
        scope: &NamespaceScope,
        ctx: &NameContext,
        prefix: &str,
    ) {
        if matches!(ctx, NameContext::Use) {
            item.insert_text = fqn.to_string();
            return;
        }
        if prefix.starts_with('\\') {
            item.insert_text = format!("\\{fqn}");
            return;
        }
        if prefix.contains('\\') {
            // keep the user's qualification style: complete the remaining segments
            let typed_ns = prefix.rsplit_once('\\').map(|(ns, _)| ns).unwrap_or("");
            match scope.resolve_class(&format!("{typed_ns}\\{}", fqn.short())) == *fqn {
                true => item.insert_text = format!("{typed_ns}\\{}", fqn.short()),
                false => item.insert_text = format!("\\{fqn}"),
            }
            return;
        }
        match scope.shortest_class_name(fqn) {
            Some(name) => item.insert_text = name,
            None => {
                if !self.sema.ws.config().auto_import || scope.alias_taken(fqn.short()) {
                    item.insert_text = format!("\\{fqn}");
                } else {
                    item.insert_text = fqn.short().to_string();
                    item.additional_edits.push(use_edit(self.sema, scope, fqn.as_str()));
                }
            }
        }
    }

    // ------------------------------------------------------------------ misc

    fn named_args(&mut self, left: &SyntaxToken, prefix: &str, range: TextRange) {
        let Some(arg_list) = left.parent_ancestors().find(|n| n.kind() == ARG_LIST) else { return };
        // only at the start of an argument
        let Some(call) = arg_list.parent() else { return };
        let params = match crate::signature_help::callee_params(self.sema, &call) {
            Some(p) => p,
            None => return,
        };
        for p in params {
            if matches_prefix(&p.name, prefix) && !prefix.is_empty() {
                let mut item = CompletionItem::new(format!("{}:", p.name), CompletionKind::Variable, range);
                item.insert_text = format!("{}: ", p.name);
                item.detail = p.effective_type().map(|t| t.to_string());
                item.sort_text = Some(format!("0{}", p.name));
                self.items.push(item);
            }
        }
    }

    /// Values for a string literal argument contributed by plugins.
    fn string_values(&mut self, token: &SyntaxToken) {
        let Some((arg, content)) = string_arg_at(self.sema, self.offset) else { return };
        let cx = crate::plugins::PluginCx { ws: self.sema.ws, file: self.sema.file };
        let prefix_len = usize::from(self.offset.checked_sub(content.start()).unwrap_or_default());
        let prefix: String = arg.value.chars().take(prefix_len).collect();
        let _ = token;
        let mut seen = HashSet::new();
        for p in self.sema.ws.plugins() {
            for c in p.string_candidates(&cx, &arg) {
                if !c.value.to_ascii_lowercase().contains(&prefix.to_ascii_lowercase())
                    || !seen.insert(c.value.clone())
                {
                    continue;
                }
                let mut item = CompletionItem::new(c.value.clone(), c.kind, content);
                item.detail = c.detail;
                item.sort_text =
                    Some(format!("{}{}", if c.value.starts_with(&prefix) { 0 } else { 1 }, c.value));
                self.items.push(item);
            }
        }
    }

    /// `/** */` directly before a declaration: offer a generated PHPDoc.
    fn docblock(&mut self, token: &SyntaxToken) -> bool {
        let inner = token.text().trim_start_matches("/**").trim_end_matches("*/");
        if !inner.trim().is_empty() || self.offset != token.text_range().start() + TextSize::from(3) {
            return false;
        }
        let mut next = token.next_sibling_or_token();
        while let Some(php_syntax::NodeOrToken::Token(t)) = &next {
            if t.kind() != WHITESPACE {
                return false;
            }
            next = t.next_sibling_or_token();
        }
        let Some(php_syntax::NodeOrToken::Node(decl)) = next else { return false };
        let text = &self.sema.text;
        let start = usize::from(token.text_range().start());
        let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = text[line_start..start].chars().take_while(|c| c.is_whitespace()).collect();
        let Some(doc) = crate::code_actions::phpdoc_for(self.sema, &decl, &indent, true) else {
            return false;
        };
        let mut item = CompletionItem::new("/** */", CompletionKind::Snippet, token.text_range());
        item.detail = Some("PHPDoc".into());
        item.insert_text = doc;
        item.is_snippet = true;
        item.sort_text = Some("0".into());
        self.items.push(item);
        true
    }

    fn doc_tags(&mut self, token: &SyntaxToken) {
        let text = token.text();
        let rel = usize::from(self.offset - token.text_range().start());
        let before = &text[..rel.min(text.len())];
        let Some(at) = before.rfind('@') else { return };
        let word = &before[at + 1..];
        if word.contains(char::is_whitespace) {
            return;
        }
        let start = token.text_range().start() + TextSize::from(at as u32 + 1);
        let range = TextRange::new(start, self.offset);
        for tag in DOC_TAGS {
            if matches_prefix(tag, word) || word.is_empty() {
                let mut item = CompletionItem::new(*tag, CompletionKind::Keyword, range);
                item.sort_text = Some(format!("0{tag}"));
                self.items.push(item);
            }
        }
    }
}

fn in_arrow_fn_of(token: &SyntaxToken, scope: &SyntaxNode) -> bool {
    token.parent_ancestors().take_while(|a| a != scope).any(|a| a.kind() == ARROW_FN_EXPR)
        && token
            .parent_ancestors()
            .find(|n| matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | SOURCE_FILE))
            .as_ref()
            == Some(scope)
}

fn prev_significant(token: &SyntaxToken) -> Option<SyntaxToken> {
    let mut t = token.prev_token();
    while let Some(tok) = t {
        if !tok.kind().is_trivia() {
            return Some(tok);
        }
        t = tok.prev_token();
    }
    None
}

/// An edit inserting `use <what>;` into `scope`.
pub(crate) fn use_edit(sema: &Sema, scope: &NamespaceScope, what: &str) -> TextEdit {
    let at = scope.use_insert_offset;
    let text = &sema.text;
    let before = &text[..usize::from(at).min(text.len())];
    let new_text = if !scope.imports.is_empty() {
        format!("\nuse {what};")
    } else if before.trim_end().ends_with('{') {
        format!("\n    use {what};\n")
    } else if scope.namespace.is_some() {
        format!("\n\nuse {what};")
    } else {
        format!("\nuse {what};\n")
    };
    TextEdit { range: TextRange::empty(at), new_text }
}

/// The string literal argument at `offset`: plugin info and the range of
/// its content (between the quotes).
pub(crate) fn string_arg_at(sema: &Sema, offset: TextSize) -> Option<(crate::plugins::StringArg, TextRange)> {
    use crate::plugins::{Callee, StringArg};
    use php_syntax::ast::AstNode;
    let token = sema.root.token_at_offset(offset).find(|t| t.kind() == STRING)?;
    let r = token.text_range();
    let text = token.text();
    let quote_len = TextSize::from(1);
    let closed = text.len() >= 2 && text.ends_with(&text[..1]);
    let content = TextRange::new(r.start() + quote_len, if closed { r.end() - quote_len } else { r.end() });
    if !content.contains_inclusive(offset) {
        return None;
    }
    let lit = token.parent().filter(|p| p.kind() == LITERAL)?;
    let arg = lit.parent().filter(|p| p.kind() == ARG)?;
    let list = arg.parent()?;
    let call = list.parent()?;
    let index = list.children().filter(|c| c.kind() == ARG).position(|a| a == arg)?;
    let callee = match call.kind() {
        CALL_EXPR => {
            let callee = call.first_child().filter(|c| c.kind() == NAME_EXPR)?;
            let name = php_syntax::ast::text_without_trivia(&callee);
            let fqn = match sema.resolve_function(&name, callee.text_range().start()) {
                Some((_, f)) => f.fqn.clone(),
                None => {
                    let r = sema.scope_at(callee.text_range().start()).resolve_function(&name);
                    r.fallback.unwrap_or(r.name)
                }
            };
            Callee::Function(fqn)
        }
        METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
            let name = call.children().find_map(php_syntax::ast::MemberName::cast)?.text()?;
            let lhs = call.first_child()?;
            let classes = if call.kind() == STATIC_CALL_EXPR {
                crate::classify::static_lhs_classes(sema, &lhs)
            } else {
                sema.infer().expr_type(&lhs).class_names()
            };
            let class = classes.into_iter().next().unwrap_or_else(|| Fqn::new(""));
            Callee::Method { class, name, is_static: call.kind() == STATIC_CALL_EXPR }
        }
        _ => return None,
    };
    let value = sema.text[content].to_string();
    Some((StringArg { callee, index, value }, content))
}
