//! Global symbol index over all files of a workspace (including stubs).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use php_hir::symbols::{
    ClassConstDef, ClassDef, ConstDef, EnumCaseDef, FileSymbols, FunctionDef, PropertyDef,
};
use php_hir::{Fqn, Type};

/// Identifies a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// Where a file comes from; used to rank duplicate definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileKind {
    /// A file of the workspace (or an opened document).
    Workspace,
    /// A dependency below `vendor/` or an include path.
    Vendor,
    /// A built-in stub.
    Stub,
}

#[derive(Debug, Clone)]
struct Entry {
    kind: FileKind,
    symbols: Arc<FileSymbols>,
}

#[derive(Debug, Default, Clone)]
pub struct Index {
    files: HashMap<FileId, Entry>,
    classes: HashMap<String, Vec<FileId>>,
    functions: HashMap<String, Vec<FileId>>,
    constants: HashMap<String, Vec<FileId>>,
    globals: HashMap<String, Vec<FileId>>,
}

/// A class member found through inheritance.
#[derive(Debug, Clone, Copy)]
pub struct Found<'a, T> {
    pub file: FileId,
    /// The class declaring the member.
    pub owner: &'a ClassDef,
    pub def: &'a T,
}

/// All members of a class including inherited ones (overrides win).
#[derive(Debug, Default)]
pub struct Members<'a> {
    pub methods: Vec<Found<'a, FunctionDef>>,
    pub properties: Vec<Found<'a, PropertyDef>>,
    pub constants: Vec<Found<'a, ClassConstDef>>,
    pub cases: Vec<Found<'a, EnumCaseDef>>,
}

fn const_key(fqn: &Fqn) -> String {
    // namespace part is case insensitive, the constant name is not
    match fqn.as_str().rfind('\\') {
        Some(i) => format!("{}\\{}", fqn.as_str()[..i].to_ascii_lowercase(), &fqn.as_str()[i + 1..]),
        None => fqn.as_str().to_string(),
    }
}

impl Index {
    pub fn new() -> Index {
        Index::default()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Adds or replaces the symbols of a file.
    pub fn set_file(&mut self, file: FileId, kind: FileKind, symbols: Arc<FileSymbols>) {
        self.remove_file(file);
        for c in &symbols.classes {
            self.classes.entry(c.fqn.key()).or_default().push(file);
        }
        for f in &symbols.functions {
            self.functions.entry(f.fqn.key()).or_default().push(file);
        }
        for c in &symbols.constants {
            self.constants.entry(const_key(&c.fqn)).or_default().push(file);
        }
        for g in &symbols.globals {
            let ids = self.globals.entry(g.name.clone()).or_default();
            if !ids.contains(&file) {
                ids.push(file);
            }
        }
        self.files.insert(file, Entry { kind, symbols });
    }

    pub fn remove_file(&mut self, file: FileId) {
        let Some(old) = self.files.remove(&file) else { return };
        let remove = |map: &mut HashMap<String, Vec<FileId>>, key: String| {
            if let Some(v) = map.get_mut(&key) {
                v.retain(|f| *f != file);
                if v.is_empty() {
                    map.remove(&key);
                }
            }
        };
        for c in &old.symbols.classes {
            remove(&mut self.classes, c.fqn.key());
        }
        for f in &old.symbols.functions {
            remove(&mut self.functions, f.fqn.key());
        }
        for c in &old.symbols.constants {
            remove(&mut self.constants, const_key(&c.fqn));
        }
        for g in &old.symbols.globals {
            remove(&mut self.globals, g.name.clone());
        }
    }

    pub fn file_symbols(&self, file: FileId) -> Option<&Arc<FileSymbols>> {
        self.files.get(&file).map(|e| &e.symbols)
    }

    pub fn file_kind(&self, file: FileId) -> Option<FileKind> {
        self.files.get(&file).map(|e| e.kind)
    }

    pub fn files(&self) -> impl Iterator<Item = (FileId, FileKind, &FileSymbols)> {
        self.files.iter().map(|(id, e)| (*id, e.kind, &*e.symbols))
    }

    fn ranked(&self, ids: &[FileId]) -> Vec<FileId> {
        let mut ids = ids.to_vec();
        ids.sort_by_key(|id| (self.file_kind(*id), *id));
        ids
    }

    // ------------------------------------------------------------------ lookups

    /// All definitions of a class (workspace before vendor before stubs).
    pub fn classes(&self, fqn: &Fqn) -> Vec<(FileId, &ClassDef)> {
        let Some(ids) = self.classes.get(&fqn.key()) else { return Vec::new() };
        let mut out = Vec::new();
        for id in self.ranked(ids) {
            if let Some(e) = self.files.get(&id) {
                out.extend(e.symbols.classes.iter().filter(|c| c.fqn.eq_ignore_case(fqn)).map(|c| (id, c)));
            }
        }
        out
    }

    pub fn class(&self, fqn: &Fqn) -> Option<(FileId, &ClassDef)> {
        self.classes(fqn).into_iter().next()
    }

    pub fn functions(&self, fqn: &Fqn) -> Vec<(FileId, &FunctionDef)> {
        let Some(ids) = self.functions.get(&fqn.key()) else { return Vec::new() };
        let mut out = Vec::new();
        for id in self.ranked(ids) {
            if let Some(e) = self.files.get(&id) {
                out.extend(e.symbols.functions.iter().filter(|f| f.fqn.eq_ignore_case(fqn)).map(|f| (id, f)));
            }
        }
        out
    }

    /// Classes assigned to the global variable `name` (without `$`) at file
    /// level in workspace and vendor files.
    pub fn global_variable_classes(&self, name: &str) -> Vec<Fqn> {
        let Some(ids) = self.globals.get(name) else { return Vec::new() };
        let mut out: Vec<Fqn> = Vec::new();
        for id in self.ranked(ids) {
            let Some(e) = self.files.get(&id) else { continue };
            for g in e.symbols.globals.iter().filter(|g| g.name == name) {
                if !out.iter().any(|c| c.eq_ignore_case(&g.class)) {
                    out.push(g.class.clone());
                }
            }
        }
        out
    }

    pub fn function(&self, fqn: &Fqn) -> Option<(FileId, &FunctionDef)> {
        self.functions(fqn).into_iter().next()
    }

    pub fn constants(&self, fqn: &Fqn) -> Vec<(FileId, &ConstDef)> {
        let key = const_key(fqn);
        let Some(ids) = self.constants.get(&key) else { return Vec::new() };
        let mut out = Vec::new();
        for id in self.ranked(ids) {
            if let Some(e) = self.files.get(&id) {
                out.extend(e.symbols.constants.iter().filter(|c| const_key(&c.fqn) == key).map(|c| (id, c)));
            }
        }
        out
    }

    pub fn constant(&self, fqn: &Fqn) -> Option<(FileId, &ConstDef)> {
        self.constants(fqn).into_iter().next()
    }

    pub fn all_classes(&self) -> impl Iterator<Item = (FileId, &ClassDef)> {
        self.files.iter().flat_map(|(id, e)| e.symbols.classes.iter().map(move |c| (*id, c)))
    }

    pub fn all_functions(&self) -> impl Iterator<Item = (FileId, &FunctionDef)> {
        self.files.iter().flat_map(|(id, e)| e.symbols.functions.iter().map(move |f| (*id, f)))
    }

    pub fn all_constants(&self) -> impl Iterator<Item = (FileId, &ConstDef)> {
        self.files.iter().flat_map(|(id, e)| e.symbols.constants.iter().map(move |c| (*id, c)))
    }

    // ------------------------------------------------------------------ inheritance

    /// The class and all its ancestors in member lookup order: the class,
    /// its traits, then the parent chain (each with traits), then
    /// interfaces, then `@mixin` classes.
    pub fn ancestors(&self, fqn: &Fqn) -> Vec<(FileId, &ClassDef)> {
        self.ancestors_with(fqn, true)
    }

    /// Like [`Index::ancestors`]; `mixins` controls whether `@mixin`
    /// classes are included.
    pub fn ancestors_with(&self, fqn: &Fqn, include_mixins: bool) -> Vec<(FileId, &ClassDef)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut mixins = vec![fqn.clone()];
        let mut first = true;
        // the class itself, then each `@mixin` (recursively, cycle safe)
        while let Some(start) = mixins.pop() {
            if !first && seen.contains(&start.key()) {
                continue;
            }
            first = false;
            let mut interfaces = Vec::new();
            let mut current = Some(start);
            while let Some(name) = current.take() {
                if !seen.insert(name.key()) {
                    break;
                }
                let Some((id, class)) = self.class(&name) else { break };
                out.push((id, class));
                self.collect_traits(class, &mut seen, &mut out);
                interfaces.extend(class.interfaces.iter().cloned());
                if include_mixins {
                    mixins.extend(class.mixins.iter().rev().cloned());
                }
                current = class.parent.clone();
            }
            while let Some(i) = interfaces.pop() {
                if !seen.insert(i.key()) {
                    continue;
                }
                if let Some((id, class)) = self.class(&i) {
                    out.push((id, class));
                    interfaces.extend(class.interfaces.iter().cloned());
                }
            }
        }
        out
    }

    fn collect_traits<'a>(
        &'a self,
        class: &ClassDef,
        seen: &mut HashSet<String>,
        out: &mut Vec<(FileId, &'a ClassDef)>,
    ) {
        for t in &class.traits {
            if !seen.insert(t.key()) {
                continue;
            }
            if let Some((id, tr)) = self.class(t) {
                out.push((id, tr));
                self.collect_traits(tr, seen, out);
            }
        }
    }

    /// Whether some supertype (parent, interface, trait, mixin) of the class
    /// is not in the index, so member lookups may be incomplete.
    pub fn has_unknown_ancestor(&self, fqn: &Fqn) -> bool {
        let ancestors = self.ancestors(fqn);
        if ancestors.is_empty() {
            return true;
        }
        ancestors.iter().any(|(_, c)| c.supertypes().chain(c.mixins.iter()).any(|s| self.class(s).is_none()))
    }

    /// Concrete template arguments of ancestor `target` when looking at
    /// `class<args>`, following `@extends` / `@implements` / `@use` generics:
    /// for `class UserList extends Collection` with
    /// `@extends Collection<int, User>`, `ancestor_args(UserList, [], Collection)`
    /// is `[int, User]`.
    pub fn ancestor_args(&self, class: &Fqn, args: &[Type], target: &Fqn) -> Option<Vec<Type>> {
        let mut seen = HashSet::new();
        self.ancestor_args_inner(class, args, target, &mut seen)
    }

    fn ancestor_args_inner(
        &self,
        class: &Fqn,
        args: &[Type],
        target: &Fqn,
        seen: &mut HashSet<String>,
    ) -> Option<Vec<Type>> {
        if class.eq_ignore_case(target) {
            return Some(args.to_vec());
        }
        if !seen.insert(class.key()) {
            return None;
        }
        let (_, def) = self.class(class)?;
        let subst = |t: &Type| -> Type {
            t.substitute(&|name| {
                def.templates.iter().position(|tp| tp.name == name).and_then(|i| args.get(i).cloned())
            })
        };
        // generic supertypes first, they carry the arguments
        for g in &def.generic_supertypes {
            if let Type::Named(sup, sargs) = g {
                let sargs: Vec<Type> = sargs.iter().map(subst).collect();
                if let Some(found) = self.ancestor_args_inner(sup, &sargs, target, seen) {
                    return Some(found);
                }
            }
        }
        for sup in def.supertypes() {
            if def.generic_supertypes.iter().any(|g| matches!(g, Type::Named(n, _) if n.eq_ignore_case(sup)))
            {
                continue;
            }
            if let Some(found) = self.ancestor_args_inner(sup, &[], target, seen) {
                return Some(found);
            }
        }
        None
    }

    /// Substitutes the templates of `owner` (an ancestor of `class<args>`)
    /// in `ty`.
    pub fn substitute_templates(&self, ty: &Type, class: &Fqn, args: &[Type], owner: &Fqn) -> Type {
        let Some((_, owner_def)) = self.class(owner) else { return ty.clone() };
        if owner_def.templates.is_empty() {
            return ty.clone();
        }
        let owner_args = self.ancestor_args(class, args, owner).unwrap_or_default();
        ty.substitute(&|name| {
            owner_def.templates.iter().position(|tp| tp.name == name).and_then(|i| owner_args.get(i).cloned())
        })
    }

    /// Whether `sub` is `sup` or extends / implements it.
    pub fn is_subtype(&self, sub: &Fqn, sup: &Fqn) -> bool {
        sub.eq_ignore_case(sup) || self.ancestors(sub).iter().any(|(_, c)| c.fqn.eq_ignore_case(sup))
    }

    pub fn parent_of(&self, fqn: &Fqn) -> Option<Fqn> {
        self.class(fqn).and_then(|(_, c)| c.parent.clone())
    }

    pub fn find_method(&self, class: &Fqn, name: &str) -> Option<Found<'_, FunctionDef>> {
        let mut magic = None;
        for (file, owner) in self.ancestors(class) {
            // `use T { method as alias; }`
            if let Some((method, tr, _)) =
                owner.trait_aliases.iter().find(|(_, _, a)| a.eq_ignore_ascii_case(name))
            {
                let from: Vec<&Fqn> = match tr {
                    Some(t) => vec![t],
                    None => owner.traits.iter().collect(),
                };
                if let Some(found) = from.into_iter().find_map(|t| self.find_method(t, method)) {
                    return Some(found);
                }
            }
            if let Some(def) = owner.method(name) {
                // real methods win over `@method` tags of subclasses only if
                // the tag is not more specific; prefer the first non-abstract
                if def.is_magic && magic.is_none() {
                    magic = Some(Found { file, owner, def });
                    continue;
                }
                return Some(Found { file, owner, def });
            }
        }
        magic
    }

    pub fn find_property(&self, class: &Fqn, name: &str) -> Option<Found<'_, PropertyDef>> {
        self.ancestors(class)
            .into_iter()
            .find_map(|(file, owner)| owner.property(name).map(|def| Found { file, owner, def }))
    }

    pub fn find_class_constant(&self, class: &Fqn, name: &str) -> Option<Found<'_, ClassConstDef>> {
        self.ancestors(class)
            .into_iter()
            .find_map(|(file, owner)| owner.constant(name).map(|def| Found { file, owner, def }))
    }

    pub fn find_case(&self, class: &Fqn, name: &str) -> Option<Found<'_, EnumCaseDef>> {
        let (file, owner) = self.class(class)?;
        owner.case(name).map(|def| Found { file, owner, def })
    }

    /// All members visible on a class, the most derived declaration first.
    pub fn members(&self, class: &Fqn) -> Members<'_> {
        self.members_with(class, true)
    }

    /// Like [`Index::members`]; concrete methods win over abstract ones
    /// declared earlier in the lookup order (e.g. abstract trait methods).
    pub fn members_with(&self, class: &Fqn, include_mixins: bool) -> Members<'_> {
        let mut m = Members::default();
        let mut methods: HashMap<String, usize> = HashMap::new();
        let mut seen_props = HashSet::new();
        let mut seen_consts = HashSet::new();
        for (file, owner) in self.ancestors_with(class, include_mixins) {
            for def in &owner.methods {
                let key = def.name.to_ascii_lowercase();
                match methods.get(&key) {
                    Some(&i) => {
                        if m.methods[i].def.is_abstract && !def.is_abstract {
                            m.methods[i] = Found { file, owner, def };
                        }
                    }
                    None => {
                        methods.insert(key, m.methods.len());
                        m.methods.push(Found { file, owner, def });
                    }
                }
            }
            for def in &owner.properties {
                if seen_props.insert(def.name.clone()) {
                    m.properties.push(Found { file, owner, def });
                }
            }
            for def in &owner.constants {
                if seen_consts.insert(def.name.clone()) {
                    m.constants.push(Found { file, owner, def });
                }
            }
            if owner.fqn.eq_ignore_case(class) {
                for def in &owner.cases {
                    m.cases.push(Found { file, owner, def });
                }
            }
        }
        m
    }

    /// Direct and indirect subtypes (implementations / subclasses).
    pub fn subtypes(&self, fqn: &Fqn) -> Vec<(FileId, &ClassDef)> {
        self.all_classes()
            .filter(|(_, c)| !c.fqn.eq_ignore_case(fqn) && self.is_subtype(&c.fqn, fqn))
            .collect()
    }

    /// Classes that directly extend / implement / use `fqn`.
    pub fn direct_subtypes(&self, fqn: &Fqn) -> Vec<(FileId, &ClassDef)> {
        self.all_classes().filter(|(_, c)| c.supertypes().any(|s| s.eq_ignore_case(fqn))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use php_hir::{analyze_file, ExtractOptions};

    fn symbols(text: &str) -> Arc<FileSymbols> {
        let parse = php_syntax::parse(text, php_syntax::PhpVersion::LATEST);
        let (_, s) = analyze_file(&parse.tree(), ExtractOptions::new(php_syntax::PhpVersion::LATEST));
        Arc::new(s)
    }

    #[test]
    fn inheritance_lookup() {
        let mut idx = Index::new();
        idx.set_file(
            FileId(0),
            FileKind::Workspace,
            symbols("<?php namespace A; interface I { function i(); } trait T { function t() {} } class Base implements I { use T; public $p; function i() {} function base() {} const C = 1; }"),
        );
        idx.set_file(
            FileId(1),
            FileKind::Workspace,
            symbols("<?php namespace A; class Child extends Base { function base(): int {} }"),
        );
        let child = Fqn::new("a\\child");
        let m = idx.find_method(&child, "base").unwrap();
        assert_eq!(m.owner.fqn.as_str(), "A\\Child");
        assert_eq!(idx.find_method(&child, "T").unwrap().owner.fqn.as_str(), "A\\T");
        assert_eq!(idx.find_method(&child, "i").unwrap().owner.fqn.as_str(), "A\\Base");
        assert!(idx.find_property(&child, "p").is_some());
        assert!(idx.find_class_constant(&child, "C").is_some());
        assert!(idx.is_subtype(&child, &Fqn::new("A\\I")));
        let members = idx.members(&child);
        assert_eq!(members.methods.len(), 3);
        assert_eq!(idx.subtypes(&Fqn::new("A\\I")).len(), 2);
        idx.remove_file(FileId(1));
        assert!(idx.class(&child).is_none());
    }
}
