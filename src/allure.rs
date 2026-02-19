use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;
use tracing::{debug, error, info};

use crate::state::AppState;

#[derive(Clone, Debug)]
pub enum AllureConfigChoice {
    /// Не использовать --config
    None,
    /// JS-конфиг текстом (запишем в run_dir/allure.config.mjs)
    InlineJs(String),
    // Путь к конфигу на диске (опционально, “на будущее”)
    Path(PathBuf),
}

#[derive(thiserror::Error, Debug)]
pub enum AllureError {
    #[error("allure binary not found: {0}")]
    BinaryNotFound(String),

    #[error("failed to write allure config: {0}")]
    WriteConfig(#[from] std::io::Error),

    #[error("allure generate failed (exit={exit_code:?}): {message}")]
    GenerateFailed {
        exit_code: Option<i32>,
        message: String,
    },
}

async fn write_config_file(run_dir: &Path, js: &str) -> Result<PathBuf, std::io::Error> {
    let p = run_dir.join("allure.config.mjs");
    tokio::fs::write(&p, js).await?;
    Ok(p)
}
pub async fn generate_report(
    state: &AppState,
    results_dir: &Path,
    report_dir: &Path,
    run_dir: &Path,
    config: AllureConfigChoice,
) -> Result<(), AllureError> {
    let allure_bin = state.allure_bin.as_str();

    let config_path: Option<PathBuf> = match config {
        AllureConfigChoice::None => None,
        AllureConfigChoice::Path(p) => Some(p),
        AllureConfigChoice::InlineJs(js) => Some(write_config_file(run_dir, &js).await?),
    };

    let mut cmd = Command::new(allure_bin);
    cmd.arg("generate")
        .arg(results_dir)
        .arg("-o")
        .arg(report_dir);

    if let Some(cfg) = &config_path {
        cmd.arg("--config").arg(cfg);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    info!(
        allure_bin = %allure_bin,
        results_dir = %results_dir.display(),
        report_dir = %report_dir.display(),
        config = %config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".into()),
        "running allure generate"
    );
    debug!(command = ?cmd, "spawn allure command");

    let out = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AllureError::BinaryNotFound(allure_bin.to_string())
        } else {
            AllureError::GenerateFailed {
                exit_code: None,
                message: format!("failed to spawn allure: {e}"),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if !out.status.success() {
        error!(
            exit_code = out.status.code(),
            stdout = %stdout,
            stderr = %stderr,
            "allure generate failed"
        );
        return Err(AllureError::GenerateFailed {
            exit_code: out.status.code(),
            message: format!(
                "stdout={}\nstderr={}",
                stdout.trim_end(),
                stderr.trim_end()
            ),
        });
    }

    Ok(())
}
