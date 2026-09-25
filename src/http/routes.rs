use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::task;

use super::ProjectsConfig;

use crate::core::{
    glossary::{GlossaryEntry, GlossaryOptions, create_micro_glossary_with_options},
    projects::get_projects,
    prompt::{
        PreparePromptRequest, PreparePromptResult, prepare_translation_prompt as prepare_prompt,
    },
};

const MAX_FUZZY_THRESHOLD: u32 = 4;

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiErrorResponse>)>;

#[derive(Debug, Deserialize)]
pub(super) struct MatchGlossaryRequest {
    pub content: String,
    pub glossary: Vec<GlossaryEntry>,

    #[serde(default)]
    pub options: GlossaryOptions,
}

#[derive(Debug, Serialize)]
pub(super) struct MatchGlossaryResponse {
    pub micro_glossary: Vec<GlossaryEntry>,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiError {
    pub code: String,
    pub message: String,
}

pub(super) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct ProjectsResponse {
    projects: Vec<String>,
}

pub(super) async fn discover_projects(
    State(config): State<Option<Arc<ProjectsConfig>>>,
) -> Response {
    let Some(config) = config else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let result = task::spawn_blocking(move || get_projects(&config.directory)).await;

    let mut response = match result {
        Ok(Ok(projects)) => Json(ProjectsResponse { projects }).into_response(),
        Ok(Err(error)) => core_error(error).into_response(),
        Err(error) => join_error(error).into_response(),
    };

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());

    response
}

pub(super) async fn match_glossary(
    Json(request): Json<MatchGlossaryRequest>,
) -> ApiResult<MatchGlossaryResponse> {
    validate_content(&request.content, "content")?;
    validate_options(request.options)?;

    let result = task::spawn_blocking(move || {
        create_micro_glossary_with_options(&request.content, &request.glossary, request.options)
    })
    .await
    .map_err(join_error)?
    .map_err(core_error)?;

    Ok(Json(MatchGlossaryResponse {
        micro_glossary: result,
    }))
}

pub(super) async fn prepare_translation_prompt(
    Json(request): Json<PreparePromptRequest>,
) -> ApiResult<PreparePromptResult> {
    validate_content(&request.chapter, "chapter")?;
    validate_content(&request.translation_prompt, "translation_prompt")?;
    validate_options(request.glossary_options)?;

    let result = task::spawn_blocking(move || prepare_prompt(request))
        .await
        .map_err(join_error)?
        .map_err(core_error)?;

    Ok(Json(result))
}

pub(super) async fn default_error(mut response: Response) -> Response {
    let status = response.status();

    if (status.is_client_error() || status.is_server_error())
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            != Some("application/json")
    {
        let reason = status.canonical_reason().unwrap_or("Request failed");

        let json = Json(ApiErrorResponse {
            error: ApiError {
                code: reason.to_lowercase().replace(' ', "_"),
                message: reason.to_string(),
            },
        })
        .into_response();

        response.headers_mut().remove(header::CONTENT_LENGTH);
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            json.headers()[header::CONTENT_TYPE].clone(),
        );

        *response.body_mut() = json.into_body();
    }

    response
}

fn validate_content(
    value: &str,
    field: &'static str,
) -> Result<(), (StatusCode, Json<ApiErrorResponse>)> {
    if value.trim().is_empty() {
        return Err(bad_request(
            "empty_field",
            format!("'{field}' must not be empty."),
        ));
    }

    Ok(())
}

fn validate_options(options: GlossaryOptions) -> Result<(), (StatusCode, Json<ApiErrorResponse>)> {
    if options.fuzzy_threshold > MAX_FUZZY_THRESHOLD {
        return Err(bad_request(
            "invalid_fuzzy_threshold",
            format!("'fuzzy_threshold' must not exceed {MAX_FUZZY_THRESHOLD}."),
        ));
    }

    Ok(())
}

fn bad_request(
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiErrorResponse {
            error: ApiError {
                code: code.into(),
                message: message.into(),
            },
        }),
    )
}

fn core_error(error: anyhow::Error) -> (StatusCode, Json<ApiErrorResponse>) {
    eprintln!("Core processing error: {error:#}");

    internal_error()
}

fn join_error(error: task::JoinError) -> (StatusCode, Json<ApiErrorResponse>) {
    eprintln!("Blocking worker failed: {error}");

    internal_error()
}

fn internal_error() -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiErrorResponse {
            error: ApiError {
                code: "internal_error".to_string(),
                message: "Failed to process request.".to_string(),
            },
        }),
    )
}
