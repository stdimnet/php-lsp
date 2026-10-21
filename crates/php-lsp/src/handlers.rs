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
