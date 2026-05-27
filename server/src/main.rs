use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CodeAction, CodeActionOrCommand, CodeActionParams, CodeActionProviderCapability,
    ExecuteCommandOptions, ExecuteCommandParams, InitializeParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
    notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument},
    request::{CodeActionRequest, ExecuteCommand, Shutdown},
};
use serde_json::{Value, json};
use tempfile::{TempDir, TempPath};

const MARK_FIRST: &str = "diffTool.markFirst";
const MARK_SECOND: &str = "diffTool.markSecond";

#[derive(Clone)]
struct Document {
    uri: Uri,
    text: String,
}

struct State {
    docs: HashMap<Uri, Document>,
    first: Option<Document>,
    second: Option<Document>,
    active_diff: Option<ActiveDiff>,
    marker_path: TempPath,
    temp_dirs: Vec<TempDir>,
}

impl State {
    fn new() -> io::Result<Self> {
        let marker_path = tempfile::Builder::new()
            .prefix("zed-diff-tool-first-marker-")
            .suffix(".json")
            .tempfile()?
            .into_temp_path();
        Ok(Self {
            docs: HashMap::new(),
            first: None,
            second: None,
            active_diff: None,
            marker_path,
            temp_dirs: Vec::new(),
        })
    }

    fn marker_path(&self) -> &Path {
        self.marker_path.as_ref()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let _params: InitializeParams = serde_json::from_value(initialize_params)?;
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![MARK_FIRST.into(), MARK_SECOND.into()],
            work_done_progress_options: Default::default(),
        }),
        ..Default::default()
    };
    connection.initialize_finish(initialize_id, json!({ "capabilities": capabilities }))?;

    let mut state = State::new()?;
    for message in &connection.receiver {
        match message {
            Message::Notification(notification) => match notification.method.as_str() {
                DidOpenTextDocument::METHOD => {
                    let params: lsp_types::DidOpenTextDocumentParams =
                        serde_json::from_value(notification.params)?;
                    state.docs.insert(
                        params.text_document.uri.clone(),
                        Document {
                            uri: params.text_document.uri,
                            text: params.text_document.text,
                        },
                    );
                }
                DidChangeTextDocument::METHOD => {
                    let params: lsp_types::DidChangeTextDocumentParams =
                        serde_json::from_value(notification.params)?;
                    let uri = params.text_document.uri;
                    if let Some(change) = params.content_changes.into_iter().last() {
                        state.docs.insert(
                            uri.clone(),
                            Document {
                                uri: uri.clone(),
                                text: change.text,
                            },
                        );
                        refresh_active_diff_for_uri(&state, &uri).ok();
                    }
                }
                DidCloseTextDocument::METHOD => {
                    let params: lsp_types::DidCloseTextDocumentParams =
                        serde_json::from_value(notification.params)?;
                    state.docs.remove(&params.text_document.uri);
                }
                _ => {}
            },
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }

                match request.method.as_str() {
                    CodeActionRequest::METHOD => {
                        let id = request.id.clone();
                        let params: CodeActionParams = serde_json::from_value(request.params)?;
                        connection.sender.send(Message::Response(Response {
                            id,
                            result: Some(code_actions(&params.text_document.uri)),
                            error: None,
                        }))?;
                    }
                    ExecuteCommand::METHOD => {
                        let id = request.id.clone();
                        let params: ExecuteCommandParams = serde_json::from_value(request.params)?;
                        let result = execute_command(&mut state, params);
                        connection.sender.send(Message::Response(Response {
                            id,
                            result: Some(json!(result.is_ok())),
                            error: result.err().map(|message| lsp_server::ResponseError {
                                code: lsp_server::ErrorCode::InternalError as i32,
                                message,
                                data: None,
                            }),
                        }))?;
                    }
                    Shutdown::METHOD => {
                        let id = request.id.clone();
                        connection
                            .sender
                            .send(Message::Response(Response::new_ok(id, Value::Null)))?;
                    }
                    _ => connection.sender.send(Message::Response(Response::new_err(
                        request.id.clone(),
                        lsp_server::ErrorCode::MethodNotFound as i32,
                        format!("unsupported request: {}", request.method),
                    )))?,
                }
            }
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}

fn code_actions(uri: &Uri) -> Value {
    let args = vec![json!(uri.as_str())];
    let first = CodeAction {
        title: "DiffTool: Mark 1st file".into(),
        command: Some(lsp_types::Command {
            title: "DiffTool: Mark 1st file".into(),
            command: MARK_FIRST.into(),
            arguments: Some(args.clone()),
        }),
        ..Default::default()
    };
    let second = CodeAction {
        title: "DiffTool: Mark 2nd file".into(),
        command: Some(lsp_types::Command {
            title: "DiffTool: Mark 2nd file".into(),
            command: MARK_SECOND.into(),
            arguments: Some(args),
        }),
        ..Default::default()
    };

    serde_json::to_value(vec![
        CodeActionOrCommand::CodeAction(first),
        CodeActionOrCommand::CodeAction(second),
    ])
    .expect("serializing code actions should not fail")
}

fn execute_command(state: &mut State, params: ExecuteCommandParams) -> Result<(), String> {
    let uri = params
        .arguments
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| "missing document uri argument".to_string())?;
    let uri: Uri = uri
        .parse()
        .map_err(|error| format!("invalid document uri: {error}"))?;
    let document = state.docs.get(&uri).cloned().ok_or_else(|| {
        format!(
            "document is not open in the diff-tool LSP session: {}",
            uri.as_str()
        )
    })?;

    let marked_second = match params.command.as_str() {
        MARK_FIRST => {
            save_first_marker(state.marker_path(), &document)
                .map_err(|error| format!("failed to save first marker: {error}"))?;
            state.first = Some(document);
            false
        }
        MARK_SECOND => {
            state.second = Some(document);
            true
        }
        _ => return Err(format!("unsupported command: {}", params.command)),
    };

    if marked_second && state.second.is_some() {
        let first = state
            .first
            .take()
            .or_else(|| load_first_marker(state.marker_path()).ok().flatten());
        let Some(first) = first else {
            return Ok(());
        };
        let second = state.second.take().expect("second document was checked");
        let first = latest_document(state, first);
        let second = latest_document(state, second);
        let diff_inputs = write_diff_inputs(&first, &second)
            .map_err(|error| format!("failed to write diff inputs: {error}"))?;
        let first_path = diff_inputs.first_path.clone();
        let second_path = diff_inputs.second_path.clone();
        state.active_diff = Some(ActiveDiff {
            first_uri: first.uri,
            second_uri: second.uri,
            first_path: first_path.clone(),
            second_path: second_path.clone(),
        });
        state.temp_dirs.push(diff_inputs.dir);
        open_diff_in_zed(&first_path, &second_path).map_err(|error| {
            format!(
                "created diff inputs at {} and {}, but failed to open them in Zed: {error}",
                first_path.display(),
                second_path.display()
            )
        })?;
        remove_first_marker(state.marker_path()).ok();
    }

    Ok(())
}

fn latest_document(state: &State, document: Document) -> Document {
    state.docs.get(&document.uri).cloned().unwrap_or(document)
}

fn save_first_marker(path: &Path, document: &Document) -> io::Result<()> {
    let marker = json!({
        "uri": document.uri.as_str(),
        "text": document.text,
    });
    fs::write(path, marker.to_string())
}

fn load_first_marker(path: &Path) -> io::Result<Option<Document>> {
    let marker = match fs::read_to_string(path) {
        Ok(marker) => marker,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let marker: Value = serde_json::from_str(&marker).map_err(io::Error::other)?;
    let Some(uri) = marker.get("uri").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(text) = marker.get("text").and_then(Value::as_str) else {
        return Ok(None);
    };
    let uri = uri.parse().map_err(io::Error::other)?;
    Ok(Some(Document {
        uri,
        text: text.to_string(),
    }))
}

fn remove_first_marker(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

struct DiffInputs {
    dir: TempDir,
    first_path: PathBuf,
    second_path: PathBuf,
}

struct ActiveDiff {
    first_uri: Uri,
    second_uri: Uri,
    first_path: PathBuf,
    second_path: PathBuf,
}

fn write_diff_inputs(first: &Document, second: &Document) -> io::Result<DiffInputs> {
    let dir = tempfile::Builder::new()
        .prefix("zed-diff-tool-")
        .tempdir()?;
    let first_label = display_name(&first.uri, "first");
    let second_label = display_name(&second.uri, "second");
    let first_path = dir.path().join(unique_diff_filename("left", &first_label));
    let second_path = dir
        .path()
        .join(unique_diff_filename("right", &second_label));
    fs::write(&first_path, &first.text)?;
    fs::write(&second_path, &second.text)?;
    Ok(DiffInputs {
        dir,
        first_path,
        second_path,
    })
}

fn refresh_active_diff_for_uri(state: &State, changed_uri: &Uri) -> io::Result<()> {
    let Some(active_diff) = &state.active_diff else {
        return Ok(());
    };

    if changed_uri != &active_diff.first_uri && changed_uri != &active_diff.second_uri {
        return Ok(());
    }

    let Some(first) = state.docs.get(&active_diff.first_uri) else {
        return Ok(());
    };
    let Some(second) = state.docs.get(&active_diff.second_uri) else {
        return Ok(());
    };

    fs::write(&active_diff.first_path, &first.text)?;
    fs::write(&active_diff.second_path, &second.text)?;
    Ok(())
}

fn open_diff_in_zed(first_path: &Path, second_path: &Path) -> io::Result<()> {
    if let Ok(cli) = std::env::var("ZED_DIFF_TOOL_ZED") {
        return spawn_diff(cli, first_path, second_path);
    }

    for cli in zed_cli_candidates() {
        if spawn_diff(cli, first_path, second_path).is_ok() {
            return Ok(());
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find a Zed CLI for opening a diff",
    ))
}

fn spawn_diff(cli: impl AsRef<Path>, first_path: &Path, second_path: &Path) -> io::Result<()> {
    let status = ProcessCommand::new(cli.as_ref())
        .arg("--diff")
        .arg(first_path)
        .arg(second_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} exited with {status}",
            cli.as_ref().display()
        )))
    }
}

fn zed_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("zed"),
        PathBuf::from("zeditor"),
        PathBuf::from("zed-preview"),
    ];

    if cfg!(target_os = "macos") {
        candidates.extend([
            PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli"),
            PathBuf::from("/Applications/Zed Preview.app/Contents/MacOS/cli"),
        ]);
    }

    if cfg!(target_os = "windows") {
        candidates.extend([PathBuf::from("zed.exe"), PathBuf::from("zed-preview.exe")]);
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data);
            candidates.extend([
                base.join("Programs/Zed/zed.exe"),
                base.join("Programs/Zed Preview/zed.exe"),
            ]);
        }
    }

    candidates
}

fn display_name(uri: &Uri, fallback: &str) -> String {
    url::Url::parse(uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn sanitize_filename(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    sanitized.trim_matches('-').chars().take(48).collect()
}

fn unique_diff_filename(side: &str, label: &str) -> String {
    let label = sanitize_filename(label);
    if label.is_empty() {
        side.to_string()
    } else {
        format!("{side}-{label}")
    }
}

trait LspMethod {
    const METHOD: &'static str;
}

impl LspMethod for DidOpenTextDocument {
    const METHOD: &'static str = "textDocument/didOpen";
}

impl LspMethod for DidChangeTextDocument {
    const METHOD: &'static str = "textDocument/didChange";
}

impl LspMethod for DidCloseTextDocument {
    const METHOD: &'static str = "textDocument/didClose";
}

impl LspMethod for CodeActionRequest {
    const METHOD: &'static str = "textDocument/codeAction";
}

impl LspMethod for ExecuteCommand {
    const METHOD: &'static str = "workspace/executeCommand";
}

impl LspMethod for Shutdown {
    const METHOD: &'static str = "shutdown";
}
