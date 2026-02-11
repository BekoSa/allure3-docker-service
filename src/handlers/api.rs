use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

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
pub struct ProjectsSummaryResp {
    pub total_projects: usize,
    pub total_runs: usize,
    pub projects: Vec<storage::ProjectSummary>,
}

#[derive(Serialize)]
pub struct DeleteResp {
    pub deleted: bool,
    pub project: String,
}

#[derive(Serialize)]
pub struct RegenerateResp {
    pub project: String,
    pub run_id: u64,
    pub status: String,        // "success" | "failed"
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct RunsResp {
    pub project: String,
    pub runs: Vec<RunItem>,
}

#[derive(Serialize)]
pub struct RunItem {
    pub run_id: u64,
    pub status: Option<String>, // success/failed/None
    pub error: Option<String>,
    pub ui_url: String,
}

pub async fn list_projects_summary(State(state): State<AppState>) -> impl IntoResponse {
    let summaries = match storage::list_project_summaries(&state.data_dir).await {
        Ok(x) => x,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("list summaries: {e}"),
            )
                .into_response()
        }
    };

    let total_projects = summaries.len();
    let total_runs = summaries.iter().map(|p| p.runs_count).sum::<usize>();

    (
        StatusCode::OK,
        Json(ProjectsSummaryResp {
            total_projects,
            total_runs,
            projects: summaries,
        }),
    )
        .into_response()
}

pub async fn list_runs(
    State(state): State<AppState>,
    Path(project_raw): Path<String>,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };

    // list ids
    let mut ids = match storage::list_run_ids(&state.data_dir, &project).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("list runs: {e}")).into_response(),
    };

    // newest first
    ids.sort_unstable_by(|a, b| b.cmp(a));

    let mut runs = Vec::with_capacity(ids.len());
    for id in ids {
        let rdir = storage::run_dir(&state.data_dir, &project, id);
        let st = storage::read_run_status(&rdir).await;

        runs.push(RunItem {
            run_id: id,
            status: st.as_ref().map(|x| x.status.clone()),
            error: st.and_then(|x| x.error),
            ui_url: format!("/ui/{}/runs/{}/", project, id),
        });
    }

    (StatusCode::OK, Json(RunsResp { project, runs })).into_response()
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(project_raw): Path<String>,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };

    let lock = state.project_lock(&project);
    let _guard = lock.lock().await;

    if let Err(e) = storage::delete_project(&state.data_dir, &project).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("delete project: {e}")).into_response();
    }

    (StatusCode::OK, Json(DeleteResp { deleted: true, project })).into_response()
}

pub async fn regenerate_run(
    State(state): State<AppState>,
    Path((project_raw, run_id)): Path<(String, u64)>,
) -> impl IntoResponse {
    let project = match sanitize_name(&project_raw) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid project").into_response(),
    };

    let lock = state.project_lock(&project);
    let _guard = lock.lock().await;

    let run_dir = storage::run_dir(&state.data_dir, &project, run_id);
    let results_dir = run_dir.join("allure-results");
    let report_dir = run_dir.join("report");

    let _ = tokio::fs::remove_dir_all(&report_dir).await;

    match allure::generate_report(&state.allure_bin, &results_dir, &report_dir).await {
        Ok(()) => {
            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &storage::RunStatus { status: "success".into(), error: None },
            )
                .await;

            let pdir = storage::project_dir(&state.data_dir, &project);
            let latest = storage::read_latest_run_id(&pdir).await;
            if latest.is_none() || latest == Some(run_id) {
                let _ = storage::set_latest_run_id(&pdir, run_id).await;
            }

            (StatusCode::OK, Json(RegenerateResp {
                project,
                run_id,
                status: "success".into(),
                error: None,
            })).into_response()
        }
        Err(e) => {
            let err_text = e.to_string();
            error!(project=%project, run_id=run_id, error=%err_text, "regenerate failed");

            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &storage::RunStatus { status: "failed".into(), error: Some(err_text.clone()) },
            )
                .await;

            (StatusCode::INTERNAL_SERVER_ERROR, Json(RegenerateResp {
                project,
                run_id,
                status: "failed".into(),
                error: Some(err_text),
            })).into_response()
        }
    }
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
    let results_dir = run_dir.join("allure-results");
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

    if let Err(e) = storage::write_json(&run_dir.join("meta.json"), &meta).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("write meta.json: {e}")).into_response();
    }

    let limits = UnzipLimits::default();
    if let Err(e) = unzip::unzip_safely(zip_bytes, results_dir.clone(), limits).await {
        warn!(project=%project, run_id=run_id, error=%e, "failed to unzip results");

        let _ = storage::write_json(
            &run_dir.join("status.json"),
            &storage::RunStatus { status: "failed".into(), error: Some(format!("bad zip: {e}")) },
        )
            .await;

        return (StatusCode::BAD_REQUEST, format!("bad zip: {e}")).into_response();
    }

    match allure::generate_report(&state.allure_bin, &results_dir, &report_dir).await {
        Ok(()) => {
            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &storage::RunStatus { status: "success".into(), error: None },
            )
                .await;

            if let Err(e) = storage::set_latest_run_id(&project_dir, run_id).await {
                warn!(project=%project, run_id=run_id, error=%e, "set latest_run_id failed");
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
            error!(project=%project, run_id=run_id, error=%err_text, "report generation failed");

            let _ = storage::write_json(
                &run_dir.join("status.json"),
                &storage::RunStatus { status: "failed".into(), error: Some(err_text.clone()) },
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
