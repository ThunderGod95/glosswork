mod project_files;
mod routes;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};

use routes::{
    default_error, discover_projects, health, match_glossary, prepare_translation_prompt,
};

pub(super) struct ProjectsConfig {
    directory: PathBuf,
}

fn build(projects: Option<Arc<ProjectsConfig>>) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/api/v1/glossary/match", post(match_glossary))
        .route(
            "/api/v1/prompts/translation",
            post(prepare_translation_prompt),
        );

    if projects.is_some() {
        router = router
            .route("/api/v1/projects", get(discover_projects))
            .route(
                "/api/v1/projects/{project}/files",
                get(project_files::list).post(project_files::create),
            )
            .route(
                "/api/v1/projects/{project}/file",
                get(project_files::read)
                    .put(project_files::write)
                    .patch(project_files::move_entry),
            )
            .route(
                "/api/v1/projects/{project}/files/ws",
                get(project_files::watch),
            );
    }

    router
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(middleware::map_response(default_error))
        .with_state(projects)
}

pub fn launch() -> anyhow::Result<()> {
    let projects = match std::env::var_os("HTTP_PROJECTS_DIR") {
        None => None,

        Some(directory) => {
            anyhow::ensure!(!directory.is_empty(), "HTTP_PROJECTS_DIR must not be empty");

            let directory = PathBuf::from(directory).canonicalize()?;

            anyhow::ensure!(directory.is_dir(), "HTTP_PROJECTS_DIR must be a directory");

            Some(Arc::new(ProjectsConfig { directory }))
        }
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let address = match std::env::var("HTTP_BIND") {
                Ok(address) => address.parse::<std::net::SocketAddr>()?,
                Err(std::env::VarError::NotPresent) => "127.0.0.1:8000".parse()?,
                Err(error) => return Err(error.into()),
            };

            let listener = tokio::net::TcpListener::bind(address).await?;

            eprintln!("HTTP server listening on http://{}", listener.local_addr()?);

            axum::serve(listener, build(projects))
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            Ok(())
        })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
