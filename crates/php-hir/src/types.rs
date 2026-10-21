//! The PHP type model shared by declarations, PHPDoc and inference.

use std::fmt;
use std::sync::Arc;

/// A fully qualified name without leading backslash (`App\Models\User`).
///
/// Class and function names are case insensitive in PHP; use [`Fqn::key`]
/// for lookups.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fqn(Arc<str>);

impl Fqn {
    pub fn new(name: &str) -> Fqn {
        Fqn(name.trim_start_matches('\\').into())
    }

    /// Joins a namespace and a name.
    pub fn join(namespace: Option<&Fqn>, name: &str) -> Fqn {
        match namespace {
            Some(ns) if !ns.0.is_empty() => {
                Fqn(format!("{}\\{}", ns.0, name.trim_start_matches('\\')).into())
            }
            _ => Fqn::new(name),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Lowercase key for case insensitive lookups.
    pub fn key(&self) -> String {
        self.0.to_ascii_lowercase()
    }

    /// Last segment: `User` for `App\Models\User`.
    pub fn short(&self) -> &str {
        self.0.rsplit('\\').next().unwrap_or(&self.0)
    }

    /// Namespace part, if any: `App\Models` for `App\Models\User`.
    pub fn namespace(&self) -> Option<&str> {
        self.0.rfind('\\').map(|i| &self.0[..i])
    }

    pub fn eq_ignore_case(&self, other: &Fqn) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl fmt::Debug for Fqn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Fqn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeEntry {
    pub key: String,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Mixed,
    Void,
    Never,
    Null,
    Bool,
    True,
    False,
    Int,
    Float,
    String,
    Object,
    Callable,
    Iterable(Option<Box<Type>>, Option<Box<Type>>),
    Resource,
    /// `array`, `array<V>`, `array<K, V>`, `V[]`
    Array(Option<Box<Type>>, Option<Box<Type>>),
    /// `list<V>`
    List(Option<Box<Type>>),
    /// `array{a: int, b?: string}`
    Shape(Vec<ShapeEntry>),
    /// `class-string<T>`
    ClassString(Option<Box<Type>>),
    /// A class, interface or enum, optionally with generic arguments.
    Named(Fqn, Vec<Type>),
    /// `self` (not yet resolved to a class)
    SelfType,
    /// `static` / `$this`
    Static,
    /// `parent`
    Parent,
    /// A template parameter (`@template T`).
    Template(Arc<str>),
    /// A literal value type (`'foo'`, `42`).
    Literal(String),
    /// `Closure(int): string` / `callable(int): string`
    Closure(Vec<Type>, Option<Box<Type>>),
    Union(Vec<Type>),
    Intersection(Vec<Type>),
}

impl Type {
    pub fn named(name: &str) -> Type {
        Type::Named(Fqn::new(name), Vec::new())
    }

    pub fn array_of(value: Type) -> Type {
        Type::Array(None, Some(Box::new(value)))
    }

    pub fn nullable(self) -> Type {
        Type::union(vec![self, Type::Null])
    }

    /// Builds a normalized union (flattened, deduplicated).
    pub fn union(types: Vec<Type>) -> Type {
        let mut out: Vec<Type> = Vec::new();
        for t in types {
            match t {
                Type::Union(inner) => {
                    for i in inner {
                        if !out.contains(&i) {
                            out.push(i);
                        }
                    }
                }
                t => {
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
            }
        }
        if out.contains(&Type::Mixed) {
            return Type::Mixed;
        }
        if out.contains(&Type::True) && out.contains(&Type::False) {
            out.retain(|t| !matches!(t, Type::True | Type::False));
            out.push(Type::Bool);
        }
        match out.len() {
            0 => Type::Never,
            1 => out.pop().unwrap(),
            _ => Type::Union(out),
        }
    }

    pub fn is_mixed(&self) -> bool {
        matches!(self, Type::Mixed)
    }

    pub fn is_nullable(&self) -> bool {
        match self {
            Type::Null | Type::Mixed => true,
            Type::Union(ts) => ts.iter().any(Type::is_nullable),
            _ => false,
        }
    }

    pub fn without_null(&self) -> Type {
        match self {
            Type::Union(ts) => Type::union(ts.iter().filter(|t| **t != Type::Null).cloned().collect()),
            t => t.clone(),
        }
    }

    /// All union members (or the type itself).
    pub fn members(&self) -> Vec<&Type> {
        match self {
            Type::Union(ts) => ts.iter().flat_map(|t| t.members()).collect(),
            t => vec![t],
        }
    }

    /// Class names this type refers to at the top level (unions and
    /// intersections are flattened). `self`/`static`/`parent` must have been
    /// resolved with [`Type::resolve_relative`] first.
    pub fn class_names(&self) -> Vec<Fqn> {
        let mut out = Vec::new();
        self.collect_class_names(&mut out);
        out
    }

    fn collect_class_names(&self, out: &mut Vec<Fqn>) {
        match self {
            Type::Named(n, _) => {
                if !out.iter().any(|o: &Fqn| o.eq_ignore_case(n)) {
                    out.push(n.clone());
                }
            }
            Type::Union(ts) | Type::Intersection(ts) => ts.iter().for_each(|t| t.collect_class_names(out)),
            _ => {}
        }
    }

    /// Replaces `self`, `static` and `parent` by concrete classes.
    pub fn resolve_relative(
        &self,
        self_class: Option<&Fqn>,
        static_class: Option<&Fqn>,
        parent: Option<&Fqn>,
    ) -> Type {
        self.map(&mut |t| match t {
            Type::SelfType => self_class.map(|c| Type::Named(c.clone(), Vec::new())),
            Type::Static => static_class.or(self_class).map(|c| Type::Named(c.clone(), Vec::new())),
            Type::Parent => parent.map(|c| Type::Named(c.clone(), Vec::new())),
            _ => None,
        })
    }

    /// Substitutes template parameters.
    pub fn substitute(&self, subst: &dyn Fn(&str) -> Option<Type>) -> Type {
        self.map(&mut |t| match t {
            Type::Template(name) => subst(name),
            _ => None,
        })
    }

    /// Structural map: `f` returns `Some` to replace a node.
    pub fn map(&self, f: &mut dyn FnMut(&Type) -> Option<Type>) -> Type {
        if let Some(t) = f(self) {
            return t;
        }
        let bx = |t: &Option<Box<Type>>, f: &mut dyn FnMut(&Type) -> Option<Type>| {
            t.as_ref().map(|t| Box::new(t.map(f)))
        };
        match self {
            Type::Iterable(k, v) => Type::Iterable(bx(k, f), bx(v, f)),
            Type::Array(k, v) => Type::Array(bx(k, f), bx(v, f)),
            Type::List(v) => Type::List(bx(v, f)),
            Type::Shape(entries) => Type::Shape(
                entries
                    .iter()
                    .map(|e| ShapeEntry { key: e.key.clone(), ty: e.ty.map(f), optional: e.optional })
                    .collect(),
            ),
            Type::ClassString(t) => Type::ClassString(bx(t, f)),
            Type::Named(n, args) => Type::Named(n.clone(), args.iter().map(|a| a.map(f)).collect()),
            Type::Closure(params, ret) => {
                Type::Closure(params.iter().map(|p| p.map(f)).collect(), bx(ret, f))
            }
            Type::Union(ts) => Type::union(ts.iter().map(|t| t.map(f)).collect()),
            Type::Intersection(ts) => Type::Intersection(ts.iter().map(|t| t.map(f)).collect()),
            t => t.clone(),
        }
    }

    /// The element type when iterating with `foreach` (for array-like types).
    pub fn iterable_value(&self) -> Option<Type> {
        match self {
            Type::Array(_, v) | Type::Iterable(_, v) => v.as_deref().cloned(),
            Type::List(v) => v.as_deref().cloned(),
            Type::Shape(entries) => Some(Type::union(entries.iter().map(|e| e.ty.clone()).collect())),
            Type::Union(ts) => {
                let vs: Vec<Type> = ts.iter().filter_map(Type::iterable_value).collect();
                if vs.is_empty() {
                    None
                } else {
                    Some(Type::union(vs))
                }
            }
            _ => None,
        }
    }

    /// The key type when iterating.
    pub fn iterable_key(&self) -> Option<Type> {
        match self {
            Type::Array(k, _) | Type::Iterable(k, _) => k.as_deref().cloned(),
            Type::List(_) => Some(Type::Int),
            _ => None,
        }
    }

    /// Type of `$value[$key]` for a known string key (shapes) or any key.
    pub fn index(&self, key: Option<&str>) -> Option<Type> {
        match self {
            Type::Shape(entries) => {
                let key = key?;
                entries.iter().find(|e| e.key == key).map(|e| e.ty.clone())
            }
            Type::Union(ts) => {
                let vs: Vec<Type> = ts.iter().filter_map(|t| t.index(key)).collect();
                if vs.is_empty() {
                    None
                } else {
                    Some(Type::union(vs))
                }
            }
            t => t.iterable_value(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opt = |t: &Option<Box<Type>>| t.as_ref().map(|t| t.to_string());
        match self {
            Type::Mixed => f.write_str("mixed"),
            Type::Void => f.write_str("void"),
            Type::Never => f.write_str("never"),
            Type::Null => f.write_str("null"),
            Type::Bool => f.write_str("bool"),
            Type::True => f.write_str("true"),
            Type::False => f.write_str("false"),
            Type::Int => f.write_str("int"),
            Type::Float => f.write_str("float"),
            Type::String => f.write_str("string"),
            Type::Object => f.write_str("object"),
            Type::Callable => f.write_str("callable"),
            Type::Resource => f.write_str("resource"),
            Type::Iterable(k, v) => match (opt(k), opt(v)) {
                (Some(k), Some(v)) => write!(f, "iterable<{k}, {v}>"),
                (None, Some(v)) => write!(f, "iterable<{v}>"),
                _ => f.write_str("iterable"),
            },
            Type::Array(k, v) => match (opt(k), v.as_deref()) {
                (
                    None,
                    Some(
                        v @ (Type::Named(..)
                        | Type::Int
                        | Type::String
                        | Type::Float
                        | Type::Bool
                        | Type::Mixed),
                    ),
                ) => {
                    write!(f, "{v}[]")
                }
                (Some(k), Some(v)) => write!(f, "array<{k}, {v}>"),
                (None, Some(v)) => write!(f, "array<{v}>"),
                _ => f.write_str("array"),
            },
            Type::List(v) => match opt(v) {
                Some(v) => write!(f, "list<{v}>"),
                None => f.write_str("list"),
            },
            Type::Shape(entries) => {
                f.write_str("array{")?;
                for (i, e) in entries.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}{}: {}", e.key, if e.optional { "?" } else { "" }, e.ty)?;
                }
                f.write_str("}")
            }
            Type::ClassString(t) => match opt(t) {
                Some(t) => write!(f, "class-string<{t}>"),
                None => f.write_str("class-string"),
            },
            Type::Named(n, args) => {
                write!(f, "{n}")?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Type::SelfType => f.write_str("self"),
            Type::Static => f.write_str("static"),
            Type::Parent => f.write_str("parent"),
            Type::Template(n) => f.write_str(n),
            Type::Literal(l) => f.write_str(l),
            Type::Closure(params, ret) => {
                f.write_str("Closure(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str(")")?;
                if let Some(r) = ret {
                    write!(f, ": {r}")?;
                }
                Ok(())
            }
            Type::Union(ts) => {
                // `?T` for simple nullable types
                if ts.len() == 2 && ts.contains(&Type::Null) {
                    let other = ts.iter().find(|t| **t != Type::Null).unwrap();
                    if !matches!(other, Type::Intersection(_) | Type::Closure(..)) {
                        return write!(f, "?{other}");
                    }
                }
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        f.write_str("|")?;
                    }
                    match t {
                        Type::Intersection(_) | Type::Closure(..) => write!(f, "({t})")?,
                        _ => write!(f, "{t}")?,
                    }
                }
                Ok(())
            }
            Type::Intersection(ts) => {
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        f.write_str("&")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
        }
    }
}

/// Maps a builtin (non-class) type keyword to its type.
pub fn builtin_type(name: &str) -> Option<Type> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "mixed" => Type::Mixed,
        "void" => Type::Void,
        "never" | "never-return" | "never-returns" | "no-return" | "noreturn" => Type::Never,
        "null" => Type::Null,
        "bool" | "boolean" => Type::Bool,
        "true" => Type::True,
        "false" => Type::False,
        "int" | "integer" | "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
        | "non-zero-int" | "literal-int" => Type::Int,
        "float" | "double" => Type::Float,
        "string"
        | "non-empty-string"
        | "numeric-string"
        | "literal-string"
        | "non-falsy-string"
        | "truthy-string"
        | "lowercase-string"
        | "non-empty-lowercase-string"
        | "callable-string"
        | "trait-string"
        | "interface-string"
        | "enum-string"
        | "non-empty-literal-string" => Type::String,
        "object" => Type::Object,
        "callable" | "pure-callable" | "callable-array" | "callable-object" => Type::Callable,
        "iterable" => Type::Iterable(None, None),
        "resource" | "closed-resource" | "open-resource" => Type::Resource,
        "array" | "non-empty-array" | "associative-array" => Type::Array(None, None),
        "list" | "non-empty-list" => Type::List(None),
        "array-key" => Type::Union(vec![Type::Int, Type::String]),
        "scalar" => Type::Union(vec![Type::Int, Type::Float, Type::String, Type::Bool]),
        "numeric" => Type::Union(vec![Type::Int, Type::Float, Type::String]),
        "number" => Type::Union(vec![Type::Int, Type::Float]),
        "class-string" => Type::ClassString(None),
        "self" => Type::SelfType,
        "static" | "$this" => Type::Static,
        "parent" => Type::Parent,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_normalizes() {
        let t =
            Type::union(vec![Type::Int, Type::Union(vec![Type::Int, Type::Null]), Type::True, Type::False]);
        assert_eq!(t.to_string(), "int|null|bool");
        assert_eq!(Type::named("A\\B").nullable().to_string(), "?A\\B");
        assert_eq!(Type::array_of(Type::named("User")).to_string(), "User[]");
    }
}
