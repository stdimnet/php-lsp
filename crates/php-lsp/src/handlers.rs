//! Request handlers for language features.

use lsp_server::Request;
use lsp_types as lsp;
use php_ide::{CompletionKind, FileId, ReferenceKind, TextSize};
use serde_json::Value;

use crate::convert;
use crate::server::{Result, Server};

/// Handles a request; `None` means the response was already sent.
pub(crate) fn dispatch(s: &mut Server, req: Request) -> Option<Result<Value>> {
    use lsp::request::*;
    let r = match req.method.as_str() {
        DocumentSymbolRequest::METHOD => handle(s, req, document_symbols),
        FoldingRangeRequest::METHOD => handle(s, req, folding_ranges),
        SelectionRangeRequest::METHOD => handle(s, req, selection_ranges),
        HoverRequest::METHOD => handle(s, req, hover),
        GotoDefinition::METHOD | GotoDeclaration::METHOD => handle(s, req, definition),
        GotoTypeDefinition::METHOD => handle(s, req, type_definition),
        GotoImplementation::METHOD => handle(s, req, implementation),
        References::METHOD => handle(s, req, references),
        DocumentHighlightRequest::METHOD => handle(s, req, document_highlight),
        Completion::METHOD => handle(s, req, completion),
        SignatureHelpRequest::METHOD => handle(s, req, signature_help),
        WorkspaceSymbolRequest::METHOD => handle(s, req, workspace_symbol),
        InlayHintRequest::METHOD => handle(s, req, inlay_hints),
        PrepareRenameRequest::METHOD => handle(s, req, prepare_rename),
        Rename::METHOD => handle(s, req, rename),
        CodeActionRequest::METHOD => handle(s, req, code_action),
        CallHierarchyPrepare::METHOD => handle(s, req, call_hierarchy_prepare),
        CallHierarchyIncomingCalls::METHOD => handle(s, req, incoming_calls),
        CallHierarchyOutgoingCalls::METHOD => handle(s, req, outgoing_calls),
        TypeHierarchyPrepare::METHOD => handle(s, req, type_hierarchy_prepare),
        TypeHierarchySupertypes::METHOD => handle(s, req, supertypes),
        TypeHierarchySubtypes::METHOD => handle(s, req, subtypes),
        CodeLensRequest::METHOD => handle(s, req, code_lens),
        CodeLensResolve::METHOD => handle(s, req, code_lens_resolve),
        SemanticTokensFullRequest::METHOD => handle(s, req, semantic_tokens_full),
        SemanticTokensRangeRequest::METHOD => handle(s, req, semantic_tokens_range),
        crate::ext::SyntaxTree::METHOD => handle(s, req, syntax_tree),
        _ => {
            s.respond_error(
                req.id,
                lsp_server::ErrorCode::MethodNotFound,
                &format!("unhandled method {}", req.method),
            );
            return None;
        }
    };
    Some(r)
}

fn handle<P, R>(s: &mut Server, req: Request, f: fn(&mut Server, P) -> R) -> Result<Value>
where
    P: serde::de::DeserializeOwned,
    R: serde::Serialize,
{
    let params: P = serde_json::from_value(req.params)?;
    Ok(serde_json::to_value(f(s, params))?)
}

fn position(s: &Server, p: &lsp::TextDocumentPositionParams) -> Option<(FileId, TextSize)> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    Some((id, convert::offset(&index, s.encoding, p.position)))
}

fn document_symbols(s: &mut Server, p: lsp::DocumentSymbolParams) -> Option<lsp::DocumentSymbolResponse> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let symbols = s.workspace.document_symbols(id);
    Some(lsp::DocumentSymbolResponse::Nested(
        symbols.into_iter().map(|sym| convert::document_symbol(&index, s.encoding, sym)).collect(),
    ))
}

fn folding_ranges(s: &mut Server, p: lsp::FoldingRangeParams) -> Option<Vec<lsp::FoldingRange>> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let text = s.workspace.file(id)?.text.clone();
    Some(
        s.workspace
            .folding_ranges(id)
            .into_iter()
            .filter_map(|f| convert::folding_range(&index, &text, f))
            .collect(),
    )
}

fn selection_ranges(s: &mut Server, p: lsp::SelectionRangeParams) -> Option<Vec<lsp::SelectionRange>> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let offsets: Vec<_> = p.positions.iter().map(|&pos| convert::offset(&index, s.encoding, pos)).collect();
    let result = s
        .workspace
        .selection_ranges(id, &offsets)
        .into_iter()
        .map(|ranges| {
            let mut parent: Option<Box<lsp::SelectionRange>> = None;
            for r in ranges.into_iter().rev() {
                parent = Some(Box::new(lsp::SelectionRange {
                    range: convert::range(&index, s.encoding, r),
                    parent,
                }));
            }
            parent.map(|b| *b).unwrap_or(lsp::SelectionRange { range: lsp::Range::default(), parent: None })
        })
        .collect();
    Some(result)
}

fn hover(s: &mut Server, p: lsp::HoverParams) -> Option<lsp::Hover> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let h = s.workspace.hover(id, offset)?;
    let index = s.workspace.line_index(id)?;
    let contents = if s.caps.markdown {
        lsp::HoverContents::Markup(lsp::MarkupContent { kind: lsp::MarkupKind::Markdown, value: h.markdown })
    } else {
        lsp::HoverContents::Markup(lsp::MarkupContent { kind: lsp::MarkupKind::PlainText, value: h.markdown })
    };
    Some(lsp::Hover { contents, range: Some(convert::range(&index, s.encoding, h.range)) })
}

fn locations(s: &Server, locs: Vec<php_ide::Location>) -> Option<lsp::GotoDefinitionResponse> {
    if locs.is_empty() {
        return None;
    }
    if s.caps.location_links {
        let links: Vec<lsp::LocationLink> = locs
            .into_iter()
            .filter_map(|l| {
                let full = s.location(l.file, l.range)?;
                let focus = s.location(l.file, l.focus)?;
                Some(lsp::LocationLink {
                    origin_selection_range: None,
                    target_uri: full.uri,
                    target_range: full.range,
                    target_selection_range: focus.range,
                })
            })
            .collect();
        return Some(lsp::GotoDefinitionResponse::Link(links));
    }
    let out: Vec<lsp::Location> = locs.into_iter().filter_map(|l| s.location(l.file, l.focus)).collect();
    Some(lsp::GotoDefinitionResponse::Array(out))
}

fn definition(s: &mut Server, p: lsp::GotoDefinitionParams) -> Option<lsp::GotoDefinitionResponse> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let locs = s.workspace.goto_definition(id, offset);
    locations(s, locs)
}

fn type_definition(s: &mut Server, p: lsp::GotoDefinitionParams) -> Option<lsp::GotoDefinitionResponse> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let locs = s.workspace.goto_type_definition(id, offset);
    locations(s, locs)
}

fn implementation(s: &mut Server, p: lsp::GotoDefinitionParams) -> Option<lsp::GotoDefinitionResponse> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let locs = s.workspace.goto_implementation(id, offset);
    locations(s, locs)
}

fn references(s: &mut Server, p: lsp::ReferenceParams) -> Option<Vec<lsp::Location>> {
    let (id, offset) = position(s, &p.text_document_position)?;
    let refs = s.workspace.references(id, offset, p.context.include_declaration);
    Some(refs.into_iter().filter_map(|r| s.location(r.file, r.range)).collect())
}

fn document_highlight(
    s: &mut Server,
    p: lsp::DocumentHighlightParams,
) -> Option<Vec<lsp::DocumentHighlight>> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let index = s.workspace.line_index(id)?;
    Some(
        s.workspace
            .document_highlights(id, offset)
            .into_iter()
            .map(|r| lsp::DocumentHighlight {
                range: convert::range(&index, s.encoding, r.range),
                kind: Some(match r.kind {
                    ReferenceKind::Write | ReferenceKind::Declaration => lsp::DocumentHighlightKind::WRITE,
                    ReferenceKind::Read => lsp::DocumentHighlightKind::READ,
                }),
            })
            .collect(),
    )
}

fn completion_kind(k: CompletionKind) -> lsp::CompletionItemKind {
    match k {
        CompletionKind::Class | CompletionKind::Trait => lsp::CompletionItemKind::CLASS,
        CompletionKind::Interface => lsp::CompletionItemKind::INTERFACE,
        CompletionKind::Enum => lsp::CompletionItemKind::ENUM,
        CompletionKind::EnumMember => lsp::CompletionItemKind::ENUM_MEMBER,
        CompletionKind::Method => lsp::CompletionItemKind::METHOD,
        CompletionKind::Function => lsp::CompletionItemKind::FUNCTION,
        CompletionKind::Property => lsp::CompletionItemKind::PROPERTY,
        CompletionKind::Constant => lsp::CompletionItemKind::CONSTANT,
        CompletionKind::Variable => lsp::CompletionItemKind::VARIABLE,
        CompletionKind::Keyword => lsp::CompletionItemKind::KEYWORD,
        CompletionKind::Namespace => lsp::CompletionItemKind::MODULE,
        CompletionKind::Snippet => lsp::CompletionItemKind::SNIPPET,
    }
}

fn completion(s: &mut Server, p: lsp::CompletionParams) -> Option<lsp::CompletionResponse> {
    let (id, offset) = position(s, &p.text_document_position)?;
    let index = s.workspace.line_index(id)?;
    let list = s.workspace.completions(id, offset);
    let enc = s.encoding;
    let items = list
        .items
        .into_iter()
        .map(|c| {
            let (text, format) = if c.is_snippet && s.caps.snippets {
                (c.insert_text.clone(), lsp::InsertTextFormat::SNIPPET)
            } else {
                (c.insert_text.replace("$0", ""), lsp::InsertTextFormat::PLAIN_TEXT)
            };
            let command = (c.trigger_signature_help && s.caps.vscode).then(|| lsp::Command {
                title: "Trigger parameter hints".into(),
                command: "editor.action.triggerParameterHints".into(),
                arguments: None,
            });
            lsp::CompletionItem {
                label: c.label.clone(),
                kind: Some(completion_kind(c.kind)),
                detail: c.detail,
                documentation: c.documentation.map(|d| {
                    lsp::Documentation::MarkupContent(lsp::MarkupContent {
                        kind: lsp::MarkupKind::Markdown,
                        value: d,
                    })
                }),
                tags: c.deprecated.then(|| vec![lsp::CompletionItemTag::DEPRECATED]),
                sort_text: c.sort_text,
                filter_text: c.filter_text,
                insert_text_format: Some(format),
                text_edit: Some(lsp::CompletionTextEdit::Edit(lsp::TextEdit {
                    range: convert::range(&index, enc, c.replace_range),
                    new_text: text,
                })),
                additional_text_edits: if c.additional_edits.is_empty() {
                    None
                } else {
                    Some(
                        c.additional_edits
                            .into_iter()
                            .map(|e| lsp::TextEdit {
                                range: convert::range(&index, enc, e.range),
                                new_text: e.new_text,
                            })
                            .collect(),
                    )
                },
                command,
                ..Default::default()
            }
        })
        .collect();
    Some(lsp::CompletionResponse::List(lsp::CompletionList { is_incomplete: list.incomplete, items }))
}

fn signature_help(s: &mut Server, p: lsp::SignatureHelpParams) -> Option<lsp::SignatureHelp> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    let sh = s.workspace.signature_help(id, offset)?;
    Some(lsp::SignatureHelp {
        signatures: sh
            .signatures
            .into_iter()
            .map(|sig| lsp::SignatureInformation {
                label: sig.label,
                documentation: sig.documentation.map(|d| {
                    lsp::Documentation::MarkupContent(lsp::MarkupContent {
                        kind: lsp::MarkupKind::Markdown,
                        value: d,
                    })
                }),
                parameters: Some(
                    sig.parameters
                        .into_iter()
                        .map(|p| lsp::ParameterInformation {
                            label: lsp::ParameterLabel::Simple(p.label),
                            documentation: p.documentation.map(lsp::Documentation::String),
                        })
                        .collect(),
                ),
                active_parameter: None,
            })
            .collect(),
        active_signature: Some(sh.active_signature as u32),
        active_parameter: sh.active_parameter.map(|a| a as u32),
    })
}

#[allow(deprecated)]
fn workspace_symbol(s: &mut Server, p: lsp::WorkspaceSymbolParams) -> Option<lsp::WorkspaceSymbolResponse> {
    let symbols = s.workspace.workspace_symbols(&p.query, 200);
    Some(lsp::WorkspaceSymbolResponse::Flat(
        symbols
            .into_iter()
            .filter_map(|sym| {
                Some(lsp::SymbolInformation {
                    name: sym.name,
                    kind: convert::symbol_kind(sym.kind),
                    tags: sym.deprecated.then(|| vec![lsp::SymbolTag::DEPRECATED]),
                    deprecated: None,
                    location: s.location(sym.file, sym.range)?,
                    container_name: sym.container,
                })
            })
            .collect(),
    ))
}

fn syntax_tree(s: &mut Server, p: crate::ext::SyntaxTreeParams) -> Option<String> {
    let id = s.file_for(&p.text_document.uri)?;
    s.workspace.syntax_tree(id)
}

fn inlay_hints(s: &mut Server, p: lsp::InlayHintParams) -> Option<Vec<lsp::InlayHint>> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let range = convert::text_range(&index, s.encoding, p.range);
    Some(
        s.workspace
            .inlay_hints(id, range)
            .into_iter()
            .map(|h| lsp::InlayHint {
                position: convert::position(&index, s.encoding, h.offset),
                label: lsp::InlayHintLabel::String(h.label),
                kind: Some(match h.kind {
                    php_ide::InlayHintKind::Parameter => lsp::InlayHintKind::PARAMETER,
                    php_ide::InlayHintKind::Type => lsp::InlayHintKind::TYPE,
                }),
                text_edits: None,
                tooltip: None,
                padding_left: Some(h.padding_left),
                padding_right: Some(h.padding_right),
                data: None,
            })
            .collect(),
    )
}

pub(crate) fn semantic_tokens_legend() -> lsp::SemanticTokensLegend {
    lsp::SemanticTokensLegend {
        token_types: php_ide::TOKEN_TYPES.iter().map(|t| lsp::SemanticTokenType::new(t)).collect(),
        token_modifiers: php_ide::TOKEN_MODIFIERS
            .iter()
            .map(|m| lsp::SemanticTokenModifier::new(m))
            .collect(),
    }
}

fn encode_tokens(s: &Server, id: FileId, range: Option<php_ide::TextRange>) -> Option<lsp::SemanticTokens> {
    let index = s.workspace.line_index(id)?;
    let mut data = Vec::new();
    let (mut prev_line, mut prev_col) = (0u32, 0u32);
    for t in s.workspace.semantic_tokens(id, range) {
        let start = convert::position(&index, s.encoding, t.range.start());
        let end = convert::position(&index, s.encoding, t.range.end());
        if end.line != start.line {
            continue;
        }
        let delta_line = start.line - prev_line;
        let delta_start = if delta_line == 0 { start.character - prev_col } else { start.character };
        data.push(lsp::SemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type: t.token_type as u32,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = start.line;
        prev_col = start.character;
    }
    Some(lsp::SemanticTokens { result_id: None, data })
}

fn semantic_tokens_full(s: &mut Server, p: lsp::SemanticTokensParams) -> Option<lsp::SemanticTokensResult> {
    let id = s.file_for(&p.text_document.uri)?;
    encode_tokens(s, id, None).map(lsp::SemanticTokensResult::Tokens)
}

fn semantic_tokens_range(
    s: &mut Server,
    p: lsp::SemanticTokensRangeParams,
) -> Option<lsp::SemanticTokensRangeResult> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let range = convert::text_range(&index, s.encoding, p.range);
    encode_tokens(s, id, Some(range)).map(lsp::SemanticTokensRangeResult::Tokens)
}

// ---------------------------------------------------------------------- rename

fn workspace_edit(s: &Server, change: php_ide::SourceChange) -> Option<lsp::WorkspaceEdit> {
    let mut changes = std::collections::HashMap::new();
    for (file, edits) in change.edits {
        let index = s.workspace.line_index(file)?;
        let uri = s.uri_for(file)?;
        let edits: Vec<lsp::TextEdit> = edits
            .into_iter()
            .map(|e| lsp::TextEdit {
                range: convert::range(&index, s.encoding, e.range),
                new_text: e.new_text,
            })
            .collect();
        changes.insert(uri, edits);
    }
    Some(lsp::WorkspaceEdit { changes: Some(changes), document_changes: None, change_annotations: None })
}

fn prepare_rename(s: &mut Server, p: lsp::TextDocumentPositionParams) -> Option<lsp::PrepareRenameResponse> {
    let (id, offset) = position(s, &p)?;
    let index = s.workspace.line_index(id)?;
    match s.workspace.prepare_rename(id, offset) {
        Ok((range, text)) => Some(lsp::PrepareRenameResponse::RangeWithPlaceholder {
            range: convert::range(&index, s.encoding, range),
            placeholder: text,
        }),
        Err(e) => {
            s.show_message(&e.0);
            None
        }
    }
}

fn rename(s: &mut Server, p: lsp::RenameParams) -> Option<lsp::WorkspaceEdit> {
    let (id, offset) = position(s, &p.text_document_position)?;
    match s.workspace.rename(id, offset, &p.new_name) {
        Ok(change) => workspace_edit(s, change),
        Err(e) => {
            s.show_message(&e.0);
            None
        }
    }
}

// ---------------------------------------------------------------------- code actions

fn code_action(s: &mut Server, p: lsp::CodeActionParams) -> Option<Vec<lsp::CodeActionOrCommand>> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    let range = convert::text_range(&index, s.encoding, p.range);
    let only = p.context.only.clone();
    let actions = s.workspace.code_actions(id, range);
    let mut out = Vec::new();
    for a in actions {
        let kind = match a.kind {
            php_ide::CodeActionKind::QuickFix => lsp::CodeActionKind::QUICKFIX,
            php_ide::CodeActionKind::Refactor => lsp::CodeActionKind::REFACTOR,
            php_ide::CodeActionKind::RefactorRewrite => lsp::CodeActionKind::REFACTOR_REWRITE,
            php_ide::CodeActionKind::Source => lsp::CodeActionKind::SOURCE,
            php_ide::CodeActionKind::SourceOrganizeImports => lsp::CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
        };
        if let Some(only) = &only {
            if !only.iter().any(|o| kind.as_str().starts_with(o.as_str())) {
                continue;
            }
        }
        let diagnostics: Vec<lsp::Diagnostic> = a
            .fixes
            .map(|code| {
                p.context
                    .diagnostics
                    .iter()
                    .filter(|d| d.code == Some(lsp::NumberOrString::String(code.as_str().to_string())))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        out.push(lsp::CodeActionOrCommand::CodeAction(lsp::CodeAction {
            title: a.title,
            kind: Some(kind),
            diagnostics: if diagnostics.is_empty() { None } else { Some(diagnostics) },
            edit: workspace_edit(s, a.change),
            command: None,
            is_preferred: Some(a.is_preferred),
            disabled: None,
            data: None,
        }));
    }
    Some(out)
}

// ---------------------------------------------------------------------- hierarchies

fn symbol_kind_from_lsp(k: lsp::SymbolKind) -> php_ide::SymbolKind {
    match k {
        lsp::SymbolKind::INTERFACE => php_ide::SymbolKind::Interface,
        lsp::SymbolKind::ENUM => php_ide::SymbolKind::Enum,
        lsp::SymbolKind::METHOD => php_ide::SymbolKind::Method,
        lsp::SymbolKind::CONSTRUCTOR => php_ide::SymbolKind::Constructor,
        lsp::SymbolKind::FUNCTION => php_ide::SymbolKind::Function,
        lsp::SymbolKind::NAMESPACE => php_ide::SymbolKind::Namespace,
        _ => php_ide::SymbolKind::Class,
    }
}

fn to_lsp_item(s: &Server, item: &php_ide::HierarchyItem) -> Option<(lsp::Uri, lsp::Range, lsp::Range)> {
    let index = s.workspace.line_index(item.file)?;
    Some((
        s.uri_for(item.file)?,
        convert::range(&index, s.encoding, item.range),
        convert::range(&index, s.encoding, item.selection_range),
    ))
}

fn call_item(s: &Server, item: &php_ide::HierarchyItem) -> Option<lsp::CallHierarchyItem> {
    let (uri, range, selection_range) = to_lsp_item(s, item)?;
    Some(lsp::CallHierarchyItem {
        name: item.name.clone(),
        kind: convert::symbol_kind(item.kind),
        tags: None,
        detail: item.detail.clone(),
        uri,
        range,
        selection_range,
        data: None,
    })
}

fn from_lsp_item(
    s: &Server,
    uri: &lsp::Uri,
    name: &str,
    kind: lsp::SymbolKind,
    range: lsp::Range,
    sel: lsp::Range,
) -> Option<php_ide::HierarchyItem> {
    let file = s.file_for(uri)?;
    let index = s.workspace.line_index(file)?;
    Some(php_ide::HierarchyItem {
        name: name.to_string(),
        kind: symbol_kind_from_lsp(kind),
        detail: None,
        file,
        range: convert::text_range(&index, s.encoding, range),
        selection_range: convert::text_range(&index, s.encoding, sel),
    })
}

fn call_hierarchy_prepare(
    s: &mut Server,
    p: lsp::CallHierarchyPrepareParams,
) -> Option<Vec<lsp::CallHierarchyItem>> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    Some(s.workspace.prepare_call_hierarchy(id, offset).iter().filter_map(|i| call_item(s, i)).collect())
}

fn ranges_in(s: &Server, file: FileId, ranges: &[php_ide::TextRange]) -> Vec<lsp::Range> {
    let Some(index) = s.workspace.line_index(file) else { return Vec::new() };
    ranges.iter().map(|r| convert::range(&index, s.encoding, *r)).collect()
}

fn incoming_calls(
    s: &mut Server,
    p: lsp::CallHierarchyIncomingCallsParams,
) -> Option<Vec<lsp::CallHierarchyIncomingCall>> {
    let i = &p.item;
    let item = from_lsp_item(s, &i.uri, &i.name, i.kind, i.range, i.selection_range)?;
    Some(
        s.workspace
            .incoming_calls(&item)
            .iter()
            .filter_map(|c| {
                Some(lsp::CallHierarchyIncomingCall {
                    from: call_item(s, &c.item)?,
                    from_ranges: ranges_in(s, c.item.file, &c.ranges),
                })
            })
            .collect(),
    )
}

fn outgoing_calls(
    s: &mut Server,
    p: lsp::CallHierarchyOutgoingCallsParams,
) -> Option<Vec<lsp::CallHierarchyOutgoingCall>> {
    let i = &p.item;
    let item = from_lsp_item(s, &i.uri, &i.name, i.kind, i.range, i.selection_range)?;
    Some(
        s.workspace
            .outgoing_calls(&item)
            .iter()
            .filter_map(|c| {
                Some(lsp::CallHierarchyOutgoingCall {
                    to: call_item(s, &c.item)?,
                    from_ranges: ranges_in(s, item.file, &c.ranges),
                })
            })
            .collect(),
    )
}

fn type_item(s: &Server, item: &php_ide::HierarchyItem) -> Option<lsp::TypeHierarchyItem> {
    let (uri, range, selection_range) = to_lsp_item(s, item)?;
    Some(lsp::TypeHierarchyItem {
        name: item.name.clone(),
        kind: convert::symbol_kind(item.kind),
        tags: None,
        detail: item.detail.clone(),
        uri,
        range,
        selection_range,
        data: None,
    })
}

fn type_hierarchy_prepare(
    s: &mut Server,
    p: lsp::TypeHierarchyPrepareParams,
) -> Option<Vec<lsp::TypeHierarchyItem>> {
    let (id, offset) = position(s, &p.text_document_position_params)?;
    Some(s.workspace.prepare_type_hierarchy(id, offset).iter().filter_map(|i| type_item(s, i)).collect())
}

fn supertypes(s: &mut Server, p: lsp::TypeHierarchySupertypesParams) -> Option<Vec<lsp::TypeHierarchyItem>> {
    let i = &p.item;
    let item = from_lsp_item(s, &i.uri, &i.name, i.kind, i.range, i.selection_range)?;
    Some(s.workspace.supertypes(&item).iter().filter_map(|i| type_item(s, i)).collect())
}

fn subtypes(s: &mut Server, p: lsp::TypeHierarchySubtypesParams) -> Option<Vec<lsp::TypeHierarchyItem>> {
    let i = &p.item;
    let item = from_lsp_item(s, &i.uri, &i.name, i.kind, i.range, i.selection_range)?;
    Some(s.workspace.subtypes(&item).iter().filter_map(|i| type_item(s, i)).collect())
}

// ---------------------------------------------------------------------- code lens

fn code_lens(s: &mut Server, p: lsp::CodeLensParams) -> Option<Vec<lsp::CodeLens>> {
    let id = s.file_for(&p.text_document.uri)?;
    let index = s.workspace.line_index(id)?;
    Some(
        s.workspace
            .code_lenses(id)
            .into_iter()
            .map(|l| lsp::CodeLens {
                range: convert::range(&index, s.encoding, l.range),
                command: None,
                data: Some(serde_json::json!({
                    "uri": p.text_document.uri.as_str(),
                    "start": u32::from(l.range.start()),
                    "end": u32::from(l.range.end()),
                    "kind": match l.kind {
                        php_ide::CodeLensKind::References => "references",
                        php_ide::CodeLensKind::Implementations => "implementations",
                        php_ide::CodeLensKind::Parent => "parent",
                    },
                })),
            })
            .collect(),
    )
}

fn code_lens_resolve(s: &mut Server, mut lens: lsp::CodeLens) -> lsp::CodeLens {
    let resolved = (|| {
        let data = lens.data.as_ref()?;
        let uri: lsp::Uri = data.get("uri")?.as_str()?.parse().ok()?;
        let id = s.file_for(&uri)?;
        let start = TextSize::from(data.get("start")?.as_u64()? as u32);
        let end = TextSize::from(data.get("end")?.as_u64()? as u32);
        let kind = match data.get("kind")?.as_str()? {
            "references" => php_ide::CodeLensKind::References,
            "implementations" => php_ide::CodeLensKind::Implementations,
            _ => php_ide::CodeLensKind::Parent,
        };
        let r = s
            .workspace
            .resolve_code_lens(id, &php_ide::CodeLens { range: php_ide::TextRange::new(start, end), kind })?;
        let locations: Vec<lsp::Location> =
            r.targets.iter().filter_map(|t| s.location(t.file, t.focus)).collect();
        let command = if s.caps.vscode {
            if kind == php_ide::CodeLensKind::Parent {
                let loc = locations.first()?;
                lsp::Command {
                    title: r.title,
                    command: "editor.action.goToLocations".into(),
                    arguments: Some(vec![
                        serde_json::to_value(&uri).ok()?,
                        serde_json::to_value(lens.range.start).ok()?,
                        serde_json::to_value(vec![loc]).ok()?,
                        serde_json::json!("goto"),
                        serde_json::json!(""),
                    ]),
                }
            } else {
                lsp::Command {
                    title: r.title,
                    command: "editor.action.showReferences".into(),
                    arguments: Some(vec![
                        serde_json::to_value(&uri).ok()?,
                        serde_json::to_value(lens.range.start).ok()?,
                        serde_json::to_value(locations).ok()?,
                    ]),
                }
            }
        } else {
            lsp::Command { title: r.title, command: String::new(), arguments: None }
        };
        Some(command)
    })();
    lens.command = Some(resolved.unwrap_or(lsp::Command {
        title: String::new(),
        command: String::new(),
        arguments: None,
    }));
    lens
}
