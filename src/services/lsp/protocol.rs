//! What the messages say. This is the one place that uses the protocol's types: they are read out of
//! what the server sends and written into what the client sends, and what is given to the rest of the
//! module is made of types of its own, so that nothing else depends on them.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::position::{Encoding, Position};

mod code_action;
mod formatting;
mod inlay;
mod lens;
mod rename;
mod semantic;
mod signature;
mod symbols;
pub use self::code_action::{
    Action as ReportedAction, CODE_ACTION, action_params, actions as parse_actions,
};
pub use self::formatting::{FORMATTING, formatting_edits, formatting_params};
pub use self::inlay::{
    HintKind, INLAY_HINT, Reported as ReportedHint, hint_params, hints as parse_hints,
};
pub use self::lens::{
    CODE_LENS, CODE_LENS_RESOLVE, LensKind, Unresolved, lens_params, resolved as resolved_lens,
    unresolved as unresolved_lenses,
};
pub use self::rename::{
    REFERENCES, RENAME, WorkspaceEdit, references_params, rename_params, workspace_edit,
};
pub use self::semantic::{
    Legend, Raw as RawToken, SEMANTIC_TOKENS_FULL, SEMANTIC_TOKENS_REFRESH,
    legend as semantic_legend, token_params as semantic_params, tokens as parse_tokens,
};
pub use self::signature::{SIGNATURE_HELP, SignatureHelp, signature_help, signature_params};
pub use self::symbols::{
    Symbol, SymbolKind, WORKSPACE_SYMBOL, symbol_params, symbols as parse_symbols,
};

/// How the server wants the text it is told about kept up to date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    /// It is not told of changes.
    None,
    /// It is given the whole text on every change.
    Full,
    /// It is given the part that changed.
    Incremental,
}

/// What the server said it can do and how it wants to be spoken to, from what it answered to `initialize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub sync: SyncKind,
    /// Whether the server completes.
    pub completion: bool,
    /// Whether the server finds where a name is defined.
    pub definition: bool,
    /// Whether the server says what a name is, with the documentation that goes with it.
    pub hover: bool,
    /// Whether the server has notes for the text, such as the type of a name, which are drawn in the line.
    pub inlay_hints: bool,
    /// Whether the server says what parameters a call takes while the caret is inside its arguments.
    pub signature_help: bool,
    /// Whether the server is asked for the diagnostics of a file, and does not publish what it finds in the text of its own accord.
    pub pull_diagnostics: bool,
    /// What a character of a position is, which the server chose from what the client offered.
    pub encoding: Encoding,
}

/// How bad a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// A diagnostic as the server said it, with its range as positions, which the text it is about is needed to
/// turn into bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    pub range: (Position, Position),
    pub severity: Severity,
    pub message: String,
    pub code: Option<String>,
    /// Whether this diagnostic marks unused or unnecessary code (grayed out instead of underlined).
    pub unused: bool,
}

/// The diagnostics the server published for one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub path: PathBuf,
    /// Which version of the file they are about, when the server says.
    pub version: Option<i32>,
    pub diagnostics: Vec<Reported>,
}

/// A change of a text, as the server is told of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The whole text is this.
    Full(String),
    /// The text in the range is replaced by this.
    Incremental {
        range: (Position, Position),
        text: String,
    },
}

pub const INITIALIZE: &str = "initialize";
pub const INITIALIZED: &str = "initialized";
pub const SHUTDOWN: &str = "shutdown";
pub const EXIT: &str = "exit";
pub const DID_OPEN: &str = "textDocument/didOpen";
pub const DID_CHANGE: &str = "textDocument/didChange";
pub const DID_CLOSE: &str = "textDocument/didClose";
pub const DID_SAVE: &str = "textDocument/didSave";
pub const COMPLETION: &str = "textDocument/completion";
pub const COMPLETION_RESOLVE: &str = "completionItem/resolve";
pub const DEFINITION: &str = "textDocument/definition";
pub const HOVER: &str = "textDocument/hover";
pub const CANCEL_REQUEST: &str = "$/cancelRequest";
pub const PUBLISH_DIAGNOSTICS: &str = "textDocument/publishDiagnostics";
/// The request for what the server finds wrong in a file, which rust-analyzer answers for what it finds itself: the code that is
/// switched off by a `cfg` is one of what it says.
pub const DIAGNOSTIC: &str = "textDocument/diagnostic";
/// What the server asks when what it said of the files is not as it was, and they are to be asked for again.
pub const DIAGNOSTIC_REFRESH: &str = "workspace/diagnostic/refresh";
/// Tells the server that its settings changed, so that it asks for them again.
pub const DID_CHANGE_CONFIGURATION: &str = "workspace/didChangeConfiguration";
pub const PROGRESS: &str = "$/progress";
/// rust-analyzer's way of saying that it is busy or not, which it does for a client that asks.
pub const SERVER_STATUS: &str = "experimental/serverStatus";

/// How the server says it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Ok,
    Warning,
    Error,
}

/// What the server said of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    /// Whether it has nothing left to do: false while it loads a project or works through it.
    pub quiescent: bool,
    pub health: Health,
    pub message: Option<String>,
}

/// One step of a job that the server is doing, and which job it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Which job: the server gives each a token, which is a number or text, and here is text.
    pub token: String,
    pub update: ProgressUpdate,
}

/// What a step says of its job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressUpdate {
    /// The job begins, and what it is.
    Begin {
        title: String,
        message: Option<String>,
        percentage: Option<u32>,
    },
    /// The job goes on. What it does not say stays as it was.
    Report {
        message: Option<String>,
        percentage: Option<u32>,
    },
    /// The job is over.
    End,
}

/// The address of the file at `path`, as the protocol writes one: `file://` and the path, with every
/// byte that is not plain written as `%` and its two digits.
pub fn uri_of(path: &Path) -> String {
    let path = path.to_string_lossy();
    let mut uri = String::with_capacity("file://".len() + path.len());
    uri.push_str("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The path an address names, if it is one of a file on this machine.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // The part before the path is the machine: none, or this one.
    let path = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/')?;
        if &rest[..slash] != "localhost" {
            return None;
        }
        &rest[slash..]
    };
    let mut bytes = Vec::with_capacity(path.len());
    let mut remaining = path.bytes();
    while let Some(byte) = remaining.next() {
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let (high, low) = (remaining.next()?, remaining.next()?);
        let digit = |digit: u8| char::from(digit).to_digit(16);
        bytes.push((digit(high)? * 16 + digit(low)?) as u8);
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// The address of the file at `path`, as the protocol's own type holds it.
fn uri(path: &Path) -> lsp_types::Uri {
    std::str::FromStr::from_str(&uri_of(path))
        .expect("a file address that is written in percent is an address")
}

fn position(position: Position) -> lsp_types::Position {
    lsp_types::Position {
        line: position.line,
        character: position.character,
    }
}

fn from_position(position: lsp_types::Position) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
}

/// The settings the server is started with and given when asked: those of the lenses, those of
/// the symbols, check-on-save, and the cargo target triple when overridden.
pub fn server_options(target: Option<&str>) -> Value {
    let mut options = lens::server_options();
    if let (Some(options), Value::Object(symbols)) =
        (options.as_object_mut(), symbols::server_options())
    {
        options.extend(symbols);
    }
    // What is on disk is checked when a file is saved, with `cargo check`: the errors the compiler finds are the server's to publish.
    if let Some(options) = options.as_object_mut() {
        options.insert("checkOnSave".to_owned(), json!(true));
        options.insert("check".to_owned(), json!({"command": "check"}));
        if let Some(target) = target {
            options.insert("cargo".to_owned(), json!({"target": target}));
        }
    }
    options
}

/// The parameters of `workspace/didChangeConfiguration`.
pub fn did_change_configuration_params(target: Option<&str>) -> Value {
    json!({"settings": server_options(target)})
}

/// The parameters of `initialize` for a client that has `root` open.
pub fn initialize_params(root: &Path, process_id: u32) -> Value {
    initialize_params_with_target(root, process_id, None)
}

/// The parameters of `initialize` for a client that has `root` open and an optional cargo `target`.
#[allow(deprecated)] // `root_uri` is deprecated for the folders, and servers that are older use it.
pub fn initialize_params_with_target(root: &Path, process_id: u32, target: Option<&str>) -> Value {
    use lsp_types::{
        ClientCapabilities, ClientInfo, GeneralClientCapabilities, InitializeParams,
        PositionEncodingKind, PublishDiagnosticsClientCapabilities, TextDocumentClientCapabilities,
        TextDocumentSyncClientCapabilities, WorkspaceFolder,
    };
    let root_uri = uri(root);
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let params = InitializeParams {
        process_id: Some(process_id),
        root_uri: Some(root_uri.clone()),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri: root_uri,
            name,
        }]),
        capabilities: ClientCapabilities {
            // The client says it watches the files, and does not: a server that is told so does not watch
            // them itself, and its own watching is what costs the most of its loading.
            workspace: Some(lsp_types::WorkspaceClientCapabilities {
                configuration: Some(true),
                workspace_edit: serde_json::from_value(code_action::workspace_edit_capability())
                    .expect("what the client says of edits is what the protocol has"),
                semantic_tokens: serde_json::from_value(semantic::workspace_capability())
                    .expect("what the client says of tokens is what the protocol has"),
                diagnostic: Some(lsp_types::DiagnosticWorkspaceClientCapabilities {
                    refresh_support: Some(true),
                }),
                did_change_watched_files: Some(
                    lsp_types::DidChangeWatchedFilesClientCapabilities {
                        dynamic_registration: Some(true),
                        relative_pattern_support: Some(true),
                    },
                ),
                ..Default::default()
            }),
            // rust-analyzer says when it is busy and when it is not, which not every job it does is announced as.
            experimental: Some(serde_json::json!({
                "serverStatusNotification": true,
                // The lists of places that the lenses count are shown, so the server is to make them.
                "commands": lens::client_commands(),
            })),
            // Jobs of the server, such as loading a project, are shown as they go.
            window: Some(lsp_types::WindowClientCapabilities {
                work_done_progress: Some(true),
                ..Default::default()
            }),
            general: Some(GeneralClientCapabilities {
                // UTF-8 first, so a server that can count bytes does, and UTF-16, which every server can.
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF8,
                    PositionEncodingKind::UTF16,
                ]),
                ..Default::default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    dynamic_registration: Some(false),
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                    did_save: Some(true),
                }),
                // The diagnostics of a file are asked for, and the server says when to ask again.
                diagnostic: Some(lsp_types::DiagnosticClientCapabilities {
                    dynamic_registration: Some(false),
                    related_document_support: Some(false),
                }),
                publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                    related_information: Some(false),
                    version_support: Some(true),
                    tag_support: Some(lsp_types::TagSupport {
                        value_set: vec![
                            lsp_types::DiagnosticTag::UNNECESSARY,
                            lsp_types::DiagnosticTag::DEPRECATED,
                        ],
                    }),
                    ..Default::default()
                }),
                // Places, and not links: where the name is, and nothing of the rest of what it names.
                definition: Some(lsp_types::GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                // What a name is and what is written of it, as markdown, which the editor draws as it draws a doc comment.
                hover: Some(lsp_types::HoverClientCapabilities {
                    dynamic_registration: Some(false),
                    content_format: Some(vec![
                        lsp_types::MarkupKind::Markdown,
                        lsp_types::MarkupKind::PlainText,
                    ]),
                }),
                inlay_hint: serde_json::from_value(inlay::client_capability())
                    .expect("what the client says of hints is what the protocol has"),
                code_action: serde_json::from_value(code_action::client_capability())
                    .expect("what the client says of actions is what the protocol has"),
                formatting: serde_json::from_value(formatting::client_capability())
                    .expect("what the client says of formatting is what the protocol has"),
                semantic_tokens: serde_json::from_value(semantic::client_capability())
                    .expect("what the client says of tokens is what the protocol has"),
                signature_help: serde_json::from_value(signature::client_capability())
                    .expect("what the client says of signature help is what the protocol has"),
                completion: Some(lsp_types::CompletionClientCapabilities {
                    completion_item: Some(lsp_types::CompletionItemCapability {
                        snippet_support: Some(false),
                        insert_replace_support: Some(false),
                        // The path that an item is imported from is not part of its name: it is said beside it.
                        label_details_support: Some(true),
                        // The edits of an item, such as the line that imports it, are asked for when the item is
                        // chosen: rust-analyzer offers the items that are not in scope only to a client that says so.
                        resolve_support: Some(lsp_types::CompletionItemCapabilityResolveSupport {
                            properties: vec!["additionalTextEdits".to_owned()],
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        initialization_options: Some(server_options(target)),
        client_info: Some(ClientInfo {
            name: "Refractor".to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
        ..Default::default()
    };
    serde_json::to_value(params).expect("the parameters of initialize can be written")
}

/// What the server answered to `initialize`.
pub fn initialize_result(result: &Value) -> Result<Capabilities, String> {
    use lsp_types::{
        InitializeResult, PositionEncodingKind, TextDocumentSyncCapability, TextDocumentSyncKind,
    };
    let result: InitializeResult =
        serde_json::from_value(result.clone()).map_err(|error| error.to_string())?;
    let kind = match result.capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(kind)) => Some(kind),
        Some(TextDocumentSyncCapability::Options(options)) => options.change,
        None => None,
    };
    let sync = match kind {
        Some(kind) if kind == TextDocumentSyncKind::INCREMENTAL => SyncKind::Incremental,
        Some(kind) if kind == TextDocumentSyncKind::FULL => SyncKind::Full,
        _ => SyncKind::None,
    };
    let encoding = match result.capabilities.position_encoding {
        None => Encoding::Utf16,
        Some(kind) if kind == PositionEncodingKind::UTF16 => Encoding::Utf16,
        Some(kind) if kind == PositionEncodingKind::UTF8 => Encoding::Utf8,
        Some(kind) => {
            return Err(format!(
                "the server chose {} positions, which were not offered",
                kind.as_str()
            ));
        }
    };
    let definition = match result.capabilities.definition_provider {
        None | Some(lsp_types::OneOf::Left(false)) => false,
        Some(_) => true,
    };
    let hover = match result.capabilities.hover_provider {
        None | Some(lsp_types::HoverProviderCapability::Simple(false)) => false,
        Some(_) => true,
    };
    Ok(Capabilities {
        sync,
        completion: result
            .capabilities
            .completion_provider
            .is_some(),
        definition,
        hover,
        inlay_hints: match result.capabilities.inlay_hint_provider {
            None | Some(lsp_types::OneOf::Left(false)) => false,
            Some(_) => true,
        },
        signature_help: result.capabilities.signature_help_provider.is_some(),
        pull_diagnostics: result.capabilities.diagnostic_provider.is_some(),
        encoding,
    })
}

/// The parameters of `textDocument/didOpen`.
pub fn did_open_params(path: &Path, version: i32, text: &str) -> Value {
    let params = lsp_types::DidOpenTextDocumentParams {
        text_document: lsp_types::TextDocumentItem {
            uri: uri(path),
            language_id: "rust".to_owned(),
            version,
            text: text.to_owned(),
        },
    };
    serde_json::to_value(params).expect("the parameters of didOpen can be written")
}

/// The parameters of `textDocument/didChange` for one change.
pub fn did_change_params(path: &Path, version: i32, change: &Change) -> Value {
    let event = match change {
        Change::Full(text) => lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.clone(),
        },
        Change::Incremental { range, text } => lsp_types::TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: position(range.0),
                end: position(range.1),
            }),
            range_length: None,
            text: text.clone(),
        },
    };
    let params = lsp_types::DidChangeTextDocumentParams {
        text_document: lsp_types::VersionedTextDocumentIdentifier {
            uri: uri(path),
            version,
        },
        content_changes: vec![event],
    };
    serde_json::to_value(params).expect("the parameters of didChange can be written")
}

/// The parameters of `textDocument/didClose`.
pub fn did_close_params(path: &Path) -> Value {
    let params = lsp_types::DidCloseTextDocumentParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri(path) },
    };
    serde_json::to_value(params).expect("the parameters of didClose can be written")
}

/// The parameters of `textDocument/didSave`. The text is not in them: the server reads the file.
pub fn did_save_params(path: &Path) -> Value {
    let params = lsp_types::DidSaveTextDocumentParams {
        text_document: lsp_types::TextDocumentIdentifier { uri: uri(path) },
        text: None,
    };
    serde_json::to_value(params).expect("the parameters of didSave can be written")
}

/// The parameters of `experimental/serverStatus`.
pub fn server_status(params: &Value) -> Result<ServerStatus, String> {
    let health = match params.get("health").and_then(Value::as_str) {
        Some("ok") => Health::Ok,
        Some("warning") => Health::Warning,
        Some("error") => Health::Error,
        other => return Err(format!("a health that is not one: {other:?}")),
    };
    let quiescent = params
        .get("quiescent")
        .and_then(Value::as_bool)
        .ok_or("no word on whether the server is idle")?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .filter(|message| !message.is_empty())
        .map(str::to_owned);
    Ok(ServerStatus {
        quiescent,
        health,
        message,
    })
}

/// The parameters of `$/progress`: a step of a job of the server's.
pub fn progress(params: &Value) -> Result<Progress, String> {
    use lsp_types::{NumberOrString, ProgressParams, ProgressParamsValue, WorkDoneProgress};
    let params: ProgressParams =
        serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
    let token = match params.token {
        NumberOrString::Number(number) => number.to_string(),
        NumberOrString::String(text) => text,
    };
    let ProgressParamsValue::WorkDone(step) = params.value;
    let update = match step {
        WorkDoneProgress::Begin(begin) => ProgressUpdate::Begin {
            title: begin.title,
            message: begin.message,
            percentage: begin.percentage,
        },
        WorkDoneProgress::Report(report) => ProgressUpdate::Report {
            message: report.message,
            percentage: report.percentage,
        },
        WorkDoneProgress::End(_) => ProgressUpdate::End,
    };
    Ok(Progress { token, update })
}

/// The parameters of `textDocument/publishDiagnostics`.
pub fn publish_diagnostics(params: &Value) -> Result<Published, String> {
    use lsp_types::PublishDiagnosticsParams;
    let params: PublishDiagnosticsParams =
        serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
    let path = path_of(params.uri.as_str()).ok_or_else(|| {
        format!(
            "diagnostics for {}, which is not a file",
            params.uri.as_str()
        )
    })?;
    let diagnostics = params
        .diagnostics
        .into_iter()
        .map(reported)
        .collect();
    Ok(Published {
        path,
        version: params.version,
        diagnostics,
    })
}

/// What the server said of one problem, in the terms of the editor.
fn reported(diagnostic: lsp_types::Diagnostic) -> Reported {
    use lsp_types::{DiagnosticSeverity, NumberOrString};
    let severity = match diagnostic.severity {
        Some(severity) if severity == DiagnosticSeverity::WARNING => Severity::Warning,
        Some(severity) if severity == DiagnosticSeverity::INFORMATION => Severity::Information,
        Some(severity) if severity == DiagnosticSeverity::HINT => Severity::Hint,
        // An error, and what a server that says nothing of it means.
        _ => Severity::Error,
    };
    let has_unnecessary_tag = diagnostic
        .tags
        .as_ref()
        .is_some_and(|tags| tags.contains(&lsp_types::DiagnosticTag::UNNECESSARY));
    let code = diagnostic.code.map(|code| match code {
        NumberOrString::Number(number) => number.to_string(),
        NumberOrString::String(text) => text,
    });
    // Code that is switched off by a `cfg` is said to be unnecessary, and is grayed out as unused code is.
    let unused = severity != Severity::Error
        && (has_unnecessary_tag || code.as_deref().is_some_and(is_unused_lint));
    Reported {
        range: (
            from_position(diagnostic.range.start),
            from_position(diagnostic.range.end),
        ),
        severity,
        message: diagnostic.message,
        code,
        unused,
    }
}

/// The parameters of `textDocument/diagnostic`, which asks for what the server finds wrong in the file `path`.
pub fn diagnostic_params(path: &Path) -> Value {
    json!({"textDocument": {"uri": uri_of(path)}})
}

/// What the server answered to `textDocument/diagnostic`: the problems it found, or none where it says that what it said
/// before is still so, which is not asked of it.
pub fn diagnostic_result(result: Value) -> Result<Option<Vec<Reported>>, String> {
    use lsp_types::DocumentDiagnosticReport;
    let report: DocumentDiagnosticReport =
        serde_json::from_value(result).map_err(|error| error.to_string())?;
    Ok(match report {
        DocumentDiagnosticReport::Full(full) => Some(
            full.full_document_diagnostic_report
                .items
                .into_iter()
                .map(reported)
                .collect(),
        ),
        DocumentDiagnosticReport::Unchanged(_) => None,
    })
}

/// Whether a rustc / rust-analyzer diagnostic code marks unused or unreachable code.
fn is_unused_lint(code: &str) -> bool {
    matches!(
        code,
        "dead_code"
            | "unused_imports"
            | "unused_variables"
            | "unused_assignments"
            | "unused_mut"
            | "unused_macros"
            | "unused_parens"
            | "unused_braces"
            | "unused_labels"
            | "unused_lifetimes"
            | "unused_qualifications"
            | "unused_extern_crates"
            | "unused_doc_comments"
            | "unreachable_code"
            | "unreachable_patterns"
            | "unreachable_pub"
    )
}

/// What kind of thing a completion item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Color,
    File,
    Reference,
    Folder,
    EnumMember,
    Constant,
    Struct,
    Event,
    Operator,
    TypeParameter,
    /// A kind that the item does not say, or that is not one of these.
    Other,
}

/// A completion item as the server said it, with its ranges as positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawItem {
    pub label: String,
    pub kind: ItemKind,
    pub detail: Option<String>,
    /// What is said beside the label, such as where an item that is not in scope is imported from.
    pub label_detail: Option<String>,
    pub sort: Option<String>,
    pub filter: Option<String>,
    /// What is put in the text: the edit's text, else the insert text, else the label, with the markup of a
    /// snippet taken out.
    pub text: String,
    /// What the text replaces, when the item says. None means the word before the caret.
    pub range: Option<(Position, Position)>,
    /// The other edits that putting the item in asks for, such as the line that imports it.
    pub extra: Vec<((Position, Position), String)>,
    /// What is sent to the server to ask for the other edits of an item that has them still to give: the item as
    /// far as the server needs it to know which it is. None for an item that has nothing to resolve.
    pub resolve: Option<Value>,
    pub deprecated: bool,
}

/// What the server answered to a request for completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completions {
    pub items: Vec<RawItem>,
    /// Whether the list is cut short, so that more of the word typed would give another.
    pub incomplete: bool,
}

/// A place in a file that something is defined at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    /// Where the name starts, as the server counts.
    pub at: Position,
    /// Where it ends: just after its last character.
    pub end: Position,
}

/// The parameters of `textDocument/definition` at `at` in the file `path`.
pub fn definition_params(path: &Path, at: Position) -> Value {
    let params = lsp_types::GotoDefinitionParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(path) },
            position: position(at),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    serde_json::to_value(params)
        .expect("the parameters of a request for a definition can be written")
}

/// What the server answered to a request for a definition: a place, a list of them, or links, which say where the name is
/// as well as where the whole of what it names is; or nothing. What is not in a file is left out.
pub fn definition_result(result: Value) -> Result<Vec<Place>, String> {
    use lsp_types::GotoDefinitionResponse;
    if result.is_null() {
        return Ok(Vec::new());
    }
    let place = |uri: &lsp_types::Uri, range: lsp_types::Range| {
        path_of(uri.as_str()).map(|path| Place {
            path,
            at: from_position(range.start),
            end: from_position(range.end),
        })
    };
    Ok(
        match serde_json::from_value::<GotoDefinitionResponse>(result)
            .map_err(|error| error.to_string())?
        {
            GotoDefinitionResponse::Scalar(location) => place(&location.uri, location.range)
                .into_iter()
                .collect(),
            GotoDefinitionResponse::Array(locations) => locations
                .iter()
                .filter_map(|location| place(&location.uri, location.range))
                .collect(),
            GotoDefinitionResponse::Link(links) => links
                .iter()
                .filter_map(|link| place(&link.target_uri, link.target_selection_range))
                .collect(),
        },
    )
}

/// The parameters of `textDocument/hover` at `at` in the file `path`.
pub fn hover_params(path: &Path, at: Position) -> Value {
    let params = lsp_types::HoverParams {
        text_document_position_params: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(path) },
            position: position(at),
        },
        work_done_progress_params: Default::default(),
    };
    serde_json::to_value(params).expect("the parameters of a request for a hover can be written")
}

/// What the server says of a name: the words, as markdown, and the part of the text they are about where it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverText {
    pub markdown: String,
    pub range: Option<(Position, Position)>,
}

/// What the server answered to a request for a hover: the words in one of the three shapes the protocol allows, or nothing.
/// Words that are only blanks are nothing.
pub fn hover_result(result: Value) -> Result<Option<HoverText>, String> {
    use lsp_types::{HoverContents, MarkedString};
    if result.is_null() {
        return Ok(None);
    }
    let hover: lsp_types::Hover =
        serde_json::from_value(result).map_err(|error| error.to_string())?;
    let marked = |marked: MarkedString| match marked {
        MarkedString::String(text) => text,
        // A snippet of code that is not markdown, and is made so by a fence, as the viewer draws it as code.
        MarkedString::LanguageString(code) => format!("```{}\n{}\n```", code.language, code.value),
    };
    let markdown = match hover.contents {
        HoverContents::Scalar(one) => marked(one),
        HoverContents::Array(many) => many
            .into_iter()
            .map(marked)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(content) => content.value,
    };
    let markdown = markdown.trim().to_owned();
    if markdown.is_empty() {
        return Ok(None);
    }
    Ok(Some(HoverText {
        markdown,
        range: hover
            .range
            .map(|range| (from_position(range.start), from_position(range.end))),
    }))
}

/// The parameters of `textDocument/completion` at `at` in the file `path`.
pub fn completion_params(path: &Path, at: Position) -> Value {
    let params = lsp_types::CompletionParams {
        text_document_position: lsp_types::TextDocumentPositionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(path) },
            position: position(at),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    serde_json::to_value(params).expect("the parameters of a request for completion can be written")
}

/// What the server answered to a request for completion: a list of items, a list that may be cut short, or
/// nothing.
pub fn completion_result(result: Value) -> Result<Completions, String> {
    use lsp_types::CompletionResponse;
    if result.is_null() {
        return Ok(Completions {
            items: Vec::new(),
            incomplete: false,
        });
    }
    // The answer is taken whole and not copied: a server may give thousands of items.
    let (items, incomplete) = match serde_json::from_value::<CompletionResponse>(result)
        .map_err(|error| error.to_string())?
    {
        CompletionResponse::Array(items) => (items, false),
        CompletionResponse::List(list) => (list.items, list.is_incomplete),
    };
    Ok(Completions {
        items: items.into_iter().map(raw_item).collect(),
        incomplete,
    })
}

/// What the server answered when asked for the rest of an item: the edits it asks for.
pub fn resolve_result(result: Value) -> Result<Vec<((Position, Position), String)>, String> {
    if result.is_null() {
        return Ok(Vec::new());
    }
    let item: lsp_types::CompletionItem =
        serde_json::from_value(result).map_err(|error| error.to_string())?;
    Ok(item
        .additional_text_edits
        .unwrap_or_default()
        .into_iter()
        .map(|edit| {
            (
                (
                    from_position(edit.range.start),
                    from_position(edit.range.end),
                ),
                edit.new_text,
            )
        })
        .collect())
}

fn raw_item(item: lsp_types::CompletionItem) -> RawItem {
    use lsp_types::{
        CompletionItemKind as K, CompletionItemTag, CompletionTextEdit, InsertTextFormat,
    };
    let kind = match item.kind {
        None => ItemKind::Other,
        Some(kind) => [
            (K::TEXT, ItemKind::Text),
            (K::METHOD, ItemKind::Method),
            (K::FUNCTION, ItemKind::Function),
            (K::CONSTRUCTOR, ItemKind::Constructor),
            (K::FIELD, ItemKind::Field),
            (K::VARIABLE, ItemKind::Variable),
            (K::CLASS, ItemKind::Class),
            (K::INTERFACE, ItemKind::Interface),
            (K::MODULE, ItemKind::Module),
            (K::PROPERTY, ItemKind::Property),
            (K::UNIT, ItemKind::Unit),
            (K::VALUE, ItemKind::Value),
            (K::ENUM, ItemKind::Enum),
            (K::KEYWORD, ItemKind::Keyword),
            (K::SNIPPET, ItemKind::Snippet),
            (K::COLOR, ItemKind::Color),
            (K::FILE, ItemKind::File),
            (K::REFERENCE, ItemKind::Reference),
            (K::FOLDER, ItemKind::Folder),
            (K::ENUM_MEMBER, ItemKind::EnumMember),
            (K::CONSTANT, ItemKind::Constant),
            (K::STRUCT, ItemKind::Struct),
            (K::EVENT, ItemKind::Event),
            (K::OPERATOR, ItemKind::Operator),
            (K::TYPE_PARAMETER, ItemKind::TypeParameter),
        ]
        .into_iter()
        .find_map(|(protocol, ours)| (protocol == kind).then_some(ours))
        .unwrap_or(ItemKind::Other),
    };
    let (text, range) = match item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => (edit.new_text, Some(edit.range)),
        // What is put in is the same wherever it is put: only the part of the word before the caret is replaced.
        Some(CompletionTextEdit::InsertAndReplace(edit)) => (edit.new_text, Some(edit.insert)),
        None => (
            item.insert_text
                .unwrap_or_else(|| item.label.clone()),
            None,
        ),
    };
    let text = if item.insert_text_format == Some(InsertTextFormat::SNIPPET) {
        plain_of_snippet(&text)
    } else {
        text
    };
    let extra = item
        .additional_text_edits
        .unwrap_or_default()
        .into_iter()
        .map(|edit| {
            (
                (
                    from_position(edit.range.start),
                    from_position(edit.range.end),
                ),
                edit.new_text,
            )
        })
        .collect();
    let deprecated = item.deprecated == Some(true)
        || item
            .tags
            .is_some_and(|tags| tags.contains(&CompletionItemTag::DEPRECATED));
    // rust-analyzer says that an item is to be imported by listing the imports in its data. What it does not say needs
    // nothing, and is not asked about when it is chosen.
    let resolve = item
        .data
        .filter(|data| {
            data.get("imports")
                .and_then(Value::as_array)
                .is_some_and(|imports| !imports.is_empty())
        })
        .map(|data| serde_json::json!({"label": item.label, "data": data}));
    let (label_detail, description) = item
        .label_details
        .map_or((None, None), |details| {
            (details.detail, details.description)
        });
    let said = |text: Option<String>| {
        text.map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    RawItem {
        label: item.label,
        kind,
        resolve,
        label_detail: said(label_detail),
        detail: said(item.detail).or_else(|| said(description)),
        sort: item.sort_text,
        filter: item.filter_text,
        text,
        range: range.map(|range| (from_position(range.start), from_position(range.end))),
        extra,
        deprecated,
    }
}

/// The text that a snippet is when it is typed out: its tab stops are nothing, a placeholder is its default,
/// a choice is the first of them, and a character that is escaped is itself.
fn plain_of_snippet(snippet: &str) -> String {
    fn go(chars: &mut std::iter::Peekable<std::str::Chars>, out: &mut String, inside: bool) {
        while let Some(c) = chars.next() {
            match c {
                '}' if inside => return,
                '\\' => match chars.peek() {
                    Some(&next) if matches!(next, '$' | '}' | '\\') => {
                        out.push(next);
                        chars.next();
                    }
                    _ => out.push(c),
                },
                '$' => match chars.peek() {
                    Some(d) if d.is_ascii_digit() => {
                        while chars.next_if(|d| d.is_ascii_digit()).is_some() {}
                    }
                    Some(&'{') => {
                        chars.next();
                        while chars.next_if(|d| d.is_ascii_digit()).is_some() {}
                        match chars.peek() {
                            Some(&':') => {
                                chars.next();
                                go(chars, out, true);
                            }
                            Some(&'|') => {
                                chars.next();
                                // The first of the choices, and the rest of them are skipped to the end.
                                while let Some(&c) = chars.peek() {
                                    if c == ',' || c == '|' {
                                        break;
                                    }
                                    out.push(c);
                                    chars.next();
                                }
                                for c in chars.by_ref() {
                                    if c == '}' {
                                        break;
                                    }
                                }
                            }
                            _ => {
                                // `${1}`, or a variable such as `${name}`: nothing.
                                for c in chars.by_ref() {
                                    if c == '}' {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Some(c) if c.is_alphabetic() || *c == '_' => {
                        while chars
                            .next_if(|c| c.is_alphanumeric() || *c == '_')
                            .is_some()
                        {}
                    }
                    _ => out.push(c),
                },
                _ => out.push(c),
            }
        }
    }
    let mut out = String::with_capacity(snippet.len());
    go(&mut snippet.chars().peekable(), &mut out, false);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::notification::Notification;
    use lsp_types::request::Request;
    use serde_json::json;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn the_method_names_are_the_ones_the_protocol_gives() {
        assert_eq!(
            INITIALIZE,
            <lsp_types::request::Initialize as Request>::METHOD
        );
        assert_eq!(SHUTDOWN, <lsp_types::request::Shutdown as Request>::METHOD);
        assert_eq!(
            INITIALIZED,
            <lsp_types::notification::Initialized as Notification>::METHOD
        );
        assert_eq!(
            EXIT,
            <lsp_types::notification::Exit as Notification>::METHOD
        );
        assert_eq!(
            DID_OPEN,
            <lsp_types::notification::DidOpenTextDocument as Notification>::METHOD
        );
        assert_eq!(
            DID_CHANGE,
            <lsp_types::notification::DidChangeTextDocument as Notification>::METHOD
        );
        assert_eq!(
            DID_CLOSE,
            <lsp_types::notification::DidCloseTextDocument as Notification>::METHOD
        );
        assert_eq!(
            PUBLISH_DIAGNOSTICS,
            <lsp_types::notification::PublishDiagnostics as Notification>::METHOD
        );
    }

    #[test]
    fn a_path_is_an_address_with_every_byte_that_is_not_plain_written_in_percent() {
        assert_eq!(
            uri_of(Path::new("/Users/a/src/main.rs")),
            "file:///Users/a/src/main.rs"
        );
        assert_eq!(
            uri_of(Path::new("/Users/a b/main.rs")),
            "file:///Users/a%20b/main.rs"
        );
        assert_eq!(
            uri_of(Path::new("/x/#hash/100%/a?b.rs")),
            "file:///x/%23hash/100%25/a%3Fb.rs"
        );
        assert_eq!(uri_of(Path::new("/x/\u{e9}.rs")), "file:///x/%C3%A9.rs");
        assert_eq!(
            uri_of(Path::new("/keeps/-._~/unreserved.rs")),
            "file:///keeps/-._~/unreserved.rs"
        );
    }

    #[test]
    fn an_address_is_the_path_it_was_made_from_and_only_a_file_address_is_one() {
        for path in [
            "/Users/a/src/main.rs",
            "/Users/a b/main.rs",
            "/x/#hash/100%/a?b.rs",
            "/x/\u{e9}/\u{4f60}\u{597d}/\u{1f600}.rs",
            "/",
        ] {
            assert_eq!(
                path_of(&uri_of(Path::new(path))),
                Some(PathBuf::from(path)),
                "{path}"
            );
        }
        assert_eq!(
            path_of("file:///a/b%2Fc.rs"),
            Some(PathBuf::from("/a/b/c.rs"))
        );
        assert_eq!(
            path_of("file://localhost/a/b.rs"),
            Some(PathBuf::from("/a/b.rs")),
            "a host that is this machine"
        );
        assert_eq!(path_of("http://example.com/a.rs"), None);
        assert_eq!(path_of("untitled:Untitled-1"), None);
        assert_eq!(
            path_of("file:///a/%zz.rs"),
            None,
            "a percent that is not a byte"
        );
        assert_eq!(path_of("file:///a/%ff.rs"), None, "bytes that are not text");
        assert_eq!(path_of("file:///a/%2"), None, "a percent that is cut off");
        assert_eq!(path_of(""), None);
    }

    #[test]
    fn initialize_tells_the_server_the_folder_and_that_positions_may_be_bytes() {
        let params = initialize_params(Path::new("/Users/a b/project"), 4242);
        assert_eq!(params["processId"], 4242);
        assert_eq!(params["rootUri"], "file:///Users/a%20b/project");
        assert_eq!(
            params["workspaceFolders"],
            json!([{"uri": "file:///Users/a%20b/project", "name": "project"}])
        );
        assert_eq!(params["clientInfo"]["name"], "Refractor");
        // UTF-8 first, so a server that can count bytes does, and UTF-16, which every server can.
        assert_eq!(
            params["capabilities"]["general"]["positionEncodings"],
            json!(["utf-8", "utf-16"])
        );
        assert_eq!(
            params["capabilities"]["textDocument"]["publishDiagnostics"]["versionSupport"],
            true
        );
        assert!(params["capabilities"]["textDocument"]["synchronization"].is_object());
        // What the server sends is read back as the types of the protocol say.
        serde_json::from_value::<lsp_types::InitializeParams>(params)
            .expect("the parameters are the protocol's");
    }

    #[test]
    fn initialize_asks_for_completion_as_plain_text_with_the_edits_of_an_item_asked_for_when_it_is_chosen()
     {
        let params = initialize_params(Path::new("/p"), 1);
        let item = &params["capabilities"]["textDocument"]["completion"]["completionItem"];
        // Snippets are not asked for: they are not something a text of the editor can be. And the edits of an
        // item, such as the line that imports it, are asked for when the item is chosen: a server offers the
        // items that are not in scope only to a client that does.
        assert_eq!(item["snippetSupport"], false);
        assert_eq!(item["insertReplaceSupport"], false);
        assert_eq!(
            item["resolveSupport"]["properties"],
            json!(["additionalTextEdits"])
        );
        serde_json::from_value::<lsp_types::InitializeParams>(params).expect("the protocol's own");
    }

    #[test]
    fn what_the_server_can_complete_is_read_from_what_it_answers_to_initialize() {
        let said = |capabilities: Value| {
            initialize_result(&json!({"capabilities": capabilities})).unwrap()
        };
        assert!(said(json!({"completionProvider": {"triggerCharacters": ["."]}})).completion);
        assert!(said(json!({"completionProvider": {}})).completion);
        assert!(!said(json!({})).completion);
    }

    #[test]
    fn a_request_for_completion_is_for_the_file_and_the_position() {
        let params = completion_params(
            Path::new("/p/src/main.rs"),
            Position {
                line: 3,
                character: 7,
            },
        );
        assert_eq!(params["textDocument"]["uri"], "file:///p/src/main.rs");
        assert_eq!(params["position"], json!({"line": 3, "character": 7}));
        assert_eq!(COMPLETION, "textDocument/completion");
        serde_json::from_value::<lsp_types::CompletionParams>(params).expect("the protocol's own");
    }

    fn range(line: u32, from: u32, to: u32) -> Value {
        json!({"start": {"line": line, "character": from}, "end": {"line": line, "character": to}})
    }

    #[test]
    fn a_list_of_items_is_read_with_what_each_says_of_itself() {
        let answer = json!({"isIncomplete": true, "items": [{
            "label": "push_str", "kind": 2, "detail": "fn(&mut self, &str)", "sortText": "0001", "filterText": "push_str",
            "textEdit": {"range": range(4, 8, 10), "newText": "push_str"},
            "additionalTextEdits": [{"range": range(0, 0, 0), "newText": "use a::b;\n"}],
            "tags": [1]
        }]});
        let found = completion_result(answer).unwrap();
        assert!(found.incomplete);
        let item = &found.items[0];
        assert_eq!(
            (item.label.as_str(), item.kind, item.detail.as_deref()),
            ("push_str", ItemKind::Method, Some("fn(&mut self, &str)"))
        );
        assert_eq!(
            (item.sort.as_deref(), item.filter.as_deref()),
            (Some("0001"), Some("push_str"))
        );
        assert_eq!(item.text, "push_str");
        assert_eq!(
            item.range,
            Some((
                Position {
                    line: 4,
                    character: 8
                },
                Position {
                    line: 4,
                    character: 10
                }
            ))
        );
        assert_eq!(
            item.extra,
            vec![(
                (
                    Position {
                        line: 0,
                        character: 0
                    },
                    Position {
                        line: 0,
                        character: 0
                    }
                ),
                "use a::b;\n".to_owned()
            )]
        );
        assert!(item.deprecated);
    }

    #[test]
    fn a_plain_array_of_items_is_an_answer_that_is_complete_and_null_is_no_items() {
        let found = completion_result(json!([{"label": "a"}, {"label": "b", "kind": 14}])).unwrap();
        assert!(!found.incomplete);
        assert_eq!(
            found
                .items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(
            found.items[0].kind,
            ItemKind::Other,
            "an item that says no kind"
        );
        assert_eq!(found.items[1].kind, ItemKind::Keyword);
        let none = completion_result(Value::Null).unwrap();
        assert!(none.items.is_empty() && !none.incomplete);
        assert!(completion_result(json!("not an answer")).is_err());
        assert!(
            completion_result(json!({"items": [{"kind": 3}]})).is_err(),
            "an item that has no label"
        );
    }

    #[test]
    fn what_an_item_says_beside_its_label_is_kept_and_a_description_is_its_detail_where_it_has_none()
     {
        let found = completion_result(json!([
            {"label": "push::", "labelDetails": {"detail": " (use a::push)", "description": "mod push"}},
            {"label": "len", "detail": "fn(&self) -> usize", "labelDetails": {"description": "fn(&self) -> usize2"}},
            {"label": "plain"}
        ]))
        .unwrap();
        assert_eq!(
            (
                found.items[0].label_detail.as_deref(),
                found.items[0].detail.as_deref()
            ),
            (Some("(use a::push)"), Some("mod push"))
        );
        assert_eq!(
            (
                found.items[1].label_detail.as_deref(),
                found.items[1].detail.as_deref()
            ),
            (None, Some("fn(&self) -> usize")),
            "its own detail comes first"
        );
        assert_eq!(
            (
                found.items[2].label_detail.as_deref(),
                found.items[2].detail.as_deref()
            ),
            (None, None)
        );
    }

    #[test]
    fn initialize_says_that_the_client_shows_what_an_item_says_beside_its_label() {
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["completion"]["completionItem"]["labelDetailsSupport"],
            true
        );
    }

    #[test]
    fn an_item_with_no_edit_is_its_insert_text_and_then_its_label_and_has_no_range() {
        let found = completion_result(
            json!([{"label": "label"}, {"label": "shown", "insertText": "inserted"}]),
        )
        .unwrap();
        assert_eq!(
            (found.items[0].text.as_str(), found.items[0].range),
            ("label", None)
        );
        assert_eq!(
            (found.items[1].text.as_str(), found.items[1].range),
            ("inserted", None)
        );
    }

    #[test]
    fn an_edit_that_inserts_and_replaces_is_taken_as_the_insert() {
        let found = completion_result(json!([{"label": "x", "textEdit": {"newText": "xyz", "insert": range(1, 2, 3), "replace": range(1, 2, 6)}}])).unwrap();
        assert_eq!(
            found.items[0].range,
            Some((
                Position {
                    line: 1,
                    character: 2
                },
                Position {
                    line: 1,
                    character: 3
                }
            ))
        );
        assert_eq!(found.items[0].text, "xyz");
    }

    #[test]
    fn the_kinds_of_the_protocol_are_the_kinds_of_an_item() {
        let kind_of = |number: i64| {
            completion_result(json!([{"label": "x", "kind": number}]))
                .unwrap()
                .items[0]
                .kind
        };
        let kinds: Vec<_> = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 99,
        ]
        .map(kind_of)
        .into();
        use ItemKind::*;
        assert_eq!(
            kinds,
            [
                Text,
                Method,
                Function,
                Constructor,
                Field,
                Variable,
                Class,
                Interface,
                Module,
                Property,
                Unit,
                Value,
                Enum,
                Keyword,
                Snippet,
                Color,
                File,
                Reference,
                Folder,
                EnumMember,
                Constant,
                Struct,
                Event,
                Operator,
                TypeParameter,
                Other
            ]
        );
    }

    #[test]
    fn an_item_that_is_to_be_imported_says_what_to_send_to_have_the_edit_and_the_others_do_not() {
        let imported = json!({"label": "Circle", "data": {"imports": [{"full_import_path": "shapes::Circle", "imported_name": "Circle"}], "position": {"line": 1}}});
        let plain = json!({"label": "len", "data": {"imports": [], "position": {"line": 1}}});
        let none = json!({"label": "x"});
        let found = completion_result(json!([imported, plain, none])).unwrap();
        assert_eq!(
            found.items[0].resolve,
            Some(
                json!({"label": "Circle", "data": {"imports": [{"full_import_path": "shapes::Circle", "imported_name": "Circle"}], "position": {"line": 1}}})
            )
        );
        assert_eq!(
            found.items[1].resolve, None,
            "an item that is not to be imported has nothing to resolve"
        );
        assert_eq!(found.items[2].resolve, None);
    }

    #[test]
    fn the_edits_of_a_resolved_item_are_the_ones_it_asks_for() {
        let edits = resolve_result(json!({"label": "Circle", "additionalTextEdits": [
            {"range": range(0, 0, 0), "newText": "use shapes::Circle;\n"},
            {"range": range(2, 1, 4), "newText": "x"}
        ]}))
        .unwrap();
        assert_eq!(
            edits,
            vec![
                (
                    (
                        Position {
                            line: 0,
                            character: 0
                        },
                        Position {
                            line: 0,
                            character: 0
                        }
                    ),
                    "use shapes::Circle;\n".to_owned()
                ),
                (
                    (
                        Position {
                            line: 2,
                            character: 1
                        },
                        Position {
                            line: 2,
                            character: 4
                        }
                    ),
                    "x".to_owned()
                ),
            ]
        );
        assert!(
            resolve_result(json!({"label": "x"}))
                .unwrap()
                .is_empty(),
            "an item that asks for none"
        );
        assert!(resolve_result(Value::Null).unwrap().is_empty());
        assert!(resolve_result(json!("nope")).is_err());
        assert_eq!(COMPLETION_RESOLVE, "completionItem/resolve");
    }

    #[test]
    fn the_server_says_whether_it_can_find_a_definition_and_the_client_asks_for_places_and_not_links()
     {
        let said = |capabilities: Value| {
            initialize_result(&json!({"capabilities": capabilities})).unwrap()
        };
        assert!(said(json!({"definitionProvider": true})).definition);
        assert!(said(json!({"definitionProvider": {"workDoneProgress": true}})).definition);
        assert!(!said(json!({})).definition);
        assert!(!said(json!({"definitionProvider": false})).definition);
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["definition"]["linkSupport"],
            false
        );
        assert_eq!(DEFINITION, "textDocument/definition");
        serde_json::from_value::<lsp_types::InitializeParams>(params).expect("the protocol's own");
    }

    #[test]
    fn the_server_says_whether_it_says_what_a_name_is_and_the_client_asks_for_markdown() {
        let said = |capabilities: Value| {
            initialize_result(&json!({"capabilities": capabilities})).unwrap()
        };
        assert!(said(json!({"hoverProvider": true})).hover);
        assert!(said(json!({"hoverProvider": {"workDoneProgress": true}})).hover);
        assert!(!said(json!({})).hover);
        assert!(!said(json!({"hoverProvider": false})).hover);
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["hover"]["contentFormat"],
            json!(["markdown", "plaintext"])
        );
        assert_eq!(HOVER, "textDocument/hover");
        serde_json::from_value::<lsp_types::InitializeParams>(params).expect("the protocol's own");
    }

    #[test]
    fn a_request_for_a_hover_is_for_the_file_and_the_position() {
        let params = hover_params(
            Path::new("/p/src/main.rs"),
            Position {
                line: 2,
                character: 9,
            },
        );
        assert_eq!(params["textDocument"]["uri"], "file:///p/src/main.rs");
        assert_eq!(params["position"], json!({"line": 2, "character": 9}));
        serde_json::from_value::<lsp_types::HoverParams>(params).expect("the protocol's own");
    }

    #[test]
    fn a_hover_is_read_whether_the_server_gives_markup_a_marked_string_or_a_list_of_them() {
        let range =
            json!({"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 9}});
        let markup = hover_result(json!({"contents": {"kind": "markdown", "value": "```rust\nfn f()\n```\n\nDoes a thing.\n"}, "range": range})).unwrap().unwrap();
        assert_eq!(
            markup.markdown, "```rust\nfn f()\n```\n\nDoes a thing.",
            "trimmed"
        );
        assert_eq!(
            markup.range,
            Some((
                Position {
                    line: 1,
                    character: 4
                },
                Position {
                    line: 1,
                    character: 9
                }
            ))
        );
        let plain = hover_result(json!({"contents": "just words"}))
            .unwrap()
            .unwrap();
        assert_eq!(
            (plain.markdown.as_str(), plain.range),
            ("just words", None),
            "a string, and no range"
        );
        let code = hover_result(json!({"contents": {"language": "rust", "value": "let x: u8"}}))
            .unwrap()
            .unwrap();
        assert_eq!(
            code.markdown, "```rust\nlet x: u8\n```",
            "code is fenced so that it is drawn as code"
        );
        let many =
            hover_result(json!({"contents": [{"language": "rust", "value": "fn f()"}, "Words."]}))
                .unwrap()
                .unwrap();
        assert_eq!(many.markdown, "```rust\nfn f()\n```\n\nWords.");
    }

    #[test]
    fn no_hover_is_none_and_words_of_blanks_are_none_and_what_is_not_a_hover_is_an_error() {
        assert_eq!(hover_result(Value::Null).unwrap(), None);
        assert_eq!(hover_result(json!({"contents": "  \n "})).unwrap(), None);
        assert_eq!(hover_result(json!({"contents": []})).unwrap(), None);
        assert!(hover_result(json!("nope")).is_err());
        assert!(hover_result(json!({"contents": 7})).is_err());
    }

    #[test]
    fn a_request_for_a_definition_is_for_the_file_and_the_position() {
        let params = definition_params(
            Path::new("/p/src/main.rs"),
            Position {
                line: 8,
                character: 3,
            },
        );
        assert_eq!(params["textDocument"]["uri"], "file:///p/src/main.rs");
        assert_eq!(params["position"], json!({"line": 8, "character": 3}));
        serde_json::from_value::<lsp_types::GotoDefinitionParams>(params)
            .expect("the protocol's own");
    }

    #[test]
    fn a_definition_is_read_whether_the_server_gives_one_place_a_list_or_links() {
        let place = |uri: &str, line: u32, character: u32| json!({"uri": uri, "range": range(line, character, character + 2)});
        // The places the tests give are two characters wide, and a place says where it ends as well as where it starts.
        let at = |path: &str, line, character, width| Place {
            path: PathBuf::from(path),
            at: Position { line, character },
            end: Position {
                line,
                character: character + width,
            },
        };
        assert_eq!(
            definition_result(place("file:///p/a.rs", 3, 4)).unwrap(),
            vec![at("/p/a.rs", 3, 4, 2)],
            "one place"
        );
        assert_eq!(
            definition_result(json!([
                place("file:///p/a.rs", 1, 0),
                place("file:///p/b.rs", 7, 2)
            ]))
            .unwrap(),
            vec![at("/p/a.rs", 1, 0, 2), at("/p/b.rs", 7, 2, 2)],
            "a list of them"
        );
        // A link says where the name is as well as where the whole of the thing is: it is the name that is gone to.
        let link = json!({"targetUri": "file:///p/c.rs", "targetRange": range(10, 0, 30), "targetSelectionRange": range(10, 7, 12)});
        assert_eq!(
            definition_result(json!([link])).unwrap(),
            vec![at("/p/c.rs", 10, 7, 5)]
        );
    }

    #[test]
    fn no_definition_is_none_and_what_is_not_a_place_is_an_error_and_a_place_that_is_not_a_file_is_left_out()
     {
        assert!(
            definition_result(Value::Null)
                .unwrap()
                .is_empty()
        );
        assert!(definition_result(json!([])).unwrap().is_empty());
        assert!(definition_result(json!("nope")).is_err());
        assert!(definition_result(json!({"uri": 7})).is_err());
        // A definition in what is not a file, such as a page on the web, is not somewhere to go to.
        let web = json!({"uri": "https://example.com/x.rs", "range": range(0, 0, 1)});
        let file = json!({"uri": "file:///p/a.rs", "range": range(2, 1, 3)});
        let found = definition_result(json!([web, file])).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, PathBuf::from("/p/a.rs"));
    }

    #[test]
    fn a_snippet_is_made_the_plain_text_it_would_be_typed_as() {
        let item = |text: &str| {
            completion_result(json!([{"label": "x", "insertTextFormat": 2, "insertText": text}]))
                .unwrap()
                .items
                .remove(0)
                .text
        };
        assert_eq!(item("foo($0)"), "foo()");
        assert_eq!(item("foo(${1:arg}, ${2:other})$0"), "foo(arg, other)");
        assert_eq!(item("a$1b"), "ab");
        assert_eq!(
            item("cost: \\$5 and \\}"),
            "cost: $5 and }",
            "an escaped character is itself"
        );
        assert_eq!(item("${1|one,two|}"), "one", "the first of the choices");
        assert_eq!(
            item("${1:outer ${2:inner}}"),
            "outer inner",
            "a placeholder in a placeholder"
        );
        // Plain text is as it is, whatever it has in it.
        let plain = completion_result(json!([{"label": "x", "insertText": "$0 and ${1}"}]))
            .unwrap()
            .items
            .remove(0)
            .text;
        assert_eq!(plain, "$0 and ${1}");
    }

    #[test]
    fn initialize_says_that_the_client_shows_the_progress_of_what_the_server_is_doing() {
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(params["capabilities"]["window"]["workDoneProgress"], true);
    }

    #[test]
    fn initialize_says_that_the_client_watches_the_files_so_that_the_server_does_not() {
        // A server that is not told that the client watches the files watches them itself, every folder of
        // the project, which for a project with a build folder of hundreds of thousands of files in it takes
        // a quarter of an hour of the machine.
        let params = initialize_params(Path::new("/p"), 1);
        let watching = &params["capabilities"]["workspace"]["didChangeWatchedFiles"];
        assert_eq!(watching["dynamicRegistration"], true);
        assert_eq!(watching["relativePatternSupport"], true);
        serde_json::from_value::<lsp_types::InitializeParams>(params).expect("the protocol's own");
    }

    #[test]
    fn initialize_asks_rust_analyzer_to_say_when_it_is_busy_and_when_it_is_not() {
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["experimental"]["serverStatusNotification"],
            true
        );
        assert_eq!(SERVER_STATUS, "experimental/serverStatus");
    }

    #[test]
    fn the_status_the_server_reports_is_whether_it_is_idle_how_it_is_and_what_it_says() {
        let status = |value: Value| server_status(&value);
        assert_eq!(
            status(json!({"health": "ok", "quiescent": false, "message": "Loading the workspace"})),
            Ok(ServerStatus {
                quiescent: false,
                health: Health::Ok,
                message: Some("Loading the workspace".into())
            })
        );
        assert_eq!(
            status(json!({"health": "ok", "quiescent": true})),
            Ok(ServerStatus {
                quiescent: true,
                health: Health::Ok,
                message: None
            })
        );
        assert_eq!(
            status(json!({"health": "warning", "quiescent": true, "message": ""}))
                .unwrap()
                .message,
            None,
            "a message that says nothing is none"
        );
        assert_eq!(
            status(json!({"health": "warning", "quiescent": true, "message": "no cargo"}))
                .unwrap()
                .health,
            Health::Warning
        );
        assert_eq!(
            status(json!({"health": "error", "quiescent": false}))
                .unwrap()
                .health,
            Health::Error
        );
        assert!(status(json!({"health": "wobbly", "quiescent": true})).is_err());
        assert!(
            status(json!({"health": "ok"})).is_err(),
            "no word on whether it is idle"
        );
        assert!(status(Value::Null).is_err());
    }

    #[test]
    fn a_job_that_the_server_reports_is_read_as_it_begins_goes_on_and_ends() {
        let begin = progress(&json!({"token": "rustAnalyzer/Indexing", "value": {"kind": "begin", "title": "Indexing", "cancellable": false, "percentage": 0}})).unwrap();
        assert_eq!(
            begin,
            Progress {
                token: "rustAnalyzer/Indexing".into(),
                update: ProgressUpdate::Begin {
                    title: "Indexing".into(),
                    message: None,
                    percentage: Some(0)
                }
            }
        );
        let report = progress(&json!({"token": "rustAnalyzer/Indexing", "value": {"kind": "report", "message": "3/118 (aimer_text)", "percentage": 2, "cancellable": false}})).unwrap();
        assert_eq!(
            report.update,
            ProgressUpdate::Report {
                message: Some("3/118 (aimer_text)".into()),
                percentage: Some(2)
            }
        );
        let bare_report = progress(&json!({"token": "t", "value": {"kind": "report"}})).unwrap();
        assert_eq!(
            bare_report.update,
            ProgressUpdate::Report {
                message: None,
                percentage: None
            }
        );
        let end =
            progress(&json!({"token": "rustAnalyzer/Indexing", "value": {"kind": "end"}})).unwrap();
        assert_eq!(end.update, ProgressUpdate::End);
        let end_with_message =
            progress(&json!({"token": "x", "value": {"kind": "end", "message": "done"}})).unwrap();
        assert_eq!(end_with_message.update, ProgressUpdate::End);
        // A token may be a number, which is written as its digits.
        let numbered =
            progress(&json!({"token": 7, "value": {"kind": "begin", "title": "Building"}}))
                .unwrap();
        assert_eq!(numbered.token, "7");
        assert_eq!(
            numbered.update,
            ProgressUpdate::Begin {
                title: "Building".into(),
                message: None,
                percentage: None
            }
        );
        assert_eq!(
            PROGRESS,
            <lsp_types::notification::Progress as Notification>::METHOD
        );
    }

    #[test]
    fn what_is_not_the_progress_of_a_job_is_an_error() {
        assert!(
            progress(&json!({"value": {"kind": "end"}})).is_err(),
            "no token"
        );
        assert!(progress(&json!({"token": "t"})).is_err(), "no value");
        assert!(
            progress(&json!({"token": "t", "value": {"kind": "wobble"}})).is_err(),
            "a kind that is not one"
        );
        assert!(
            progress(&json!({"token": "t", "value": {"kind": "begin"}})).is_err(),
            "a begin with no title"
        );
        assert!(progress(&Value::Null).is_err());
    }

    #[test]
    fn the_server_says_how_it_wants_changes_and_what_a_character_is() {
        let caps = |result: Value| initialize_result(&result);
        let sync = |value: Value| {
            caps(json!({"capabilities": {"textDocumentSync": value}}))
                .unwrap()
                .sync
        };
        assert_eq!(sync(json!(2)), SyncKind::Incremental);
        assert_eq!(sync(json!(1)), SyncKind::Full);
        assert_eq!(sync(json!(0)), SyncKind::None);
        assert_eq!(
            sync(json!({"openClose": true, "change": 2})),
            SyncKind::Incremental
        );
        assert_eq!(
            sync(json!({"openClose": true, "change": 1})),
            SyncKind::Full
        );
        assert_eq!(
            sync(json!({"openClose": true})),
            SyncKind::None,
            "no say about changes is none"
        );
        assert_eq!(
            caps(json!({"capabilities": {}})).unwrap().sync,
            SyncKind::None
        );

        let encoding = |value: Value| caps(json!({"capabilities": {"positionEncoding": value}}));
        assert_eq!(encoding(json!("utf-8")).unwrap().encoding, Encoding::Utf8);
        assert_eq!(encoding(json!("utf-16")).unwrap().encoding, Encoding::Utf16);
        assert_eq!(
            caps(json!({"capabilities": {}}))
                .unwrap()
                .encoding,
            Encoding::Utf16,
            "UTF-16 when the server says nothing"
        );
        assert!(
            encoding(json!("utf-32")).is_err(),
            "a client that did not offer it cannot use it"
        );
        assert!(caps(json!({"nothing": 1})).is_err());
        assert!(caps(json!(null)).is_err());
    }

    #[test]
    fn a_file_is_opened_with_its_text_and_closed_by_its_address() {
        let open = did_open_params(Path::new("/p/a b.rs"), 1, "fn main() {}\n");
        assert_eq!(
            open,
            json!({"textDocument": {"uri": "file:///p/a%20b.rs", "languageId": "rust", "version": 1, "text": "fn main() {}\n"}})
        );
        assert_eq!(
            did_close_params(Path::new("/p/a b.rs")),
            json!({"textDocument": {"uri": "file:///p/a%20b.rs"}})
        );
        serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(open)
            .expect("the protocol's own");
    }

    #[test]
    fn a_save_is_told_by_the_address_of_the_file_and_the_client_says_it_does_so() {
        assert_eq!(DID_SAVE, "textDocument/didSave");
        assert_eq!(
            did_save_params(Path::new("/p/a b.rs")),
            json!({"textDocument": {"uri": "file:///p/a%20b.rs"}})
        );
        let params = initialize_params(Path::new("/p"), 1);
        assert_eq!(
            params["capabilities"]["textDocument"]["synchronization"]["didSave"],
            json!(true)
        );
    }

    #[test]
    fn the_server_is_asked_to_check_the_project_when_a_file_is_saved() {
        let options = &initialize_params(Path::new("/p"), 1)["initializationOptions"];
        assert_eq!(
            options["checkOnSave"],
            json!(true),
            "what rust-analyzer runs `cargo check` for"
        );
        assert_eq!(options["check"]["command"], json!("check"));
        assert!(
            options["lens"].is_object(),
            "and what was asked of it before still is"
        );
    }

    #[test]
    fn a_change_is_a_version_and_either_the_whole_text_or_a_range_and_what_replaces_it() {
        let path = Path::new("/p/main.rs");
        let incremental = did_change_params(
            path,
            7,
            &Change::Incremental {
                range: (at(1, 4), at(1, 9)),
                text: "new".into(),
            },
        );
        assert_eq!(
            incremental,
            json!({
                "textDocument": {"uri": "file:///p/main.rs", "version": 7},
                "contentChanges": [{"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 9}}, "text": "new"}]
            })
        );
        let full = did_change_params(path, 8, &Change::Full("everything".into()));
        assert_eq!(
            full,
            json!({"textDocument": {"uri": "file:///p/main.rs", "version": 8}, "contentChanges": [{"text": "everything"}]})
        );
        serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(incremental)
            .expect("the protocol's own");
    }

    #[test]
    fn the_diagnostics_of_a_file_are_read_with_their_ranges_severities_and_codes() {
        let params = json!({
            "uri": "file:///p/main.rs",
            "version": 12,
            "diagnostics": [
                {"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 7}}, "severity": 1, "code": "E0425", "source": "rustc", "message": "cannot find value"},
                {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 1}}, "severity": 2, "code": 17, "message": "unused"},
                {"range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 1}}, "severity": 3, "message": "note"},
                {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 1}}, "severity": 4, "message": "hint"},
                {"range": {"start": {"line": 5, "character": 0}, "end": {"line": 5, "character": 1}}, "message": "no severity"}
            ]
        });
        let published = publish_diagnostics(&params).unwrap();
        assert_eq!(published.path, PathBuf::from("/p/main.rs"));
        assert_eq!(published.version, Some(12));
        let summary: Vec<_> = published
            .diagnostics
            .iter()
            .map(|d| {
                (
                    d.range.0,
                    d.range.1,
                    d.severity,
                    d.code.as_deref(),
                    d.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    at(0, 3),
                    at(0, 7),
                    Severity::Error,
                    Some("E0425"),
                    "cannot find value"
                ),
                (at(2, 0), at(2, 1), Severity::Warning, Some("17"), "unused"),
                (at(3, 0), at(3, 1), Severity::Information, None, "note"),
                (at(4, 0), at(4, 1), Severity::Hint, None, "hint"),
                (at(5, 0), at(5, 1), Severity::Error, None, "no severity"),
            ]
        );
        let none =
            publish_diagnostics(&json!({"uri": "file:///p/main.rs", "diagnostics": []})).unwrap();
        assert_eq!(
            (none.version, none.diagnostics.len()),
            (None, 0),
            "an empty list clears the file's diagnostics"
        );
    }

    #[test]
    fn what_is_not_a_publication_of_diagnostics_for_a_file_is_an_error() {
        assert!(
            publish_diagnostics(&json!({"diagnostics": []})).is_err(),
            "no address"
        );
        assert!(
            publish_diagnostics(&json!({"uri": "http://x/y", "diagnostics": []})).is_err(),
            "not a file"
        );
        assert!(
            publish_diagnostics(&json!({"uri": "file:///a.rs"})).is_err(),
            "no diagnostics"
        );
        assert!(
            publish_diagnostics(
                &json!({"uri": "file:///a.rs", "diagnostics": [{"message": "no range"}]})
            )
            .is_err()
        );
        assert!(publish_diagnostics(&Value::Null).is_err());
    }
}
