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

/// The range and current name of the symbol that would be renamed.
fn classify_renamable(sema: &Sema, offset: TextSize) -> Result<(Definition, TextRange), RenameError> {
    let (def, range) =
        classify(sema, offset).ok_or_else(|| RenameError("No symbol to rename here.".into()))?;
    let text = &sema.text[range];
    if text == "$this" || ["self", "static", "parent"].iter().any(|k| text.eq_ignore_ascii_case(k)) {
        return Err(RenameError(format!("Cannot rename {text}.")));
    }
    Ok((def, range))
}

pub(crate) fn prepare_rename(sema: &Sema, offset: TextSize) -> Result<(TextRange, String), RenameError> {
    let (def, range) = classify_renamable(sema, offset)?;
    check_renamable(sema, &def)?;
    let text = sema.text[range].to_string();
    Ok((range, text))
}

fn check_renamable(sema: &Sema, def: &Definition) -> Result<(), RenameError> {
    if matches!(def, Definition::Namespace(_)) {
        return Err(RenameError("Renaming namespaces is not supported.".into()));
    }
    if let Definition::Variable { name, .. } = def {
        if name == "this" {
            return Err(RenameError("Cannot rename $this.".into()));
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
    let (def, _) = classify_renamable(sema, offset)?;
    check_renamable(sema, &def)?;
    let bare = new_name.trim_start_matches('$');
    if !valid_identifier(bare) {
        return Err(RenameError(format!("'{new_name}' is not a valid name.")));
    }
    let old = def.search_name();
    let mut change = SourceChange::default();
    for r in references(sema, offset, true) {
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
