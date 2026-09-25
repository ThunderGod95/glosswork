mod routes;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};

use routes::{default_error, health, match_glossary, prepare_translation_prompt};

pub fn build() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/glossary/match", post(match_glossary))
        .route(
            "/api/v1/prompts/translation",
            post(prepare_translation_prompt),
        )
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(middleware::map_response(default_error))
}

pub fn launch() -> anyhow::Result<()> {
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

            axum::serve(listener, build())
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
