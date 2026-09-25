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
use crate::core::projects::{
    FileOperation, ProjectEntry, modify_project_file, project_tree, read_project_file,
};

#[derive(Deserialize)]
pub(super) struct FilePath {
    path: String,
}

#[derive(Deserialize, Serialize)]
pub(super) struct FileContent {
    content: String,
}

#[derive(Deserialize)]
pub(super) struct MoveRequest {
    destination: String,
}

#[derive(Deserialize)]
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

#[derive(Serialize)]
struct Snapshot<'a> {
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