use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use tower::ServiceExt;
use tower_http::services::ServeDir;

use crate::{state::AppState, storage, util::sanitize_name};

pub async fn ui_index(State(state): State<AppState>) -> impl IntoResponse {
    let projects = match storage::list_projects(&state.data_dir).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("list projects: {e}")).into_response(),
    };

    let mut items = String::new();
    for p in projects.into_iter().filter_map(|x| sanitize_name(&x)) {
        items.push_str(&format!(r#"<li><a href="/ui/{}/">{}</a></li>"#, p, p));
    }

    let html = format!(
        r#"<!doctype html>
<html lang="ru">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Allure Projects</title>
</head>
<body>
  <h1>Projects</h1>
  <ul>{}</ul>
</body>
</html>"#,
        items
    );

    Html(html).into_response()
}

pub async fn ui_project_home(Path(project_raw): Path<String>) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };
    Redirect::temporary(&format!("/ui/{}/latest/", project)).into_response()
}

pub async fn ui_latest(
    State(state): State<AppState>,
    Path(project_raw): Path<String>,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };

    let project_dir = storage::project_dir(&state.data_dir, &project);
    let run_id = match storage::read_latest_run_id(&project_dir).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "No runs yet").into_response(),
    };

    Redirect::temporary(&format!("/ui/{}/runs/{}/", project, run_id)).into_response()
}

/// Явный индекс для /runs/{run_id}/
/// Просто отдаём index.html из report dir через ServeDir (он сам найдёт index.html)
pub async fn ui_run_index(
    State(state): State<AppState>,
    Path((project_raw, run_id)): Path<(String, u64)>,
    req: Request<Body>,
) -> impl IntoResponse {
    serve_report_dir(state, project_raw, run_id, req).await
}

/// Статика для /runs/{run_id}/{*tail}
pub async fn ui_run_files(
    State(state): State<AppState>,
    Path((project_raw, run_id, _tail)): Path<(String, u64, String)>,
    req: Request<Body>,
) -> impl IntoResponse {
    serve_report_dir(state, project_raw, run_id, req).await
}

async fn serve_report_dir(
    state: AppState,
    project_raw: String,
    run_id: u64,
    req: Request<Body>,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };

    let report_dir = storage::run_dir(&state.data_dir, &project, run_id).join("report");
    let service = ServeDir::new(report_dir).append_index_html_on_directories(true);

    match service.oneshot(req).await {
        Ok(resp) => resp.into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
