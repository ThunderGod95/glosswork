use std::{io, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::{task, time};

use super::ProjectsConfig;
use crate::core::projects::{ProjectEntry, project_tree};

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

        Ok(Err(error)) => Err(match error.kind() {
            io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }),

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
