use std::{io, sync::Arc, time::Duration};

use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tokio::{task, time};

use super::ProjectsConfig;
use crate::core::projects::{ProjectEntry, project_tree};

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
    // one full scan per connection per second; use native notifications
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
            tokio::select! {
                message = socket.recv() => match message {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                    _ => {} // Axum responds to WebSocket ping frames automatically.
                },
                _ = ticks.tick() => {
                    match scan(config.clone(), project.clone()).await {
                        Ok(next) if next != entries => {
                            entries = next;
                            break;
                        }

                        Ok(_) => {},

                        Err(_) => {
                            send(&mut socket, r#"{"type":"error","message":"Project is no longer available or could not be read."}"#.into()).await;
                            let _ = time::timeout(Duration::from_secs(1), socket.send(Message::Close(None))).await;
                            return;
                        }
                    }
                }
            }
        }
    }
}
