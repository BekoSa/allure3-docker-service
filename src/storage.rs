use anyhow::Context;
use serde::Serialize;
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

    // atomic write via temp + rename
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
