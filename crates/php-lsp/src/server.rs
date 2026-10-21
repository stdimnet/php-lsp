//! The language server main loop.

use std::collections::HashMap;

use crossbeam_channel::Receiver;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{self as lsp, notification::Notification as _, request::Request as _};
use php_ide::{FileId, PositionEncoding, Workspace};
use serde_json::Value;

use crate::config::{ServerConfig, SECTIONS};
use crate::convert;

pub const SERVER_NAME: &str = "php-lsp";

/// A running language server instance.
pub struct Server {
    connection: Connection,
    workspace: Workspace,
    config: ServerConfig,
    encoding: PositionEncoding,
    uris: HashMap<FileId, lsp::Uri>,
    versions: HashMap<FileId, i32>,
    supports_config_pull: bool,
    next_request_id: i32,
    pending: HashMap<RequestId, &'static str>,
    shutdown_requested: bool,
}

/// Result of the main loop.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

impl Server {
    /// Performs the `initialize` handshake and creates the server.
    pub fn initialize(connection: Connection) -> Result<Server> {
        let (id, params) = connection.initialize_start()?;
        let params: lsp::InitializeParams = serde_json::from_value(params)?;
        let encoding = negotiate_encoding(&params.capabilities);
        let mut config = ServerConfig::default();
        if let Some(opts) = &params.initialization_options {
            config.update(opts);
        }
        let supports_config_pull =
            params.capabilities.workspace.as_ref().and_then(|w| w.configuration).unwrap_or(false);
        let result = lsp::InitializeResult {
            capabilities: capabilities(encoding),
            server_info: Some(lsp::ServerInfo {
                name: SERVER_NAME.to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        };
        connection.initialize_finish(id, serde_json::to_value(result)?)?;
        let workspace = Workspace::new(config.ide_config());
        Ok(Server {
            connection,
            workspace,
            config,
            encoding,
            uris: HashMap::new(),
            versions: HashMap::new(),
            supports_config_pull,
            next_request_id: 0,
            pending: HashMap::new(),
            shutdown_requested: false,
        })
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Runs until the client sends `exit`.
    pub fn run(mut self) -> Result<()> {
        let receiver: Receiver<Message> = self.connection.receiver.clone();
        for msg in &receiver {
            match msg {
                Message::Request(req) => {
                    if self.shutdown_requested {
                        self.respond_error(req.id, ErrorCode::InvalidRequest, "server is shutting down");
                        continue;
                    }
                    if req.method == lsp::request::Shutdown::METHOD {
                        self.shutdown_requested = true;
                        self.respond(req.id, Value::Null);
                        continue;
                    }
                    self.on_request(req);
                }
                Message::Notification(not) => {
                    if not.method == lsp::notification::Exit::METHOD {
                        return Ok(());
                    }
                    self.on_notification(not);
                }
                Message::Response(resp) => self.on_response(resp),
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ io

    fn send(&self, msg: Message) {
        // The client went away if this fails; the loop ends on its own.
        let _ = self.connection.sender.send(msg);
    }

    fn respond(&self, id: RequestId, result: Value) {
        self.send(Message::Response(Response { id, response_result: Ok(result) }));
    }

    fn respond_error(&self, id: RequestId, code: ErrorCode, msg: &str) {
        self.send(Message::Response(Response::new_err(id, code as i32, msg.to_string())));
    }

    fn notify<N: lsp::notification::Notification>(&self, params: N::Params) {
        let params = serde_json::to_value(params).unwrap_or(Value::Null);
        self.send(Message::Notification(Notification { method: N::METHOD.to_string(), params }));
    }

    fn request<R: lsp::request::Request>(&mut self, params: R::Params) {
        self.next_request_id += 1;
        let id = RequestId::from(self.next_request_id);
        self.pending.insert(id.clone(), R::METHOD);
        let params = serde_json::to_value(params).unwrap_or(Value::Null);
        self.send(Message::Request(Request { id, method: R::METHOD.to_string(), params }));
    }

    // ------------------------------------------------------------------ dispatch

    fn on_request(&mut self, req: Request) {
        let id = req.id.clone();
        let result = match req.method.as_str() {
            lsp::request::DocumentSymbolRequest::METHOD => self.handle(req, Self::document_symbols),
            lsp::request::FoldingRangeRequest::METHOD => self.handle(req, Self::folding_ranges),
            lsp::request::SelectionRangeRequest::METHOD => self.handle(req, Self::selection_ranges),
            crate::ext::SyntaxTree::METHOD => self.handle(req, Self::syntax_tree),
            _ => {
                self.respond_error(
                    id,
                    ErrorCode::MethodNotFound,
                    &format!("unhandled method {}", req.method),
                );
                return;
            }
        };
        match result {
            Ok(value) => self.respond(id, value),
            Err(e) => self.respond_error(id, ErrorCode::InvalidParams, &e.to_string()),
        }
    }

    fn handle<P, R>(&mut self, req: Request, f: fn(&mut Self, P) -> R) -> Result<Value>
    where
        P: serde::de::DeserializeOwned,
        R: serde::Serialize,
    {
        let params: P = serde_json::from_value(req.params)?;
        Ok(serde_json::to_value(f(self, params))?)
    }

    fn on_notification(&mut self, not: Notification) {
        use lsp::notification::*;
        let result: Result<()> = (|| {
            match not.method.as_str() {
                DidOpenTextDocument::METHOD => {
                    let p: lsp::DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
                    self.did_open(p);
                }
                DidChangeTextDocument::METHOD => {
                    let p: lsp::DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
                    self.did_change(p);
                }
                DidCloseTextDocument::METHOD => {
                    let p: lsp::DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
                    self.did_close(p);
                }
                DidChangeConfiguration::METHOD => {
                    let p: lsp::DidChangeConfigurationParams = serde_json::from_value(not.params)?;
                    if p.settings.is_null() || p.settings.as_object().is_some_and(|o| o.is_empty()) {
                        self.pull_config();
                    } else {
                        self.apply_config(&p.settings);
                    }
                }
                Initialized::METHOD => self.pull_config(),
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.log(&format!("failed to handle {}: {e}", not.method));
        }
    }

    fn on_response(&mut self, resp: Response) {
        let Some(method) = self.pending.remove(&resp.id) else { return };
        if method == lsp::request::WorkspaceConfiguration::METHOD {
            if let Ok(Value::Array(items)) = resp.response_result {
                let mut merged = serde_json::Map::new();
                for (section, value) in SECTIONS.iter().zip(items) {
                    if !value.is_null() {
                        merged.insert(section.to_string(), value);
                    }
                }
                self.apply_config(&Value::Object(merged));
            }
        }
    }

    fn log(&self, message: &str) {
        self.notify::<lsp::notification::LogMessage>(lsp::LogMessageParams {
            typ: lsp::MessageType::LOG,
            message: message.to_string(),
        });
    }

    // ------------------------------------------------------------------ config

    fn pull_config(&mut self) {
        if !self.supports_config_pull {
            return;
        }
        let items = SECTIONS
            .iter()
            .map(|s| lsp::ConfigurationItem { scope_uri: None, section: Some(s.to_string()) })
            .collect();
        self.request::<lsp::request::WorkspaceConfiguration>(lsp::ConfigurationParams { items });
    }

    fn apply_config(&mut self, settings: &Value) {
        let old = self.config.clone();
        self.config.update(settings);
        if self.config != old {
            self.workspace.set_config(self.config.ide_config());
            let ids: Vec<FileId> = self.uris.keys().copied().collect();
            for id in ids {
                self.publish_diagnostics(id);
            }
        }
    }

    // ------------------------------------------------------------------ documents

    fn did_open(&mut self, p: lsp::DidOpenTextDocumentParams) {
        let id = self.workspace.set_file(p.text_document.uri.as_str(), p.text_document.text);
        self.uris.insert(id, p.text_document.uri);
        self.versions.insert(id, p.text_document.version);
        self.publish_diagnostics(id);
    }

    fn did_change(&mut self, p: lsp::DidChangeTextDocumentParams) {
        let Some(id) = self.workspace.file_id(p.text_document.uri.as_str()) else { return };
        let Some(file) = self.workspace.file(id) else { return };
        let mut text = file.text.to_string();
        for change in p.content_changes {
            match change.range {
                Some(range) => {
                    // Positions refer to the document after the previous change.
                    let index = php_ide::LineIndex::new(&text);
                    let r = convert::text_range(&index, self.encoding, range);
                    text.replace_range(usize::from(r.start())..usize::from(r.end()), &change.text);
                }
                None => text = change.text,
            }
        }
        self.workspace.set_file(p.text_document.uri.as_str(), text);
        self.versions.insert(id, p.text_document.version);
        self.publish_diagnostics(id);
    }

    fn did_close(&mut self, p: lsp::DidCloseTextDocumentParams) {
        if let Some(id) = self.workspace.file_id(p.text_document.uri.as_str()) {
            self.workspace.remove_file(id);
            self.uris.remove(&id);
            self.versions.remove(&id);
            self.notify::<lsp::notification::PublishDiagnostics>(lsp::PublishDiagnosticsParams {
                uri: p.text_document.uri,
                diagnostics: Vec::new(),
                version: None,
            });
        }
    }

    fn publish_diagnostics(&mut self, id: FileId) {
        let (Some(uri), Some(index), Some(file)) =
            (self.uris.get(&id).cloned(), self.workspace.line_index(id), self.workspace.file(id))
        else {
            return;
        };
        let diagnostics = if file.text.len() as u64 > self.config.max_file_size {
            Vec::new()
        } else {
            self.workspace
                .diagnostics(id)
                .into_iter()
                .map(|d| {
                    let mut tags = Vec::new();
                    if d.unnecessary {
                        tags.push(lsp::DiagnosticTag::UNNECESSARY);
                    }
                    if d.deprecated {
                        tags.push(lsp::DiagnosticTag::DEPRECATED);
                    }
                    lsp::Diagnostic {
                        range: convert::range(&index, self.encoding, d.range),
                        severity: Some(convert::severity(d.severity)),
                        code: Some(lsp::NumberOrString::String(d.code.as_str().to_string())),
                        code_description: None,
                        source: Some(SERVER_NAME.to_string()),
                        message: d.message,
                        related_information: None,
                        tags: if tags.is_empty() { None } else { Some(tags) },
                        data: None,
                    }
                })
                .collect()
        };
        let version = self.versions.get(&id).copied();
        self.notify::<lsp::notification::PublishDiagnostics>(lsp::PublishDiagnosticsParams {
            uri,
            diagnostics,
            version,
        });
    }

    fn file_for(&self, uri: &lsp::Uri) -> Option<FileId> {
        self.workspace.file_id(uri.as_str())
    }

    // ------------------------------------------------------------------ features

    fn document_symbols(&mut self, p: lsp::DocumentSymbolParams) -> Option<lsp::DocumentSymbolResponse> {
        let id = self.file_for(&p.text_document.uri)?;
        let index = self.workspace.line_index(id)?;
        let symbols = self.workspace.document_symbols(id);
        Some(lsp::DocumentSymbolResponse::Nested(
            symbols.into_iter().map(|s| convert::document_symbol(&index, self.encoding, s)).collect(),
        ))
    }

    fn folding_ranges(&mut self, p: lsp::FoldingRangeParams) -> Option<Vec<lsp::FoldingRange>> {
        let id = self.file_for(&p.text_document.uri)?;
        let index = self.workspace.line_index(id)?;
        let text = self.workspace.file(id)?.text.clone();
        Some(
            self.workspace
                .folding_ranges(id)
                .into_iter()
                .filter_map(|f| convert::folding_range(&index, &text, f))
                .collect(),
        )
    }

    fn selection_ranges(&mut self, p: lsp::SelectionRangeParams) -> Option<Vec<lsp::SelectionRange>> {
        let id = self.file_for(&p.text_document.uri)?;
        let index = self.workspace.line_index(id)?;
        let offsets: Vec<_> =
            p.positions.iter().map(|&pos| convert::offset(&index, self.encoding, pos)).collect();
        let result = self
            .workspace
            .selection_ranges(id, &offsets)
            .into_iter()
            .map(|ranges| {
                let mut parent: Option<Box<lsp::SelectionRange>> = None;
                for r in ranges.into_iter().rev() {
                    parent = Some(Box::new(lsp::SelectionRange {
                        range: convert::range(&index, self.encoding, r),
                        parent,
                    }));
                }
                parent
                    .map(|b| *b)
                    .unwrap_or(lsp::SelectionRange { range: lsp::Range::default(), parent: None })
            })
            .collect();
        Some(result)
    }

    fn syntax_tree(&mut self, p: crate::ext::SyntaxTreeParams) -> Option<String> {
        let id = self.file_for(&p.text_document.uri)?;
        self.workspace.syntax_tree(id)
    }
}

fn negotiate_encoding(caps: &lsp::ClientCapabilities) -> PositionEncoding {
    let offered = caps.general.as_ref().and_then(|g| g.position_encodings.as_ref());
    if let Some(encodings) = offered {
        if encodings.contains(&lsp::PositionEncodingKind::UTF8) {
            return PositionEncoding::Utf8;
        }
        if encodings.contains(&lsp::PositionEncodingKind::UTF32)
            && !encodings.contains(&lsp::PositionEncodingKind::UTF16)
        {
            return PositionEncoding::Utf32;
        }
    }
    PositionEncoding::Utf16
}

fn capabilities(encoding: PositionEncoding) -> lsp::ServerCapabilities {
    lsp::ServerCapabilities {
        position_encoding: Some(match encoding {
            PositionEncoding::Utf8 => lsp::PositionEncodingKind::UTF8,
            PositionEncoding::Utf16 => lsp::PositionEncodingKind::UTF16,
            PositionEncoding::Utf32 => lsp::PositionEncodingKind::UTF32,
        }),
        text_document_sync: Some(lsp::TextDocumentSyncCapability::Options(lsp::TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(lsp::TextDocumentSyncKind::INCREMENTAL),
            will_save: None,
            will_save_wait_until: None,
            save: None,
        })),
        document_symbol_provider: Some(lsp::OneOf::Left(true)),
        folding_range_provider: Some(lsp::FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(lsp::SelectionRangeProviderCapability::Simple(true)),
        ..Default::default()
    }
}
