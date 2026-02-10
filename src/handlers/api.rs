use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    allure,
    state::AppState,
    storage,
    unzip::{self, UnzipLimits},
    util::sanitize_name,
};

#[derive(Deserialize, Serialize, Default)]
pub struct Meta {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub trigger: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Serialize)]
pub struct UploadResp {
    pub project: String,
    pub run_id: u64,
    pub ui_url: String,
    pub latest_url: String,
    pub status: String,        // "success" | "failed"
    pub error: Option<String>, // error text if failed
}

#[derive(Serialize)]
pub struct ProjectItem {
    pub project: String,
    pub ui_url: String,
    pub latest_url: String,
}

#[derive(Serialize)]
pub struct ProjectsResp {
    pub projects: Vec<ProjectItem>,
}

#[derive(Serialize)]
struct RunStatus {
    status: String,
    error: Option<String>,
}

pub async fn list_projects(State(state): State<AppState>) -> impl IntoResponse {
    let projects = match storage::list_projects(&state.data_dir).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("list projects: {e}"),
            )
                .into_response()
        }
    };

    let items = projects
        .into_iter()
        .filter_map(|p| sanitize_name(&p))
        .map(|p| ProjectItem {
            ui_url: format!("/ui/{}/", p),
            latest_url: format!("/ui/{}/latest/", p),
            project: p,
        })
        .collect::<Vec<_>>();

    (StatusCode::OK, Json(ProjectsResp { projects: items })).into_response()
}

pub async fn upload_run(
    State(state): State<AppState>,
    Path(project_raw): Path<String>,
    mut mp: Multipart,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project name").into_response(),
    };

    let lock = state.project_lock(&project);
    let _guard = lock.lock().await;

    if let Err(e) = storage::ensure_project_dirs(&state.data_dir, &project).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ensure project dirs: {e}"),
        )
            .into_response();
    }
    let project_dir = storage::project_dir(&state.data_dir, &project);

    let run_id = match storage::reserve_next_run_id(&project_dir).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reserve next_run_id: {e}"),
            )
                .into_response()
        }
    };

    let run_dir = storage::run_dir(&state.data_dir, &project, run_id);
    let results_dir = run_dir.join("results");
    let report_dir = run_dir.join("report");

    if let Err(e) = tokio::fs::create_dir_all(&results_dir).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("mkdir results_dir: {e}"),
        )
            .into_response();
    }

    let mut zip_bytes: Option<Vec<u8>> = None;
    let mut meta: Meta = Meta::default();

    while let Ok(Some(field)) = mp.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "results" {
            match field.bytes().await {
                Ok(b) => zip_bytes = Some(b.to_vec()),
                Err(e) => return (StatusCode::BAD_REQUEST, format!("read results: {e}")).into_response(),
            }
        } else if name == "meta" {
            if let Ok(t) = field.text().await {
                if let Ok(m) = serde_json::from_str::<Meta>(&t) {
                    meta = m;
                }
            }
        }
    }

    let zip_bytes = match zip_bytes {
        Some(b) => b,
        None => return (StatusCode::BAD_REQUEST, "Missing multipart field 'results'").into_response(),
    };

    // meta.json
    if let Err(e) = storage::write_json(&run_dir.join("meta.json"), &meta).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("write meta.json: {e}")).into_response();
    }

    // unzip
    let limits = UnzipLimits::default();
    if let Err(e) = unzip::unzip_safely(zip_bytes, results_dir.clone(), limits).await {
        let _ = storage::write_json(
            &run_dir.join("status.json"),
            &RunStatus {
                status: "failed".into(),
                error: Some(format!("bad zip: {e}")),
            },
        )
            .await;

        return (StatusCode::BAD_REQUEST, format!("bad zip: {e}")).into_response();
    }

    // generate report
    match allure::generate_report(&state.allure_bin, &results_dir, &report_dir).await {
        Ok(()) => {
            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &RunStatus {
                    status: "success".into(),
                    error: None,
                },
            )
                .await;

            if let Err(e) = storage::set_latest_run_id(&project_dir, run_id).await {
                tracing::warn!("set latest_run_id failed: {}", e);
            }

            info!("uploaded run: project={} run_id={}", project, run_id);

            let resp = UploadResp {
                project: project.clone(),
                run_id,
                ui_url: format!("/ui/{}/runs/{}/", project, run_id),
                latest_url: format!("/ui/{}/latest/", project),
                status: "success".into(),
                error: None,
            };

            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            let err_text = e.to_string();
            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &RunStatus {
                    status: "failed".into(),
                    error: Some(err_text.clone()),
                },
            )
                .await;

            let resp = UploadResp {
                project: project.clone(),
                run_id,
                ui_url: format!("/ui/{}/runs/{}/", project, run_id),
                latest_url: format!("/ui/{}/latest/", project),
                status: "failed".into(),
                error: Some(err_text),
            };

            (StatusCode::INTERNAL_SERVER_ERROR, Json(resp)).into_response()
        }
    }
}
