use std::{io, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::{task, time};

use super::ProjectsConfig;
use super::routes::ApiErrorResponse;
use crate::core::projects::{
    FileOperation, ProjectEntry, modify_project_file, project_tree, read_project_file,
};

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(super) struct FilePath {
    /// Project-relative path using forward slashes; no empty, hidden, or traversal components.
    #[param(example = "raws/001.txt")]
    path: String,
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub(super) struct FileContent {
    /// UTF-8 file contents. An empty string is valid.
    #[schema(example = "Chapter text")]
    content: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(super) struct MoveRequest {
    /// New project-relative path. Parent directory must exist; destination must not exist.
    #[schema(example = "raws/002.txt")]
    destination: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CreateRequest {
    File {
        path: String,
        #[serde(default)]
        content: String,
    },
    Directory {
        path: String,
    },
}

/// Read a UTF-8 file.
///
/// Hidden entries and symlinks are inaccessible. Responses use Cache-Control: no-store.
#[utoipa::path(get, path = "/api/v1/projects/{project}/file", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name"), FilePath),
    responses(
        (status = 200, description = "UTF-8 file contents", body = FileContent),
        (status = 400, description = "Invalid path, not a file, or invalid UTF-8", body = ApiErrorResponse),
        (status = 403, description = "Path is inaccessible", body = ApiErrorResponse),
        (status = 404, description = "Project or file not found", body = ApiErrorResponse),
        (status = 500, description = "File could not be read", body = ApiErrorResponse)
    ))]
pub(super) async fn read(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
    Query(path): Query<FilePath>,
) -> Response {
    let Some(config) = config else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let result =
        task::spawn_blocking(move || read_project_file(&config.directory, &project, &path.path))
            .await;

    let mut response = match result {
        Ok(Ok(content)) => Json(FileContent { content }).into_response(),

        Ok(Err(error)) => file_error(error).into_response(),

        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());

    response
}

/// Replace an existing file's contents.
///
/// Overwrites the complete file with UTF-8 text. The file must already exist.
#[utoipa::path(put, path = "/api/v1/projects/{project}/file", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name"), FilePath),
    request_body = FileContent,
    responses(
        (status = 204, description = "File updated; no response body"),
        (status = 400, description = "Invalid path, not a file, or malformed JSON", body = ApiErrorResponse),
        (status = 403, description = "Path is inaccessible", body = ApiErrorResponse),
        (status = 404, description = "Project or file not found", body = ApiErrorResponse),
        (status = 413, description = "Request body exceeds 16 MiB", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json", body = ApiErrorResponse),
        (status = 422, description = "JSON does not match the request schema", body = ApiErrorResponse),
        (status = 500, description = "File could not be written", body = ApiErrorResponse)
    ))]
pub(super) async fn write(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
    Query(path): Query<FilePath>,
    Json(body): Json<FileContent>,
) -> StatusCode {
    modify(
        config,
        project,
        path.path,
        FileOperation::Write(body.content),
        StatusCode::NO_CONTENT,
    )
    .await
}

/// Move or rename a file or directory within a project.
///
/// Destination must not exist and its parent must exist. Moving a directory into itself is rejected.
#[utoipa::path(patch, path = "/api/v1/projects/{project}/file", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name"), FilePath),
    request_body = MoveRequest,
    responses(
        (status = 204, description = "Entry moved; no response body"),
        (status = 400, description = "Invalid path, move into itself, or malformed JSON", body = ApiErrorResponse),
        (status = 403, description = "Path is inaccessible", body = ApiErrorResponse),
        (status = 404, description = "Project, source, or destination parent not found", body = ApiErrorResponse),
        (status = 409, description = "Destination already exists", body = ApiErrorResponse),
        (status = 413, description = "Request body exceeds 16 MiB", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json", body = ApiErrorResponse),
        (status = 422, description = "JSON does not match the request schema", body = ApiErrorResponse),
        (status = 500, description = "Entry could not be moved", body = ApiErrorResponse)
    ))]
pub(super) async fn move_entry(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
    Query(path): Query<FilePath>,
    Json(body): Json<MoveRequest>,
) -> StatusCode {
    modify(
        config,
        project,
        path.path,
        FileOperation::Move(body.destination),
        StatusCode::NO_CONTENT,
    )
    .await
}

/// Create a file or directory.
///
/// Use kind="file" with path and optional content (defaults to empty), or kind="directory" with path.
/// Parent directories must already exist. Existing entries are never overwritten.
#[utoipa::path(post, path = "/api/v1/projects/{project}/files", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name")),
    request_body = CreateRequest,
    responses(
        (status = 201, description = "Entry created; no response body"),
        (status = 400, description = "Invalid path or malformed JSON", body = ApiErrorResponse),
        (status = 403, description = "Path is inaccessible", body = ApiErrorResponse),
        (status = 404, description = "Project or parent directory not found", body = ApiErrorResponse),
        (status = 409, description = "Entry already exists", body = ApiErrorResponse),
        (status = 413, description = "Request body exceeds 16 MiB", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json", body = ApiErrorResponse),
        (status = 422, description = "JSON does not match the request schema", body = ApiErrorResponse),
        (status = 500, description = "Entry could not be created", body = ApiErrorResponse)
    ))]
pub(super) async fn create(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
    Json(body): Json<CreateRequest>,
) -> StatusCode {
    let (path, operation) = match body {
        CreateRequest::File { path, content } => (path, FileOperation::CreateFile(content)),
        CreateRequest::Directory { path } => (path, FileOperation::CreateDirectory),
    };
    modify(config, project, path, operation, StatusCode::CREATED).await
}

async fn modify(
    config: Option<Arc<ProjectsConfig>>,
    project: String,
    path: String,
    operation: FileOperation,
    success: StatusCode,
) -> StatusCode {
    let Some(config) = config else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    match task::spawn_blocking(move || {
        modify_project_file(&config.directory, &project, &path, operation)
    })
    .await
    {
        Ok(Ok(())) => success,
        Ok(Err(error)) => file_error(error),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn file_error(error: io::Error) -> StatusCode {
    match error.kind() {
        io::ErrorKind::InvalidInput
        | io::ErrorKind::InvalidData
        | io::ErrorKind::NotADirectory
        | io::ErrorKind::IsADirectory => StatusCode::BAD_REQUEST,
        io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        io::ErrorKind::AlreadyExists => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// List the project's file tree.
///
/// Returns a complete sorted snapshot with project-relative paths, excluding hidden entries and
/// symlinks. Empty directories are included. Responses use Cache-Control: no-store.
#[utoipa::path(get, path = "/api/v1/projects/{project}/files", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name")),
    responses(
        (status = 200, description = "Complete file tree", body = Snapshot),
        (status = 400, description = "Invalid project name", body = ApiErrorResponse),
        (status = 403, description = "Permission denied", body = ApiErrorResponse),
        (status = 404, description = "Project not found", body = ApiErrorResponse),
        (status = 500, description = "Project could not be scanned", body = ApiErrorResponse)
    ))]
pub(super) async fn list(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
) -> Response {
    let result = match config {
        Some(config) => scan(config, project).await,
        None => Err(StatusCode::SERVICE_UNAVAILABLE),
    };

    let mut response = match result {
        Ok(entries) => Json(Snapshot {
            r#type: "snapshot",
            entries: &entries,
        })
        .into_response(),
        Err(status) => status.into_response(),
    };

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());

    response
}

/// Watch the project's file tree over WebSocket.
///
/// Connect using ws:// (wss:// behind TLS). Sends a JSON Snapshot immediately, then polls every
/// second and sends complete snapshots when the tree changes. Content-only edits do not change
/// the tree. Send {"type":"refresh"} to force a snapshot; other commands are ignored.
/// Client messages and frames are limited to 4096 bytes. If scanning fails, sends
/// {"type":"error","message":"Project is no longer available or could not be read."} and closes.
/// Scalar's HTTP client cannot exercise this streaming protocol; use a WebSocket client.
#[utoipa::path(get, path = "/api/v1/projects/{project}/files/ws", tag = "Project files",
    params(("project" = String, Path, description = "Visible project directory name")),
    responses(
        (status = 101, description = "WebSocket upgrade; subsequent text messages contain Snapshot JSON"),
        (status = 400, description = "Invalid project name or WebSocket handshake", body = ApiErrorResponse),
        (status = 403, description = "Permission denied", body = ApiErrorResponse),
        (status = 404, description = "Project not found", body = ApiErrorResponse),
        (status = 426, description = "WebSocket upgrade required", body = ApiErrorResponse),
        (status = 500, description = "Project could not be scanned", body = ApiErrorResponse)
    ))]
pub(super) async fn watch(
    State(config): State<Option<Arc<ProjectsConfig>>>,
    Path(project): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(config) = config else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    match scan(config.clone(), project.clone()).await {
        Ok(entries) => ws
            .max_message_size(4096)
            .max_frame_size(4096)
            .on_upgrade(move |socket| stream(socket, config, project, entries)),

        Err(status) => status.into_response(),
    }
}

async fn scan(
    config: Arc<ProjectsConfig>,
    project: String,
) -> Result<Vec<ProjectEntry>, StatusCode> {
    match task::spawn_blocking(move || project_tree(&config.directory, &project)).await {
        Ok(Ok(entries)) => Ok(entries),

        Ok(Err(error)) => Err(file_error(error)),

        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
struct Snapshot<'a> {
    /// Always "snapshot".
    #[schema(value_type = String, pattern = "^snapshot$", example = "snapshot")]
    r#type: &'static str,
    entries: &'a [ProjectEntry],
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Command {
    Refresh,
}

async fn send(socket: &mut WebSocket, text: String) -> bool {
    matches!(
        time::timeout(
            Duration::from_secs(10),
            socket.send(Message::Text(text.into()))
        )
        .await,
        Ok(Ok(()))
    )
}

async fn stream(
    mut socket: WebSocket,
    config: Arc<ProjectsConfig>,
    project: String,
    mut entries: Vec<ProjectEntry>,
) {
    // one full scan per connection per second (plus refresh requests); use native notifications
    // and shared scans if large trees or transient changes need to be tracked.
    let mut ticks = time::interval(Duration::from_secs(1));

    ticks.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    ticks.tick().await;

    loop {
        let Ok(text) = simd_json::to_string(&Snapshot {
            r#type: "snapshot",
            entries: &entries,
        }) else {
            return;
        };

        if !send(&mut socket, text).await {
            return;
        }

        loop {
            let refresh = tokio::select! {
                message = socket.recv() => match message {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,

                    Some(Ok(Message::Text(text))) => {
                        let mut bytes = text.as_bytes().to_vec();
                        match simd_json::from_slice::<Command>(&mut bytes) {
                            Ok(Command::Refresh) => true,
                            Err(_) => continue,
                        }
                    },

                    _ => continue, // Axum responds to WebSocket ping frames automatically.
                },

                _ = ticks.tick() => false,
            };

            match scan(config.clone(), project.clone()).await {
                Ok(next) if refresh || next != entries => {
                    entries = next;
                    break;
                }

                Ok(_) => {}

                Err(_) => {
                    send(&mut socket, r#"{"type":"error","message":"Project is no longer available or could not be read."}"#.into()).await;
                    let _ =
                        time::timeout(Duration::from_secs(1), socket.send(Message::Close(None)))
                            .await;
                    return;
                }
            }
        }
    }
}
