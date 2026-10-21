//! Extraction of declared symbols (classes, functions, constants and their
//! members) from a syntax tree.

use std::sync::Arc;

use php_syntax::ast::{self, AstNode, ClassLike, ClassLikeKind, Member};
use php_syntax::{PhpVersion, SyntaxKind, SyntaxKind::*, SyntaxNode, TextRange};

use crate::names::{FileScopes, NamespaceScope};
use crate::phpdoc::{self, DocBlock, PropertyAccess, TemplateTag};
use crate::types::{builtin_type, Fqn, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamDef {
    pub name: String,
    pub ty: Option<Type>,
    pub doc_ty: Option<Type>,
    pub default: Option<String>,
    pub variadic: bool,
    pub by_ref: bool,
    /// Constructor promotion visibility.
    pub promoted: Option<Visibility>,
    pub readonly: bool,
    pub description: Option<String>,
    pub range: TextRange,
}

impl ParamDef {
    pub fn effective_type(&self) -> Option<Type> {
        prefer_doc(self.ty.as_ref(), self.doc_ty.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// Short name (`foo`); for functions see [`FunctionDef::fqn`].
    pub name: String,
    pub fqn: Fqn,
    pub range: TextRange,
    pub name_range: TextRange,
    pub params: Vec<ParamDef>,
    pub return_type: Option<Type>,
    pub doc_return: Option<Type>,
    pub by_ref: bool,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    /// Declared through a `@method` tag.
    pub is_magic: bool,
    pub doc: Option<Arc<DocBlock>>,
    pub deprecated: Option<String>,
    pub templates: Vec<TemplateTag>,
    /// The body reads its arguments dynamically (`func_get_args()`).
    pub dynamic_args: bool,
}

impl FunctionDef {
    pub fn effective_return_type(&self) -> Option<Type> {
        prefer_doc(self.return_type.as_ref(), self.doc_return.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDef {
    pub name: String,
    pub ty: Option<Type>,
    pub doc_ty: Option<Type>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_readonly: bool,
    pub default: Option<String>,
    pub range: TextRange,
    pub name_range: TextRange,
    pub doc: Option<Arc<DocBlock>>,
    pub deprecated: Option<String>,
    /// Declared through `@property*`.
    pub magic: Option<PropertyAccess>,
    /// Promoted constructor parameter.
    pub promoted: bool,
}

impl PropertyDef {
    pub fn effective_type(&self) -> Option<Type> {
        prefer_doc(self.ty.as_ref(), self.doc_ty.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassConstDef {
    pub name: String,
    pub value: Option<String>,
    pub ty: Option<Type>,
    pub visibility: Visibility,
    pub is_final: bool,
    pub range: TextRange,
    pub name_range: TextRange,
    pub doc: Option<Arc<DocBlock>>,
    pub deprecated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumCaseDef {
    pub name: String,
    pub value: Option<String>,
    pub range: TextRange,
    pub name_range: TextRange,
    pub doc: Option<Arc<DocBlock>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    pub fqn: Fqn,
    pub kind: ClassLikeKind,
    pub range: TextRange,
    pub name_range: TextRange,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
    /// `extends` of a class.
    pub parent: Option<Fqn>,
    /// `implements` of a class/enum, `extends` of an interface.
    pub interfaces: Vec<Fqn>,
    pub traits: Vec<Fqn>,
    /// `use T { method as alias; }`: (method, trait, alias).
    pub trait_aliases: Vec<(String, Option<Fqn>, String)>,
    pub methods: Vec<FunctionDef>,
    pub properties: Vec<PropertyDef>,
    pub constants: Vec<ClassConstDef>,
    pub cases: Vec<EnumCaseDef>,
    pub backing_type: Option<Type>,
    pub doc: Option<Arc<DocBlock>>,
    pub deprecated: Option<String>,
    pub templates: Vec<TemplateTag>,
    /// Generic supertypes from `@extends` / `@implements` / `@use`.
    pub generic_supertypes: Vec<Type>,
    pub mixins: Vec<Fqn>,
    pub attributes: Vec<Fqn>,
}

impl ClassDef {
    pub fn method(&self, name: &str) -> Option<&FunctionDef> {
        self.methods.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }
    pub fn property(&self, name: &str) -> Option<&PropertyDef> {
        self.properties.iter().find(|p| p.name == name)
    }
    pub fn constant(&self, name: &str) -> Option<&ClassConstDef> {
        self.constants.iter().find(|c| c.name == name)
    }
    pub fn case(&self, name: &str) -> Option<&EnumCaseDef> {
        self.cases.iter().find(|c| c.name == name)
    }
    /// Direct supertypes (parent, interfaces, traits).
    pub fn supertypes(&self) -> impl Iterator<Item = &Fqn> {
        self.parent.iter().chain(self.interfaces.iter()).chain(self.traits.iter())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDef {
    pub fqn: Fqn,
    pub value: Option<String>,
    pub range: TextRange,
    pub name_range: TextRange,
    pub doc: Option<Arc<DocBlock>>,
    pub deprecated: Option<String>,
}

/// Everything declared in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSymbols {
    pub classes: Vec<ClassDef>,
    pub functions: Vec<FunctionDef>,
    pub constants: Vec<ConstDef>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExtractOptions {
    pub php_version: PhpVersion,
    /// The file is a phpstorm stub: `@since 8.x` limits availability.
    pub stub: bool,
}

impl ExtractOptions {
    pub fn new(php_version: PhpVersion) -> Self {
        ExtractOptions { php_version, stub: false }
    }
}

/// Prefer a PHPDoc type over the declared one when it is more precise.
fn prefer_doc(declared: Option<&Type>, doc: Option<&Type>) -> Option<Type> {
    match (declared, doc) {
        (Some(d), Some(doc)) => {
            if doc.is_mixed() && !d.is_mixed() {
                Some(d.clone())
            } else {
                Some(doc.clone())
            }
        }
        (Some(d), None) => Some(d.clone()),
        (None, Some(doc)) => Some(doc.clone()),
        (None, None) => None,
    }
}

pub fn extract(file: &ast::SourceFile, scopes: &FileScopes, opts: ExtractOptions) -> FileSymbols {
    let mut out = FileSymbols::default();
    let mut cx = Cx { scopes, opts, out: &mut out };
    cx.walk(file.syntax());
    out
}

struct Cx<'a> {
    scopes: &'a FileScopes,
    opts: ExtractOptions,
    out: &'a mut FileSymbols,
}

impl Cx<'_> {
    fn walk(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            match child.kind() {
                FUNCTION_DECL => {
                    if available_in(&child, self.opts) {
                        if let Some(f) = self.function(&child) {
                            self.out.functions.push(f);
                        }
                    }
                    self.nested_defines(&child);
                }
                CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL => {
                    if available_in(&child, self.opts) {
                        let c = ClassLike::cast(child.clone()).unwrap();
                        if let Some(c) = self.class(&c) {
                            self.out.classes.push(c);
                        }
                    }
                    self.nested_defines(&child);
                }
                CONST_DECL => self.consts(&child),
                CALL_EXPR => {
                    self.define(&child);
                    self.walk(&child);
                }
                METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR | ANON_CLASS | MEMBER_LIST => {}
                _ => self.walk(&child),
            }
        }
    }

    fn scope(&self, node: &SyntaxNode) -> &NamespaceScope {
        self.scopes.at(node.text_range().start())
    }

    fn function(&self, node: &SyntaxNode) -> Option<FunctionDef> {
        let f = ast::FunctionDecl::cast(node.clone())?;
        let name = f.name()?;
        let scope = self.scope(node);
        let fqn = Fqn::join(scope.namespace.as_ref(), &name.text());
        let doc = doc_of(node);
        let templates = doc.as_ref().map(|d| d.templates()).unwrap_or_default();
        let tr = TypeResolver { scope, templates: &templates, class_templates: &[] };
        let params = self.params(f.param_list(), doc.as_deref(), &tr);
        let (return_type, doc_return) = self.return_types(node, f.return_type(), doc.as_deref(), &tr);
        Some(FunctionDef {
            name: name.text(),
            fqn,
            range: node.text_range(),
            name_range: name.syntax().text_range(),
            params,
            return_type,
            doc_return,
            by_ref: f.by_ref(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: false,
            is_magic: false,
            deprecated: deprecation(node, doc.as_deref(), self.opts.php_version),
            doc,
            templates,
            dynamic_args: uses_dynamic_args(node),
        })
    }

    fn return_types(
        &self,
        node: &SyntaxNode,
        ret: Option<ast::ReturnType>,
        doc: Option<&DocBlock>,
        tr: &TypeResolver,
    ) -> (Option<Type>, Option<Type>) {
        let mut declared = ret.and_then(|r| r.ty()).map(|t| tr.syntax_type(&t));
        if let Some(t) = stub_type_override(node, self.opts.php_version) {
            declared = t.map(|t| tr.doc_type(&t));
        }
        let doc_ret = doc
            .and_then(|d| d.return_tag())
            .filter(|t| !t.type_text.is_empty())
            .map(|t| tr.doc_type(&t.type_text));
        (declared, doc_ret)
    }

    fn params(
        &self,
        list: Option<ast::ParamList>,
        doc: Option<&DocBlock>,
        tr: &TypeResolver,
    ) -> Vec<ParamDef> {
        let Some(list) = list else { return Vec::new() };
        list.params()
            .filter(|p| available_in(p.syntax(), self.opts))
            .filter_map(|p| {
                let name = p.name()?;
                let mut ty = p.ty().map(|t| tr.syntax_type(&t));
                if let Some(t) = stub_type_override(p.syntax(), self.opts.php_version) {
                    ty = t.map(|t| tr.doc_type(&t));
                }
                let tag = doc.and_then(|d| d.param(&name));
                let doc_ty =
                    tag.as_ref().filter(|t| !t.type_text.is_empty()).map(|t| tr.doc_type(&t.type_text));
                let mods = p.modifiers();
                let promoted = mods.as_ref().and_then(|m| visibility_of(m.modifiers().map(|t| t.kind())));
                let readonly = mods.as_ref().is_some_and(|m| m.has(READONLY_KW));
                let promoted = promoted.or(if readonly { Some(Visibility::Public) } else { None });
                Some(ParamDef {
                    name,
                    ty,
                    doc_ty,
                    // stubs mark optional overload parameters as `[optional]`
                    default: p.default_value().map(|d| compact_text(&d)).or_else(|| {
                        tag.as_ref()
                            .filter(|t| t.description.starts_with("[optional]"))
                            .map(|_| "?".to_string())
                    }),
                    variadic: p.is_variadic(),
                    by_ref: p.by_ref(),
                    promoted,
                    readonly,
                    description: tag
                        .map(|t| t.description.trim_start_matches("[optional]").trim().to_string())
                        .filter(|d| !d.is_empty()),
                    range: p.syntax().text_range(),
                })
            })
            .collect()
    }

    fn class(&self, c: &ClassLike) -> Option<ClassDef> {
        let name = c.name()?;
        let node = c.syntax();
        let scope = self.scope(node);
        let fqn = Fqn::join(scope.namespace.as_ref(), &name.text());
        let doc = doc_of(node);
        let templates = doc.as_ref().map(|d| d.templates()).unwrap_or_default();
        let tr = TypeResolver { scope, templates: &[], class_templates: &templates };
        let mods = c.modifiers();
        let has = |k: SyntaxKind| mods.as_ref().is_some_and(|m| m.has(k));

        let mut parent = None;
        let mut interfaces = Vec::new();
        if let Some(ext) = c.extends() {
            let names: Vec<Fqn> = ext.names().map(|n| scope.resolve_class(&n.text())).collect();
            if c.kind() == ClassLikeKind::Interface {
                interfaces = names;
            } else {
                parent = names.into_iter().next();
            }
        }
        if let Some(imp) = c.implements() {
            interfaces.extend(imp.names().map(|n| scope.resolve_class(&n.text())));
        }
        let backing_type = node
            .children()
            .find(|n| n.kind() == ENUM_BACKING_TYPE)
            .and_then(|n| n.children().find(|c| ast::is_type_kind(c.kind())))
            .map(|t| tr.syntax_type(&t));

        let mut def = ClassDef {
            fqn,
            kind: c.kind(),
            range: node.text_range(),
            name_range: name.syntax().text_range(),
            is_abstract: has(ABSTRACT_KW),
            is_final: has(FINAL_KW),
            is_readonly: has(READONLY_KW),
            parent,
            interfaces,
            traits: Vec::new(),
            trait_aliases: Vec::new(),
            methods: Vec::new(),
            properties: Vec::new(),
            constants: Vec::new(),
            cases: Vec::new(),
            backing_type,
            deprecated: deprecation(node, doc.as_deref(), self.opts.php_version),
            templates: templates.clone(),
            generic_supertypes: Vec::new(),
            mixins: Vec::new(),
            attributes: c
                .attributes()
                .flat_map(|g| g.syntax().children().filter_map(ast::Attribute::cast).collect::<Vec<_>>())
                .filter_map(|a| a.name())
                .map(|n| scope.resolve_class(&n.text()))
                .collect(),
            doc: doc.clone(),
        };
        if let Some(doc) = &doc {
            for kind in ["extends", "implements", "use"] {
                for t in doc.generic_supertypes(kind) {
                    def.generic_supertypes.push(tr.doc_type(&t));
                }
            }
            def.mixins =
                doc.mixins().iter().filter_map(|m| tr.doc_type(m).class_names().into_iter().next()).collect();
        }

        if let Some(members) = c.member_list() {
            for m in members.members() {
                if !available_in(m.syntax(), self.opts) {
                    continue;
                }
                self.member(&m, &mut def, &tr);
            }
        }
        if let Some(doc) = &doc {
            self.magic_members(doc, &mut def, &tr);
        }
        Some(def)
    }

    fn member(&self, m: &Member, def: &mut ClassDef, class_tr: &TypeResolver) {
        let node = m.syntax();
        let doc = doc_of(node);
        let deprecated = deprecation(node, doc.as_deref(), self.opts.php_version);
        match m {
            Member::Method(method) => {
                let Some(name) = method.name() else { return };
                let Some(text) = name.text() else { return };
                let mods = method.modifiers();
                let kinds: Vec<SyntaxKind> =
                    mods.iter().flat_map(|m| m.modifiers().map(|t| t.kind())).collect();
                let templates = doc.as_ref().map(|d| d.templates()).unwrap_or_default();
                let tr = TypeResolver {
                    scope: class_tr.scope,
                    templates: &templates,
                    class_templates: class_tr.class_templates,
                };
                let params = self.params(method.param_list(), doc.as_deref(), &tr);
                // promoted constructor properties
                if text.eq_ignore_ascii_case("__construct") {
                    for p in params.iter().filter(|p| p.promoted.is_some()) {
                        def.properties.push(PropertyDef {
                            name: p.name.clone(),
                            ty: p.ty.clone(),
                            doc_ty: p.doc_ty.clone(),
                            visibility: p.promoted.unwrap(),
                            is_static: false,
                            is_readonly: p.readonly || def.is_readonly,
                            default: p.default.clone(),
                            range: p.range,
                            name_range: find_variable_range(node, p.range).unwrap_or(p.range),
                            doc: None,
                            deprecated: None,
                            magic: None,
                            promoted: true,
                        });
                    }
                }
                let (return_type, doc_return) =
                    self.return_types(node, method.return_type(), doc.as_deref(), &tr);
                def.methods.push(FunctionDef {
                    fqn: Fqn::new(&format!("{}::{}", def.fqn, text)),
                    name: text,
                    range: node.text_range(),
                    name_range: name.syntax().text_range(),
                    params,
                    return_type,
                    doc_return,
                    by_ref: node.children_with_tokens().any(|e| e.kind() == AMP),
                    visibility: visibility_of(kinds.iter().copied()).unwrap_or(Visibility::Public),
                    is_static: kinds.contains(&STATIC_KW),
                    is_abstract: kinds.contains(&ABSTRACT_KW) || def.kind == ClassLikeKind::Interface,
                    is_final: kinds.contains(&FINAL_KW),
                    is_magic: false,
                    doc,
                    deprecated,
                    templates,
                    dynamic_args: uses_dynamic_args(node),
                });
            }
            Member::Property(prop) => {
                let kinds: Vec<SyntaxKind> =
                    prop.modifiers().iter().flat_map(|m| m.modifiers().map(|t| t.kind())).collect();
                let mut ty = prop.ty().map(|t| class_tr.syntax_type(&t));
                if let Some(t) = stub_type_override(node, self.opts.php_version) {
                    ty = t.map(|t| class_tr.doc_type(&t));
                }
                for el in prop.elements() {
                    let (Some(name), Some(var)) = (el.name(), el.variable()) else { continue };
                    let doc_ty = doc
                        .as_ref()
                        .and_then(|d| d.var(Some(&name)))
                        .filter(|t| !t.type_text.is_empty())
                        .map(|t| class_tr.doc_type(&t.type_text));
                    let default = el
                        .syntax()
                        .children()
                        .find(|c| c.kind() != PROPERTY_HOOK_LIST)
                        .map(|d| compact_text(&d));
                    def.properties.push(PropertyDef {
                        name,
                        ty: ty.clone(),
                        doc_ty,
                        visibility: visibility_of(kinds.iter().copied()).unwrap_or(Visibility::Public),
                        is_static: kinds.contains(&STATIC_KW),
                        is_readonly: kinds.contains(&READONLY_KW) || def.is_readonly,
                        default,
                        range: el.syntax().text_range(),
                        name_range: var.text_range(),
                        doc: doc.clone(),
                        deprecated: deprecated.clone(),
                        magic: None,
                        promoted: false,
                    });
                }
            }
            Member::Const(c) => {
                let kinds: Vec<SyntaxKind> =
                    c.modifiers().iter().flat_map(|m| m.modifiers().map(|t| t.kind())).collect();
                let ty =
                    node.children().find(|n| ast::is_type_kind(n.kind())).map(|t| class_tr.syntax_type(&t));
                for el in c.elements() {
                    let (Some(name), Some(name_node)) = (el.name_text(), el.name_node()) else { continue };
                    def.constants.push(ClassConstDef {
                        name,
                        value: el.value().map(|v| compact_text(&v)),
                        ty: ty.clone(),
                        visibility: visibility_of(kinds.iter().copied()).unwrap_or(Visibility::Public),
                        is_final: kinds.contains(&FINAL_KW),
                        range: el.syntax().text_range(),
                        name_range: name_node.text_range(),
                        doc: doc.clone(),
                        deprecated: deprecated.clone(),
                    });
                }
            }
            Member::EnumCase(case) => {
                let Some(name) = case.name() else { return };
                def.cases.push(EnumCaseDef {
                    name: name.text().unwrap_or_default(),
                    value: node
                        .children()
                        .find(|c| c.kind() != MEMBER_NAME && c.kind() != ATTRIBUTE_GROUP)
                        .map(|v| compact_text(&v)),
                    range: node.text_range(),
                    name_range: name.syntax().text_range(),
                    doc,
                });
            }
            Member::TraitUse(t) => {
                def.traits.extend(t.names().map(|n| class_tr.scope.resolve_class(&n.text())));
                let aliases = t.syntax().descendants().filter(|n| n.kind() == TRAIT_ALIAS);
                for alias in aliases {
                    let names: Vec<String> = alias
                        .children()
                        .filter(|c| c.kind() == MEMBER_NAME)
                        .map(|c| ast::text_without_trivia(&c))
                        .collect();
                    if let [method, new_name] = names.as_slice() {
                        let tr = alias
                            .children()
                            .find(|c| c.kind() == QUALIFIED_NAME)
                            .map(|q| class_tr.scope.resolve_class(&ast::text_without_trivia(&q)));
                        def.trait_aliases.push((method.clone(), tr, new_name.clone()));
                    }
                }
            }
        }
    }

    fn magic_members(&self, doc: &DocBlock, def: &mut ClassDef, tr: &TypeResolver) {
        for (tag, access) in doc.properties() {
            let name = tag.var.clone().unwrap_or_default();
            if def.property(&name).is_some() {
                continue;
            }
            def.properties.push(PropertyDef {
                name,
                ty: None,
                doc_ty: if tag.type_text.is_empty() { None } else { Some(tr.doc_type(&tag.type_text)) },
                visibility: Visibility::Public,
                is_static: false,
                is_readonly: access == PropertyAccess::Read,
                default: None,
                range: def.name_range,
                name_range: def.name_range,
                doc: Some(Arc::new(DocBlock { summary: tag.description.clone(), ..Default::default() })),
                deprecated: None,
                magic: Some(access),
                promoted: false,
            });
        }
        for m in doc.methods() {
            if def.method(&m.name).is_some() {
                continue;
            }
            let params = m
                .params
                .iter()
                .map(|p| ParamDef {
                    name: p.name.clone(),
                    ty: None,
                    doc_ty: p.type_text.as_ref().map(|t| tr.doc_type(t)),
                    default: p.default.clone(),
                    variadic: p.variadic,
                    by_ref: false,
                    promoted: None,
                    readonly: false,
                    description: None,
                    range: def.name_range,
                })
                .collect();
            def.methods.push(FunctionDef {
                fqn: Fqn::new(&format!("{}::{}", def.fqn, m.name)),
                name: m.name.clone(),
                range: def.name_range,
                name_range: def.name_range,
                params,
                return_type: None,
                doc_return: m.return_type.as_ref().map(|t| tr.doc_type(t)),
                by_ref: false,
                visibility: Visibility::Public,
                is_static: m.is_static,
                is_abstract: false,
                is_final: false,
                is_magic: true,
                doc: Some(Arc::new(DocBlock { summary: m.description.clone(), ..Default::default() })),
                deprecated: None,
                templates: Vec::new(),
                dynamic_args: false,
            });
        }
    }

    fn consts(&mut self, node: &SyntaxNode) {
        let c = ast::ConstDecl::cast(node.clone()).unwrap();
        let doc = doc_of(node);
        let scope = self.scope(node);
        let mut defs = Vec::new();
        for el in c.elements() {
            let (Some(name), Some(name_node)) = (el.name_text(), el.name_node()) else { continue };
            defs.push(ConstDef {
                fqn: Fqn::join(scope.namespace.as_ref(), &name),
                value: el.value().map(|v| compact_text(&v)),
                range: el.syntax().text_range(),
                name_range: name_node.text_range(),
                doc: doc.clone(),
                deprecated: deprecation(node, doc.as_deref(), self.opts.php_version),
            });
        }
        self.out.constants.extend(defs);
    }

    /// `define()` calls inside function / method bodies (constants are global
    /// however they are defined, e.g. WordPress' `wp_initial_constants()`).
    fn nested_defines(&mut self, node: &SyntaxNode) {
        for call in node.descendants().filter(|n| n.kind() == CALL_EXPR) {
            self.define(&call);
        }
    }

    /// `define('NAME', value)` – names are always global or fully qualified.
    fn define(&mut self, node: &SyntaxNode) {
        let Some(callee) = node.first_child() else { return };
        if callee.kind() != NAME_EXPR
            || !ast::text_without_trivia(&callee).trim_start_matches('\\').eq_ignore_ascii_case("define")
        {
            return;
        }
        let Some(args) = node.children().find(|c| c.kind() == ARG_LIST) else { return };
        let mut args = args.children().filter(|c| c.kind() == ARG);
        let Some(first) = args.next() else { return };
        let Some(lit) = first.first_child().filter(|c| c.kind() == LITERAL) else { return };
        let Some(tok) = lit.first_token().filter(|t| t.kind() == STRING) else { return };
        let name = tok.text().trim_matches(|c| c == '\'' || c == '"');
        if name.is_empty() {
            return;
        }
        let doc = node.parent().and_then(|p| doc_of(&p));
        self.out.constants.push(ConstDef {
            fqn: Fqn::new(&name.replace("\\\\", "\\")),
            value: args.next().and_then(|a| a.first_child()).map(|v| compact_text(&v)),
            range: node.text_range(),
            name_range: tok.text_range(),
            doc,
            deprecated: None,
        });
    }
}

fn uses_dynamic_args(node: &SyntaxNode) -> bool {
    node.children().find(|c| c.kind() == BLOCK).is_some_and(|b| {
        b.descendants_with_tokens().any(|e| {
            e.as_token().is_some_and(|t| {
                t.kind() == IDENT
                    && matches!(
                        t.text().to_ascii_lowercase().as_str(),
                        "func_get_args" | "func_num_args" | "func_get_arg"
                    )
            })
        })
    })
}

fn find_variable_range(method: &SyntaxNode, param_range: TextRange) -> Option<TextRange> {
    method
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == VARIABLE && param_range.contains_range(t.text_range()))
        .map(|t| t.text_range())
}

fn visibility_of(kinds: impl Iterator<Item = SyntaxKind>) -> Option<Visibility> {
    // with asymmetric visibility the first keyword is the read visibility
    for k in kinds {
        match k {
            PUBLIC_KW | VAR_KW => return Some(Visibility::Public),
            PROTECTED_KW => return Some(Visibility::Protected),
            PRIVATE_KW => return Some(Visibility::Private),
            _ => {}
        }
    }
    None
}

pub(crate) fn doc_of(node: &SyntaxNode) -> Option<Arc<DocBlock>> {
    ast::doc_comment(node).map(|t| Arc::new(DocBlock::parse(t.text())))
}

/// Source text with whitespace collapsed (for default values, constants).
pub fn compact_text(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let mut out = String::with_capacity(text.len());
    let mut last_ws = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !last_ws {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(c);
            last_ws = false;
        }
    }
    let out = out.trim().to_string();
    if out.chars().count() > 120 {
        let mut t: String = out.chars().take(117).collect();
        t.push_str("...");
        t
    } else {
        out
    }
}

/// Resolves type syntax and PHPDoc types in a given scope.
pub struct TypeResolver<'a> {
    pub scope: &'a NamespaceScope,
    pub templates: &'a [TemplateTag],
    pub class_templates: &'a [TemplateTag],
}

impl TypeResolver<'_> {
    fn template(&self, name: &str) -> Option<Type> {
        self.templates
            .iter()
            .chain(self.class_templates.iter())
            .find(|t| t.name == name)
            .map(|t| Type::Template(t.name.as_str().into()))
    }

    pub fn name(&self, name: &str) -> Type {
        if let Some(t) = self.template(name) {
            return t;
        }
        self.scope.resolve_type_name(name)
    }

    pub fn doc_type(&self, text: &str) -> Type {
        phpdoc::parse_type(text, &|n| self.name(n))
    }

    pub fn syntax_type(&self, node: &SyntaxNode) -> Type {
        match node.kind() {
            NULLABLE_TYPE => {
                node.children().next().map(|t| self.syntax_type(&t).nullable()).unwrap_or(Type::Mixed)
            }
            UNION_TYPE => Type::union(node.children().map(|t| self.syntax_type(&t)).collect()),
            INTERSECTION_TYPE => Type::Intersection(node.children().map(|t| self.syntax_type(&t)).collect()),
            PAREN_TYPE => node.children().next().map(|t| self.syntax_type(&t)).unwrap_or(Type::Mixed),
            NAMED_TYPE => match node.first_child() {
                Some(name) => {
                    let text = ast::text_without_trivia(&name);
                    match builtin_type(&text) {
                        Some(t) => t,
                        None => Type::Named(self.scope.resolve_class(&text), Vec::new()),
                    }
                }
                None => {
                    let text = ast::text_without_trivia(node);
                    builtin_type(&text).unwrap_or(Type::Mixed)
                }
            },
            _ => Type::Mixed,
        }
    }
}

// ------------------------------------------------------------------ stub attributes

fn attributes(node: &SyntaxNode) -> impl Iterator<Item = (String, SyntaxNode)> {
    node.children()
        .filter(|c| c.kind() == ATTRIBUTE_GROUP)
        .flat_map(|g| g.children().filter(|a| a.kind() == ATTRIBUTE).collect::<Vec<_>>())
        .filter_map(|a| {
            let name = a.children().find(|c| c.kind() == QUALIFIED_NAME)?;
            let text = ast::text_without_trivia(&name);
            let short = text.rsplit('\\').next().unwrap_or(&text).to_string();
            Some((short, a))
        })
}

/// Arguments of an attribute: (name for named args, value node).
fn attr_args(attr: &SyntaxNode) -> Vec<(Option<String>, SyntaxNode)> {
    let Some(list) = attr.children().find(|c| c.kind() == ARG_LIST) else { return Vec::new() };
    list.children()
        .filter(|c| c.kind() == ARG)
        .filter_map(|arg| {
            let name = arg.children().find(|c| c.kind() == NAME).map(|n| ast::text_without_trivia(&n));
            let value = arg.children().find(|c| c.kind() != NAME)?;
            Some((name, value))
        })
        .collect()
}

fn string_value(node: &SyntaxNode) -> Option<String> {
    if node.kind() != LITERAL {
        return None;
    }
    let t = node.first_token()?;
    if t.kind() != STRING {
        return None;
    }
    let s = t.text();
    Some(s[1..s.len().saturating_sub(1)].to_string())
}

/// Evaluates `#[PhpStormStubsElementAvailable(from: 'x', to: 'y')]` and, in
/// stubs, `@since 8.5` doc tags.
fn available_in(node: &SyntaxNode, opts: ExtractOptions) -> bool {
    if opts.stub {
        if let Some(doc) = ast::doc_comment(node) {
            for line in doc.text().lines() {
                let Some(rest) = line.split("@since").nth(1) else { continue };
                let v = rest.split_whitespace().next().unwrap_or("");
                // only PHP versions (extensions document their own versions)
                if v.starts_with(['5', '7', '8']) && v.contains('.') && !rest.contains("PECL") {
                    if let Ok(since) = v.parse::<PhpVersion>() {
                        if opts.php_version < since {
                            return false;
                        }
                    }
                }
            }
        }
    }
    available(node, opts.php_version)
}

fn available(node: &SyntaxNode, version: PhpVersion) -> bool {
    for (name, attr) in attributes(node) {
        if name != "PhpStormStubsElementAvailable" {
            continue;
        }
        let mut from = None;
        let mut to = None;
        for (i, (arg_name, value)) in attr_args(&attr).into_iter().enumerate() {
            let v = string_value(&value).and_then(|s| s.parse::<PhpVersion>().ok());
            match arg_name.as_deref() {
                Some("from") => from = v,
                Some("to") => to = v,
                None if i == 0 => from = v,
                None if i == 1 => to = v,
                _ => {}
            }
        }
        if from.is_some_and(|f| version < f) || to.is_some_and(|t| version > t) {
            return false;
        }
    }
    true
}

/// Evaluates `#[LanguageLevelTypeAware(['8.0' => 'string'], default: 'int')]`.
/// Returns `Some(None)` when the attribute says "no type".
fn stub_type_override(node: &SyntaxNode, version: PhpVersion) -> Option<Option<String>> {
    for (name, attr) in attributes(node) {
        if name != "LanguageLevelTypeAware" {
            continue;
        }
        let mut best: Option<(PhpVersion, String)> = None;
        let mut default = None;
        for (arg_name, value) in attr_args(&attr) {
            match arg_name.as_deref() {
                Some("default") => default = string_value(&value),
                _ if value.kind() == ARRAY_EXPR => {
                    for el in value.children().filter(|c| c.kind() == ARRAY_ELEMENT) {
                        let parts: Vec<SyntaxNode> = el.children().collect();
                        if parts.len() != 2 {
                            continue;
                        }
                        let (Some(k), Some(v)) = (string_value(&parts[0]), string_value(&parts[1])) else {
                            continue;
                        };
                        let Ok(kv) = k.parse::<PhpVersion>() else { continue };
                        if kv <= version && best.as_ref().is_none_or(|(b, _)| kv > *b) {
                            best = Some((kv, v));
                        }
                    }
                }
                _ => {}
            }
        }
        let ty = best.map(|(_, t)| t).or(default);
        return Some(ty.filter(|t| !t.is_empty()));
    }
    None
}

fn deprecation(node: &SyntaxNode, doc: Option<&DocBlock>, version: PhpVersion) -> Option<String> {
    if let Some(d) = doc.and_then(|d| d.deprecated()) {
        return Some(d);
    }
    for (name, attr) in attributes(node) {
        if name != "Deprecated" {
            continue;
        }
        let mut reason = String::new();
        let mut since = None;
        for (i, (arg_name, value)) in attr_args(&attr).into_iter().enumerate() {
            match arg_name.as_deref() {
                Some("since") => since = string_value(&value).and_then(|s| s.parse::<PhpVersion>().ok()),
                Some("reason") | Some("message") => reason = string_value(&value).unwrap_or_default(),
                None if i == 0 => reason = string_value(&value).unwrap_or_default(),
                _ => {}
            }
        }
        if since.is_none_or(|s| version >= s) {
            return Some(reason);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use php_syntax::parse;

    fn extract_text(text: &str, version: PhpVersion) -> FileSymbols {
        let file = parse(text, PhpVersion::LATEST).tree();
        let scopes = FileScopes::build(&file);
        extract(&file, &scopes, ExtractOptions::new(version))
    }

    #[test]
    fn classes_and_members() {
        let s = extract_text(
            r#"<?php
namespace App;
use Illuminate\Database\Eloquent\Model;
/**
 * @property-read int $id
 * @method static self findOrFail(int $id)
 * @template T
 */
final class User extends Model implements \JsonSerializable {
    use HasName;
    public const TABLE = 'users';
    /** @var list<Post> */
    protected array $posts = [];
    public function __construct(private readonly string $name) {}
    /** @return T */
    public static function find(int $id): ?static { return null; }
}
function helper(array $a = []): string {}
const VERSION = 1;
define('APP_DEBUG', true);
"#,
            PhpVersion::LATEST,
        );
        let c = &s.classes[0];
        assert_eq!(c.fqn.as_str(), "App\\User");
        assert!(c.is_final);
        assert_eq!(c.parent.as_ref().unwrap().as_str(), "Illuminate\\Database\\Eloquent\\Model");
        assert_eq!(c.interfaces[0].as_str(), "JsonSerializable");
        assert_eq!(c.traits[0].as_str(), "App\\HasName");
        assert_eq!(c.constant("TABLE").unwrap().value.as_deref(), Some("'users'"));
        let posts = c.property("posts").unwrap();
        assert_eq!(posts.effective_type().unwrap().to_string(), "list<App\\Post>");
        assert_eq!(posts.visibility, Visibility::Protected);
        let name = c.property("name").unwrap();
        assert!(name.promoted && name.is_readonly);
        assert_eq!(name.visibility, Visibility::Private);
        let find = c.method("find").unwrap();
        assert!(find.is_static);
        assert_eq!(find.return_type.as_ref().unwrap().to_string(), "?static");
        assert_eq!(find.effective_return_type().unwrap().to_string(), "T");
        assert!(c.method("findOrFail").unwrap().is_magic);
        assert_eq!(c.property("id").unwrap().magic, Some(PropertyAccess::Read));
        assert_eq!(s.functions[0].fqn.as_str(), "App\\helper");
        assert_eq!(s.functions[0].params[0].default.as_deref(), Some("[]"));
        assert_eq!(s.constants[0].fqn.as_str(), "App\\VERSION");
        assert_eq!(s.constants[1].fqn.as_str(), "APP_DEBUG");
    }

    #[test]
    fn stub_attributes() {
        let text = r#"<?php
#[PhpStormStubsElementAvailable(from: '8.0')]
function only8(#[LanguageLevelTypeAware(['8.0' => 'string'], default: '')] $a): int {}
#[PhpStormStubsElementAvailable(to: '7.4')]
function only7($a) {}
#[Deprecated(since: '8.1')]
function old() {}
"#;
        let s = extract_text(text, PhpVersion::PHP_8_0);
        assert_eq!(s.functions.len(), 2);
        assert_eq!(s.functions[0].name, "only8");
        assert_eq!(s.functions[0].params[0].ty.as_ref().unwrap().to_string(), "string");
        assert!(s.functions[1].deprecated.is_none());
        let s = extract_text(text, PhpVersion::PHP_7_4);
        assert_eq!(s.functions[0].name, "only7");
        let s = extract_text(text, PhpVersion::PHP_8_2);
        assert!(s.functions.iter().find(|f| f.name == "old").unwrap().deprecated.is_some());
    }
}
