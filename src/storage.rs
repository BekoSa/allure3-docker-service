use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};

pub fn project_dir(data_dir: &Path, project: &str) -> PathBuf {
    data_dir.join("projects").join(project)
}

pub fn runs_dir(data_dir: &Path, project: &str) -> PathBuf {
    project_dir(data_dir, project).join("runs")
}

pub fn run_dir(data_dir: &Path, project: &str, run_id: u64) -> PathBuf {
    runs_dir(data_dir, project).join(run_id.to_string())
}

pub async fn ensure_project_dirs(data_dir: &Path, project: &str) -> anyhow::Result<()> {
    let runs = runs_dir(data_dir, project);
    fs::create_dir_all(&runs).await.context("create runs dir")?;
    Ok(())
}

pub async fn reserve_next_run_id(project_dir: &Path) -> anyhow::Result<u64> {
    let p = project_dir.join("next_run_id");

    let current: u64 = match fs::read_to_string(&p).await {
        Ok(s) => s.trim().parse().unwrap_or(1),
        Err(_) => 1,
    };

    let tmp = project_dir.join("next_run_id.tmp");
    let mut f = fs::File::create(&tmp).await?;
    f.write_all((current + 1).to_string().as_bytes()).await?;
    f.flush().await?;
    drop(f);
    fs::rename(&tmp, &p).await?;

    Ok(current)
}

pub async fn set_latest_run_id(project_dir: &Path, run_id: u64) -> anyhow::Result<()> {
    let p = project_dir.join("latest_run_id");
    let tmp = project_dir.join("latest_run_id.tmp");

    let mut f = fs::File::create(&tmp).await?;
    f.write_all(run_id.to_string().as_bytes()).await?;
    f.flush().await?;
    drop(f);

    fs::rename(&tmp, &p).await?;
    Ok(())
}

pub async fn read_latest_run_id(project_dir: &Path) -> Option<u64> {
    let p = project_dir.join("latest_run_id");
    let s = fs::read_to_string(&p).await.ok()?;
    s.trim().parse::<u64>().ok()
}

pub async fn write_json<T: Serialize>(path: &Path, v: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(v)?;
    let tmp = path.with_extension("tmp");

    let mut f = fs::File::create(&tmp).await?;
    f.write_all(&bytes).await?;
    f.flush().await?;
    drop(f);

    fs::rename(&tmp, path).await?;
    Ok(())
}

pub async fn list_projects(data_dir: &Path) -> anyhow::Result<Vec<String>> {
    let root = data_dir.join("projects");
    let mut out = Vec::new();

    let mut rd = match fs::read_dir(&root).await {
        Ok(r) => r,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(out);
            }
            return Err(e.into());
        }
    };

    while let Some(ent) = rd.next_entry().await? {
        let ft = ent.file_type().await?;
        if ft.is_dir() {
            if let Some(name) = ent.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }

    out.sort();
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatus {
    pub status: String,            // "success" | "failed"
    pub error: Option<String>,
}

pub async fn read_run_status(run_dir: &Path) -> Option<RunStatus> {
    let p = run_dir.join("status.json");
    let s = fs::read_to_string(&p).await.ok()?;
    serde_json::from_str::<RunStatus>(&s).ok()
}

pub async fn list_run_ids(data_dir: &Path, project: &str) -> anyhow::Result<Vec<u64>> {
    let runs_root = runs_dir(data_dir, project);
    let mut out = Vec::new();

    let mut rd = match fs::read_dir(&runs_root).await {
        Ok(r) => r,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(out);
            }
            return Err(e.into());
        }
    };

    while let Some(ent) = rd.next_entry().await? {
        let ft = ent.file_type().await?;
        if ft.is_dir() {
            if let Some(name) = ent.file_name().to_str() {
                if let Ok(id) = name.parse::<u64>() {
                    out.push(id);
                }
            }
        }
    }

    out.sort_unstable();
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub project: String,
    pub runs_count: usize,
    pub latest_run_id: Option<u64>,
    pub latest_status: Option<String>, // "success" | "failed"
    pub latest_error: Option<String>,
}

pub async fn project_summary(data_dir: &Path, project: &str) -> anyhow::Result<ProjectSummary> {
    let pdir = project_dir(data_dir, project);
    let latest = read_latest_run_id(&pdir).await;

    let run_ids = list_run_ids(data_dir, project).await?;
    let runs_count = run_ids.len();

    let (latest_status, latest_error) = if let Some(id) = latest {
        let rdir = run_dir(data_dir, project, id);
        if let Some(st) = read_run_status(&rdir).await {
            (Some(st.status), st.error)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    Ok(ProjectSummary {
        project: project.to_string(),
        runs_count,
        latest_run_id: latest,
        latest_status,
        latest_error,
    })
}

pub async fn list_project_summaries(data_dir: &Path) -> anyhow::Result<Vec<ProjectSummary>> {
    let projects = list_projects(data_dir).await?;
    let mut out = Vec::with_capacity(projects.len());
    for p in projects {
        out.push(project_summary(data_dir, &p).await?);
    }
    out.sort_by(|a, b| a.project.cmp(&b.project));
    Ok(out)
}

pub async fn delete_project(data_dir: &Path, project: &str) -> anyhow::Result<()> {
    let pdir = project_dir(data_dir, project);
    match fs::remove_dir_all(&pdir).await {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(());
            }
            Err(e.into())
        }
    }
}
