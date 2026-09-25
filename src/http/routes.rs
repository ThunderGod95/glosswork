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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(super) struct MatchGlossaryRequest {
    /// Text to search; must contain non-whitespace characters.
    #[schema(example = "李白走进了房间。", min_length = 1)]
    pub content: String,
    pub glossary: Vec<GlossaryEntry>,

    #[serde(default)]
    pub options: GlossaryOptions,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(super) struct MatchGlossaryResponse {
    /// Matching entries in the order supplied by the caller.
    pub micro_glossary: Vec<GlossaryEntry>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(super) struct HealthResponse {
    #[schema(example = "ok")]
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(super) struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(super) struct ApiError {
    /// Machine-readable error identifier.
    pub code: String,
    pub message: String,
}

/// Check server health.
#[utoipa::path(get, path = "/health", tag = "Health",
    responses((status = 200, description = "Server is running", body = HealthResponse)))]
pub(super) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize, utoipa::ToSchema)]
struct ProjectsResponse {
    projects: Vec<String>,
}

/// List available projects.
///
/// Returns sorted visible directory names. Hidden directories and symlinks are excluded.
/// Requires HTTP_PROJECTS_DIR. Responses use Cache-Control: no-store.
#[utoipa::path(get, path = "/api/v1/projects", tag = "Projects",
    responses(
        (status = 200, description = "Project names", body = ProjectsResponse),
        (status = 500, description = "Directory could not be read or contains no projects", body = ApiErrorResponse)
    ))]
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

/// Match glossary entries against text.
///
/// Performs exact matching followed by optional fuzzy matching. An empty glossary is valid.
/// Options default to fuzzy=true and fuzzy_threshold=1; the maximum threshold is 4.
#[utoipa::path(post, path = "/api/v1/glossary/match", tag = "Glossary",
    request_body = MatchGlossaryRequest,
    responses(
        (status = 200, description = "Matching glossary entries", body = MatchGlossaryResponse),
        (status = 400, description = "Malformed JSON, blank content, or invalid_fuzzy_threshold", body = ApiErrorResponse),
        (status = 413, description = "Request body exceeds 16 MiB", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json", body = ApiErrorResponse),
        (status = 422, description = "JSON does not match the request schema", body = ApiErrorResponse),
        (status = 500, description = "Glossary processing failed", body = ApiErrorResponse)
    ))]
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

/// Build a translation prompt.
///
/// Combines translation instructions, matching glossary entries, and chapter text.
/// Does not call an LLM or write files. Both chapter and translation_prompt must be nonblank.
/// Options default to fuzzy=true and fuzzy_threshold=1; the maximum threshold is 4.
#[utoipa::path(post, path = "/api/v1/prompts/translation", tag = "Prompts",
    request_body = PreparePromptRequest,
    responses(
        (status = 200, description = "Prepared prompt and matching glossary", body = PreparePromptResult),
        (status = 400, description = "Malformed JSON, empty_field, or invalid_fuzzy_threshold", body = ApiErrorResponse),
        (status = 413, description = "Request body exceeds 16 MiB", body = ApiErrorResponse),
        (status = 415, description = "Content-Type must be application/json", body = ApiErrorResponse),
        (status = 422, description = "JSON does not match the request schema", body = ApiErrorResponse),
        (status = 500, description = "Prompt preparation failed", body = ApiErrorResponse)
    ))]
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
