//! The language server main loop: documents, settings and background indexing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{select, Receiver, Sender};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{self as lsp, notification::Notification as _, request::Request as _};
use php_ide::{FileId, FileKind, IndexedFile, PositionEncoding, Workspace};
use serde_json::Value;

use crate::config::{ServerConfig, SECTIONS};
use crate::{convert, uri};

pub const SERVER_NAME: &str = "php-lsp";
const PROGRESS_TOKEN: &str = "php-lsp/indexing";

/// Result type of the server functions.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Messages from background threads to the main loop.
pub(crate) enum Task {
    Indexed { generation: u32, files: Vec<IndexedFile>, stubs: bool },
    IndexingDone { generation: u32, count: usize, millis: u128 },
}

/// What the client supports.
#[derive(Debug, Default, Clone)]
pub(crate) struct ClientCaps {
    pub config_pull: bool,
    pub work_done_progress: bool,
    pub watch_registration: bool,
    pub snippets: bool,
    pub location_links: bool,
    pub vscode: bool,
    pub markdown: bool,
}

/// A running language server instance.
pub struct Server {
    pub(crate) connection: Connection,
    pub(crate) workspace: Workspace,
    pub(crate) config: ServerConfig,
    pub(crate) encoding: PositionEncoding,
    pub(crate) caps: ClientCaps,
    /// URIs as sent by the client for open documents.
    pub(crate) client_uris: HashMap<FileId, lsp::Uri>,
    versions: HashMap<FileId, i32>,
    roots: Vec<PathBuf>,
    pub(crate) stub_dir: PathBuf,
    next_request_id: i32,
    pending: HashMap<RequestId, &'static str>,
    shutdown_requested: bool,
    task_tx: Sender<Task>,
    task_rx: Receiver<Task>,
    generation: u32,
    indexing: bool,
    cancel_indexing: Arc<std::sync::atomic::AtomicBool>,
    loaded_stubs: Vec<String>,
}

impl Server {
    /// Performs the `initialize` handshake and creates the server.
    pub fn initialize(connection: Connection) -> Result<Server> {
        let (id, params) = connection.initialize_start()?;
        let params: lsp::InitializeParams = serde_json::from_value(params)?;
        let encoding = negotiate_encoding(&params.capabilities);
        let mut config = ServerConfig::default();
        let mut storage: Option<PathBuf> = None;
        if let Some(opts) = &params.initialization_options {
            config.update(opts);
            config.update(&serde_json::json!({ "intelephense": opts }));
            if let Some(s) = opts.get("storagePath").and_then(Value::as_str) {
                storage = Some(PathBuf::from(s));
            }
        }
        let caps = client_caps(&params);
        let roots = workspace_roots(&params);
        let result = lsp::InitializeResult {
            capabilities: capabilities(encoding),
            server_info: Some(lsp::ServerInfo {
                name: SERVER_NAME.to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        };
        let mut result = serde_json::to_value(result)?;
        // not modelled by lsp-types yet
        result["capabilities"]["typeHierarchyProvider"] = Value::Bool(true);
        connection.initialize_finish(id, result)?;
        let workspace = Workspace::new(config.ide_config());
        let stub_dir = storage
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("php-lsp-stubs-{}", env!("CARGO_PKG_VERSION")));
        let (task_tx, task_rx) = crossbeam_channel::unbounded();
        Ok(Server {
            connection,
            workspace,
            config,
            encoding,
            caps,
            client_uris: HashMap::new(),
            versions: HashMap::new(),
            roots,
            stub_dir,
            next_request_id: 0,
            pending: HashMap::new(),
            shutdown_requested: false,
            task_tx,
            task_rx,
            generation: 0,
            indexing: false,
            cancel_indexing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            loaded_stubs: Vec::new(),
        })
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Runs until the client sends `exit`.
    pub fn run(mut self) -> Result<()> {
        // `initialize_finish` already consumed the `initialized` notification
        self.on_initialized();
        let receiver = self.connection.receiver.clone();
        let tasks = self.task_rx.clone();
        loop {
            select! {
                recv(receiver) -> msg => {
                    let Ok(msg) = msg else { return Ok(()) };
                    if self.on_message(msg) {
                        return Ok(());
                    }
                }
                recv(tasks) -> task => {
                    if let Ok(task) = task {
                        self.on_task(task);
                    }
                }
            }
        }
    }

    /// Handles one message; returns `true` on `exit`.
    fn on_message(&mut self, msg: Message) -> bool {
        match msg {
            Message::Request(req) => {
                if self.shutdown_requested {
                    self.respond_error(req.id, ErrorCode::InvalidRequest, "server is shutting down");
                } else if req.method == lsp::request::Shutdown::METHOD {
                    self.shutdown_requested = true;
                    self.respond(req.id, Value::Null);
                } else {
                    self.on_request(req);
                }
            }
            Message::Notification(not) => {
                if not.method == lsp::notification::Exit::METHOD {
                    return true;
                }
                self.on_notification(not);
            }
            Message::Response(resp) => self.on_response(resp),
        }
        false
    }

    // ------------------------------------------------------------------ io

    pub(crate) fn send(&self, msg: Message) {
        // The client went away if this fails; the loop ends on its own.
        let _ = self.connection.sender.send(msg);
    }

    pub(crate) fn respond(&self, id: RequestId, result: Value) {
        self.send(Message::Response(Response { id, response_result: Ok(result) }));
    }

    pub(crate) fn respond_error(&self, id: RequestId, code: ErrorCode, msg: &str) {
        self.send(Message::Response(Response::new_err(id, code as i32, msg.to_string())));
    }

    pub(crate) fn notify<N: lsp::notification::Notification>(&self, params: N::Params) {
        let params = serde_json::to_value(params).unwrap_or(Value::Null);
        self.send(Message::Notification(Notification { method: N::METHOD.to_string(), params }));
    }

    fn notify_raw(&self, method: &str, params: Value) {
        self.send(Message::Notification(Notification { method: method.to_string(), params }));
    }

    fn request<R: lsp::request::Request>(&mut self, params: R::Params) {
        self.next_request_id += 1;
        let id = RequestId::from(format!("php-lsp-{}", self.next_request_id));
        self.pending.insert(id.clone(), R::METHOD);
        let params = serde_json::to_value(params).unwrap_or(Value::Null);
        self.send(Message::Request(Request { id, method: R::METHOD.to_string(), params }));
    }

    pub(crate) fn show_message(&self, message: &str) {
        self.notify::<lsp::notification::ShowMessage>(lsp::ShowMessageParams {
            typ: lsp::MessageType::INFO,
            message: message.to_string(),
        });
    }

    /// `cancelIndexing` (Intelephense): stops the running indexing job.
    pub(crate) fn cancel_indexing(&mut self) {
        self.cancel_indexing.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn log(&self, message: &str) {
        self.notify::<lsp::notification::LogMessage>(lsp::LogMessageParams {
            typ: lsp::MessageType::LOG,
            message: message.to_string(),
        });
    }

    // ------------------------------------------------------------------ dispatch

    fn on_request(&mut self, req: Request) {
        let id = req.id.clone();
        match crate::handlers::dispatch(self, req) {
            Some(Ok(value)) => self.respond(id, value),
            Some(Err(e)) => self.respond_error(id, ErrorCode::InvalidParams, &e.to_string()),
            None => {}
        }
    }

    fn on_notification(&mut self, not: Notification) {
        use lsp::notification::*;
        let method = not.method.clone();
        let result: Result<()> = (|| {
            match not.method.as_str() {
                DidOpenTextDocument::METHOD => self.did_open(serde_json::from_value(not.params)?),
                DidChangeTextDocument::METHOD => self.did_change(serde_json::from_value(not.params)?),
                DidCloseTextDocument::METHOD => self.did_close(serde_json::from_value(not.params)?),
                DidSaveTextDocument::METHOD => {
                    let p: lsp::DidSaveTextDocumentParams = serde_json::from_value(not.params)?;
                    if self.config.diagnostics_on_save {
                        if let Some(id) = self.file_for(&p.text_document.uri) {
                            self.publish_now(id);
                        }
                    }
                }
                DidChangeWatchedFiles::METHOD => {
                    self.did_change_watched_files(serde_json::from_value(not.params)?)
                }
                DidChangeConfiguration::METHOD => {
                    let p: lsp::DidChangeConfigurationParams = serde_json::from_value(not.params)?;
                    if p.settings.is_null() || p.settings.as_object().is_some_and(|o| o.is_empty()) {
                        self.pull_config();
                    } else {
                        self.apply_config(&p.settings);
                    }
                }
                DidChangeWorkspaceFolders::METHOD => {
                    let p: lsp::DidChangeWorkspaceFoldersParams = serde_json::from_value(not.params)?;
                    for f in p.event.removed {
                        if let Some(path) = uri::path_from_uri(f.uri.as_str()) {
                            self.roots.retain(|r| r != &path);
                        }
                    }
                    for f in p.event.added {
                        if let Some(path) = uri::path_from_uri(f.uri.as_str()) {
                            self.roots.push(path);
                        }
                    }
                    self.start_indexing();
                }
                Initialized::METHOD => {}
                // Intelephense compatible commands
                "indexWorkspace" | "intelephense/index" => self.start_indexing(),
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.log(&format!("failed to handle {method}: {e}"));
        }
    }

    fn on_initialized(&mut self) {
        self.register_watchers();
        self.pull_config();
        self.start_indexing();
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

    // ------------------------------------------------------------------ config

    fn pull_config(&mut self) {
        if !self.caps.config_pull {
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
        if self.config == old {
            return;
        }
        let reindex = old.php_version != self.config.php_version
            || old.stubs != self.config.stubs
            || old.exclude != self.config.exclude
            || old.associations != self.config.associations
            || old.max_file_size != self.config.max_file_size
            || old.include_paths != self.config.include_paths;
        let mut ide = self.config.ide_config();
        if reindex {
            // keep the old stubs until re-indexing replaces them
            ide.stubs = self.workspace.config().stubs.clone();
        }
        self.workspace.set_config(ide);
        if reindex && self.generation > 0 {
            self.start_indexing();
        }
        let ids: Vec<FileId> = self.client_uris.keys().copied().collect();
        for id in ids {
            self.publish_diagnostics(id);
        }
    }

    fn register_watchers(&mut self) {
        if !self.caps.watch_registration {
            return;
        }
        let watchers = self
            .config
            .associations
            .iter()
            .map(|a| lsp::FileSystemWatcher {
                glob_pattern: lsp::GlobPattern::String(format!("**/{a}")),
                kind: None,
            })
            .collect();
        let options = lsp::DidChangeWatchedFilesRegistrationOptions { watchers };
        self.request::<lsp::request::RegisterCapability>(lsp::RegistrationParams {
            registrations: vec![lsp::Registration {
                id: "php-lsp-watch".into(),
                method: lsp::notification::DidChangeWatchedFiles::METHOD.into(),
                register_options: serde_json::to_value(options).ok(),
            }],
        });
    }

    // ------------------------------------------------------------------ indexing

    /// Re-indexes stubs and all workspace folders on a background thread.
    fn start_indexing(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        // apply the new stub list now; the stubs are re-indexed below
        self.workspace.set_config(self.config.ide_config());
        self.workspace.set_index_ready(false);
        // stubs are small: index them right away so hover / completion of
        // built-ins work from the first request
        if self.workspace.config().stubs != self.loaded_stubs || self.generation == 1 {
            self.workspace.load_stubs();
            self.loaded_stubs = self.workspace.config().stubs.clone();
        }
        let mut roots: Vec<(PathBuf, FileKind)> =
            self.roots.iter().map(|r| (r.clone(), FileKind::Workspace)).collect();
        roots.extend(self.config.include_paths.iter().map(|p| (PathBuf::from(p), FileKind::Vendor)));
        let scan = self.config.scan_config();
        let opts = self.workspace.extract_options();
        let tx = self.task_tx.clone();
        self.cancel_indexing.store(true, std::sync::atomic::Ordering::Relaxed);
        self.cancel_indexing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = self.cancel_indexing.clone();
        if !self.indexing {
            self.indexing = true;
            self.notify_raw("indexingStarted", Value::Null);
            if self.caps.work_done_progress {
                self.request::<lsp::request::WorkDoneProgressCreate>(lsp::WorkDoneProgressCreateParams {
                    token: lsp::NumberOrString::String(PROGRESS_TOKEN.into()),
                });
                self.progress(lsp::WorkDoneProgress::Begin(lsp::WorkDoneProgressBegin {
                    title: "Indexing PHP files".into(),
                    cancellable: Some(false),
                    message: None,
                    percentage: None,
                }));
            }
        }
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut count = 0;
            for (root, root_kind) in roots {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut paths = php_ide::collect_files(&root, &scan);
                if root_kind == FileKind::Vendor {
                    for p in &mut paths {
                        p.1 = FileKind::Vendor;
                    }
                }
                // read and index in chunks to keep memory bounded and the UI updated
                for chunk in paths.chunks(2000) {
                    let texts: Vec<(String, Arc<str>, FileKind)> = chunk
                        .iter()
                        .filter_map(|(p, kind)| {
                            let bytes = std::fs::read(p).ok()?;
                            let text = String::from_utf8_lossy(&bytes).into_owned();
                            Some((uri::key_from_path(p), Arc::<str>::from(text), *kind))
                        })
                        .collect();
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    count += texts.len();
                    let files = php_ide::index_files(texts, opts);
                    if tx.send(Task::Indexed { generation, files, stubs: false }).is_err() {
                        return;
                    }
                }
            }
            let _ = tx.send(Task::IndexingDone { generation, count, millis: start.elapsed().as_millis() });
        });
    }

    fn progress(&self, value: lsp::WorkDoneProgress) {
        self.notify::<lsp::notification::Progress>(lsp::ProgressParams {
            token: lsp::NumberOrString::String(PROGRESS_TOKEN.into()),
            value: lsp::ProgressParamsValue::WorkDone(value),
        });
    }

    fn on_task(&mut self, task: Task) {
        match task {
            Task::Indexed { generation, files, stubs } => {
                if generation != self.generation {
                    return;
                }
                for f in files {
                    self.workspace.add_indexed(f);
                }
                if stubs {
                    self.workspace.mark_stubs_loaded();
                }
            }
            Task::IndexingDone { generation, count, millis } => {
                if generation != self.generation {
                    return;
                }
                self.indexing = false;
                self.workspace.set_index_ready(true);
                self.log(&format!("indexed {count} files in {millis} ms"));
                let ids: Vec<FileId> = self.client_uris.keys().copied().collect();
                for id in ids {
                    self.publish_diagnostics(id);
                }
                self.notify_raw("indexingEnded", Value::Null);
                if self.caps.work_done_progress {
                    self.progress(lsp::WorkDoneProgress::End(lsp::WorkDoneProgressEnd {
                        message: Some(format!("{count} files")),
                    }));
                }
            }
        }
    }

    fn did_change_watched_files(&mut self, p: lsp::DidChangeWatchedFilesParams) {
        for change in p.changes {
            let key = uri::key_from_uri(change.uri.as_str());
            let open = self.workspace.file_id(&key).is_some_and(|id| self.client_uris.contains_key(&id));
            if open {
                continue;
            }
            match change.typ {
                lsp::FileChangeType::DELETED => {
                    if let Some(id) = self.workspace.file_id(&key) {
                        self.workspace.remove_file(id);
                    }
                }
                _ => {
                    let Some(path) = uri::path_from_uri(change.uri.as_str()) else { continue };
                    let Ok(bytes) = std::fs::read(&path) else { continue };
                    if bytes.len() as u64 > self.config.max_file_size {
                        continue;
                    }
                    let kind = if path.components().any(|c| c.as_os_str() == "vendor") {
                        FileKind::Vendor
                    } else {
                        FileKind::Workspace
                    };
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    self.workspace.index_file(&key, text.into(), kind);
                }
            }
        }
    }

    // ------------------------------------------------------------------ documents

    fn did_open(&mut self, p: lsp::DidOpenTextDocumentParams) {
        let key = uri::key_from_uri(p.text_document.uri.as_str());
        let id = self.workspace.set_file(&key, p.text_document.text);
        self.client_uris.insert(id, p.text_document.uri);
        self.versions.insert(id, p.text_document.version);
        self.publish_diagnostics(id);
    }

    fn did_change(&mut self, p: lsp::DidChangeTextDocumentParams) {
        let key = uri::key_from_uri(p.text_document.uri.as_str());
        let Some(id) = self.workspace.file_id(&key) else { return };
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
        self.workspace.set_file(&key, text);
        self.versions.insert(id, p.text_document.version);
        if !self.config.diagnostics_on_save {
            self.publish_diagnostics(id);
        }
    }

    fn did_close(&mut self, p: lsp::DidCloseTextDocumentParams) {
        let key = uri::key_from_uri(p.text_document.uri.as_str());
        let Some(id) = self.workspace.file_id(&key) else { return };
        self.client_uris.remove(&id);
        self.versions.remove(&id);
        // keep files that exist on disk indexed; forget untitled buffers
        let on_disk = uri::path_from_uri(p.text_document.uri.as_str()).is_some_and(|p| p.exists());
        if on_disk {
            self.workspace.close_file(id);
        } else {
            self.workspace.remove_file(id);
        }
        self.notify::<lsp::notification::PublishDiagnostics>(lsp::PublishDiagnosticsParams {
            uri: p.text_document.uri,
            diagnostics: Vec::new(),
            version: None,
        });
    }

    pub(crate) fn publish_diagnostics(&mut self, id: FileId) {
        self.publish_now(id);
    }

    fn publish_now(&mut self, id: FileId) {
        let (Some(uri), Some(index), Some(file)) =
            (self.client_uris.get(&id).cloned(), self.workspace.line_index(id), self.workspace.file(id))
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

    pub(crate) fn file_for(&self, uri: &lsp::Uri) -> Option<FileId> {
        self.workspace.file_id(&uri::key_from_uri(uri.as_str()))
    }

    /// URI for a file, materializing embedded stubs on disk so that clients
    /// can open them.
    pub(crate) fn uri_for(&self, id: FileId) -> Option<lsp::Uri> {
        if let Some(u) = self.client_uris.get(&id) {
            return Some(u.clone());
        }
        let file = self.workspace.file(id)?;
        let key: &str = &file.path;
        let s = match key.strip_prefix(php_ide::STUB_SCHEME) {
            Some(rel) => {
                let path = self.stub_dir.join(rel);
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&path, file.text.as_bytes()).ok()?;
                }
                uri::uri_from_key(&uri::key_from_path(&path))
            }
            None => uri::uri_from_key(key),
        };
        s.parse().ok()
    }

    /// Location of a range in a file.
    pub(crate) fn location(&self, id: FileId, range: php_ide::TextRange) -> Option<lsp::Location> {
        let index = self.workspace.line_index(id)?;
        Some(lsp::Location { uri: self.uri_for(id)?, range: convert::range(&index, self.encoding, range) })
    }
}

fn client_caps(params: &lsp::InitializeParams) -> ClientCaps {
    let caps = &params.capabilities;
    let td = caps.text_document.as_ref();
    ClientCaps {
        config_pull: caps.workspace.as_ref().and_then(|w| w.configuration).unwrap_or(false),
        work_done_progress: caps.window.as_ref().and_then(|w| w.work_done_progress).unwrap_or(false),
        watch_registration: caps
            .workspace
            .as_ref()
            .and_then(|w| w.did_change_watched_files.as_ref())
            .and_then(|d| d.dynamic_registration)
            .unwrap_or(false),
        snippets: td
            .and_then(|t| t.completion.as_ref())
            .and_then(|c| c.completion_item.as_ref())
            .and_then(|i| i.snippet_support)
            .unwrap_or(false),
        location_links: td.and_then(|t| t.definition.as_ref()).and_then(|d| d.link_support).unwrap_or(false),
        vscode: params
            .client_info
            .as_ref()
            .is_some_and(|c| c.name.contains("Visual Studio Code") || c.name.contains("VSCodium")),
        markdown: td
            .and_then(|t| t.hover.as_ref())
            .and_then(|h| h.content_format.as_ref())
            .is_none_or(|f| f.contains(&lsp::MarkupKind::Markdown)),
    }
}

#[allow(deprecated)]
fn workspace_roots(params: &lsp::InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        let roots: Vec<PathBuf> = folders.iter().filter_map(|f| uri::path_from_uri(f.uri.as_str())).collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    if let Some(root) = &params.root_uri {
        if let Some(p) = uri::path_from_uri(root.as_str()) {
            return vec![p];
        }
    }
    params.root_path.as_ref().map(|p| vec![PathBuf::from(p)]).unwrap_or_default()
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
            save: Some(lsp::TextDocumentSyncSaveOptions::Supported(true)),
        })),
        document_symbol_provider: Some(lsp::OneOf::Left(true)),
        folding_range_provider: Some(lsp::FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(lsp::SelectionRangeProviderCapability::Simple(true)),
        hover_provider: Some(lsp::HoverProviderCapability::Simple(true)),
        definition_provider: Some(lsp::OneOf::Left(true)),
        declaration_provider: Some(lsp::DeclarationCapability::Simple(true)),
        type_definition_provider: Some(lsp::TypeDefinitionProviderCapability::Simple(true)),
        implementation_provider: Some(lsp::ImplementationProviderCapability::Simple(true)),
        references_provider: Some(lsp::OneOf::Left(true)),
        document_highlight_provider: Some(lsp::OneOf::Left(true)),
        workspace_symbol_provider: Some(lsp::OneOf::Left(true)),
        inlay_hint_provider: Some(lsp::OneOf::Left(true)),
        rename_provider: Some(lsp::OneOf::Right(lsp::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        code_action_provider: Some(lsp::CodeActionProviderCapability::Options(lsp::CodeActionOptions {
            code_action_kinds: Some(vec![
                lsp::CodeActionKind::QUICKFIX,
                lsp::CodeActionKind::REFACTOR,
                lsp::CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
            ]),
            work_done_progress_options: Default::default(),
            resolve_provider: Some(false),
        })),
        call_hierarchy_provider: Some(lsp::CallHierarchyServerCapability::Simple(true)),
        document_formatting_provider: Some(lsp::OneOf::Left(true)),
        document_range_formatting_provider: Some(lsp::OneOf::Left(true)),
        code_lens_provider: Some(lsp::CodeLensOptions { resolve_provider: Some(true) }),
        semantic_tokens_provider: Some(lsp::SemanticTokensServerCapabilities::SemanticTokensOptions(
            lsp::SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: crate::handlers::semantic_tokens_legend(),
                range: Some(true),
                full: Some(lsp::SemanticTokensFullOptions::Bool(true)),
            },
        )),
        completion_provider: Some(lsp::CompletionOptions {
            trigger_characters: Some(
                ["$", ">", ":", "\\", "@", "#", "["].iter().map(|s| s.to_string()).collect(),
            ),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        signature_help_provider: Some(lsp::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        workspace: Some(lsp::WorkspaceServerCapabilities {
            workspace_folders: Some(lsp::WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(lsp::OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
    }
}
