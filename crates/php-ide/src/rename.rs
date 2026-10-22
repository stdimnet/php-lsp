//! Rename refactoring.

use php_index::{FileId, FileKind};
use php_syntax::{TextRange, TextSize};

use crate::classify::{classify, Definition};
use crate::completion::TextEdit;
use crate::navigation::references;
use crate::sema::Sema;

/// Edits for several files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceChange {
    pub edits: Vec<(FileId, Vec<TextEdit>)>,
}

impl SourceChange {
    pub fn single(file: FileId, edit: TextEdit) -> SourceChange {
        SourceChange { edits: vec![(file, vec![edit])] }
    }

    pub fn push(&mut self, file: FileId, edit: TextEdit) {
        match self.edits.iter_mut().find(|(f, _)| *f == file) {
            Some((_, v)) => v.push(edit),
            None => self.edits.push((file, vec![edit])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameError(pub String);

const MAGIC_METHODS: &[&str] = &[
    "__construct",
    "__destruct",
    "__call",
    "__callStatic",
    "__get",
    "__set",
    "__isset",
    "__unset",
    "__sleep",
    "__wakeup",
    "__serialize",
    "__unserialize",
    "__toString",
    "__invoke",
    "__set_state",
    "__clone",
    "__debugInfo",
];

const SUPERGLOBALS: &[&str] =
    &["GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV"];

/// The range and current name of the symbol that would be renamed.
fn classify_renamable(sema: &Sema, offset: TextSize) -> Result<(Definition, TextRange), RenameError> {
    let (def, range) =
        classify(sema, offset).ok_or_else(|| RenameError("No symbol to rename here.".into()))?;
    let text = &sema.text[range];
    if text == "$this" || ["self", "static", "parent", "class"].iter().any(|k| text.eq_ignore_ascii_case(k)) {
        return Err(RenameError(format!("Cannot rename {text}.")));
    }
    if let Definition::Method { name, .. } = &def {
        if MAGIC_METHODS.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            return Err(RenameError(format!("Cannot rename the magic method {name}.")));
        }
    }
    Ok((def, range))
}

pub(crate) fn prepare_rename(sema: &Sema, offset: TextSize) -> Result<(TextRange, String), RenameError> {
    let (def, range) = classify_renamable(sema, offset)?;
    check_renamable_at(sema, &def, import_alias(sema, &def, range).as_deref())?;
    let text = sema.text[range].to_string();
    Ok((range, text))
}

/// `alias`: the name at the cursor is an import alias (only this file
/// changes, so the class may be declared anywhere).
fn check_renamable_at(sema: &Sema, def: &Definition, alias: Option<&str>) -> Result<(), RenameError> {
    if alias.is_some() {
        return Ok(());
    }
    if matches!(def, Definition::Namespace(_)) {
        return Err(RenameError("Renaming namespaces is not supported.".into()));
    }
    if let Definition::Variable { name, .. } = def {
        if name == "this" || SUPERGLOBALS.contains(&name.as_str()) {
            return Err(RenameError(format!("Cannot rename ${name}.")));
        }
        return Ok(());
    }
    // declarations outside of the workspace cannot be changed
    for loc in crate::navigation::declarations(sema, def, None) {
        match sema.index().file_kind(loc.file) {
            Some(FileKind::Stub) => return Err(RenameError("Cannot rename a built-in symbol.".into())),
            Some(FileKind::Vendor) => {
                return Err(RenameError("Cannot rename a symbol declared in a dependency.".into()))
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c as u32 >= 0x80)
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c as u32 >= 0x80)
}

pub(crate) fn rename(sema: &Sema, offset: TextSize, new_name: &str) -> Result<SourceChange, RenameError> {
    let (def, range) = classify_renamable(sema, offset)?;
    let alias = import_alias(sema, &def, range);
    check_renamable_at(sema, &def, alias.as_deref())?;
    let bare = new_name.trim_start_matches('$');
    if !valid_identifier(bare) {
        return Err(RenameError(format!("'{new_name}' is not a valid name.")));
    }
    if let Some(alias) = alias {
        return rename_alias(sema, &def, &alias, bare);
    }
    let old = def.search_name();
    let mut change = SourceChange::default();
    let mut refs = references(sema, offset, true);
    // in a constructor, a promoted parameter is also the property
    if let Some(param) = promoted_param(sema, &def) {
        refs.extend(references(sema, param, true));
        refs.sort_by_key(|r| (r.file, r.range.start()));
        refs.dedup();
    }
    for r in refs {
        let Some(file) = sema.ws.file(r.file) else { continue };
        let text = &file.text[r.range];
        let has_dollar = text.starts_with('$');
        // aliased imports (`use A\B as C`) keep the alias
        if !text.trim_start_matches('$').eq_ignore_ascii_case(&old) {
            continue;
        }
        let replacement = if has_dollar { format!("${bare}") } else { bare.to_string() };
        change.push(r.file, TextEdit { range: r.range, new_text: replacement });
    }
    if change.edits.is_empty() {
        return Err(RenameError("No references found.".into()));
    }
    Ok(change)
}

/// The import alias at `range` (`use A\\B as C;` or a use of `C`).
fn import_alias(sema: &Sema, def: &Definition, range: TextRange) -> Option<String> {
    let Definition::Class(fqn) = def else { return None };
    let text = &sema.text[range];
    if text.eq_ignore_ascii_case(fqn.short()) {
        return None;
    }
    // the name is an alias when a `use ... as <text>` clause exists
    let declared =
        sema.root.descendants().filter(|n| n.kind() == php_syntax::SyntaxKind::USE_CLAUSE).any(|u| {
            u.descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .skip_while(|t| t.kind() != php_syntax::SyntaxKind::AS_KW)
                .any(|t| t.text().eq_ignore_ascii_case(text))
        });
    declared.then(|| text.to_string())
}

/// Renames an import alias and its uses in this file.
fn rename_alias(
    sema: &Sema,
    def: &Definition,
    alias: &str,
    new_name: &str,
) -> Result<SourceChange, RenameError> {
    use php_syntax::SyntaxKind::*;
    let mut change = SourceChange::default();
    for token in sema.root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if token.kind() == DOC_COMMENT {
            for r in crate::doc_types::doc_type_refs(&token) {
                let first = r.name.trim_start_matches('\\').split('\\').next().unwrap_or("");
                if r.name.starts_with('\\') || !first.eq_ignore_ascii_case(alias) {
                    continue;
                }
                let len = TextSize::from(alias.len() as u32);
                change.push(
                    sema.file,
                    TextEdit { range: TextRange::at(r.range.start(), len), new_text: new_name.into() },
                );
            }
            continue;
        }
        if token.kind() != IDENT || !token.text().eq_ignore_ascii_case(alias) {
            continue;
        }
        let in_use_alias = token.parent_ancestors().any(|a| a.kind() == USE_CLAUSE)
            && std::iter::successors(token.prev_token(), |t| t.prev_token())
                .find(|t| t.kind() != WHITESPACE)
                .is_some_and(|t| t.kind() == AS_KW);
        let same = crate::classify::classify_token(sema, &token).is_some_and(|d| d.same_as(def));
        // only the first segment of a name is the alias
        let first_segment = token.prev_token().is_none_or(|t| t.kind() != BACKSLASH);
        if in_use_alias || same && first_segment {
            change.push(sema.file, TextEdit { range: token.text_range(), new_text: new_name.into() });
        }
    }
    if change.edits.is_empty() {
        return Err(RenameError("No references found.".into()));
    }
    Ok(change)
}

/// The promoted constructor parameter a variable of the constructor is.
fn promoted_param(sema: &Sema, def: &Definition) -> Option<TextSize> {
    use php_syntax::SyntaxKind::*;
    let Definition::Variable { name, scope } = def else { return None };
    let method = sema.root.descendants().find(|n| n.text_range() == *scope && n.kind() == METHOD_DECL)?;
    let param = method.descendants().filter(|n| n.kind() == PARAM).find(|p| {
        p.children().any(|c| c.kind() == MODIFIER_LIST)
            && p.children_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == VARIABLE && t.text()[1..] == **name))
    })?;
    let token = param.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == VARIABLE)?;
    Some(token.text_range().start() + TextSize::from(1))
}
