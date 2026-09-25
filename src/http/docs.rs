use utoipa::OpenApi;

use super::{project_files, routes};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Glosswork API",
        description = "Match Chinese glossary terms, prepare translation prompts, and manage local translation projects.\n\nJSON request bodies require Content-Type: application/json and are limited to 16 MiB. Errors use {\"error\":{\"code\":\"...\",\"message\":\"...\"}}. Unknown routes return 404 and unsupported methods return 405.\n\nProject operations are available only when HTTP_PROJECTS_DIR is configured; otherwise those routes return 404 and are omitted from this document. Paths are relative to a project and use forward slashes. Absolute paths, traversal, hidden entries, and symlinks are not supported.\n\nThe server has no authentication and defaults to 127.0.0.1:8000."
    ),
    paths(
        routes::health,
        routes::match_glossary,
        routes::prepare_translation_prompt,
        routes::discover_projects,
        project_files::list,
        project_files::read,
        project_files::create,
        project_files::write,
        project_files::move_entry,
        project_files::watch
    ),
    tags(
        (name = "Health", description = "Server status and version"),
        (name = "Glossary", description = "Exact and fuzzy glossary matching"),
        (name = "Prompts", description = "Translation prompt preparation"),
        (name = "Projects", description = "Project discovery; requires HTTP_PROJECTS_DIR"),
        (name = "Project files", description = "File operations and live tree snapshots; requires HTTP_PROJECTS_DIR")
    )
)]
struct ApiDoc;

pub(super) fn openapi(projects_enabled: bool) -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    if !projects_enabled {
        document
            .paths
            .paths
            .retain(|path, _| !path.starts_with("/api/v1/projects"));
    }
    document
}
