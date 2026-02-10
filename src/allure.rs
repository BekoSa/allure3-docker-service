use anyhow::Context;
use std::path::Path;
use tokio::{fs, process::Command};

pub async fn generate_report(
    allure_bin: &str,
    results_dir: &Path,
    report_dir: &Path,
) -> anyhow::Result<()> {
    fs::create_dir_all(report_dir)
        .await
        .context("create report dir")?;

    let out = Command::new(allure_bin)
        .arg("generate")
        .arg(results_dir)
        .arg("-o")
        .arg(report_dir)
        .arg("--clean")
        .output()
        .await
        .context("spawn allure generate")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("allure generate failed: {}", stderr);
    }

    Ok(())
}
