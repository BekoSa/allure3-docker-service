use anyhow::Context;
use std::path::Path;
use tokio::{fs, process::Command};
use tracing::{debug, error, info};

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...[truncated {} bytes]", &s[..max], s.len().saturating_sub(max))
    }
}

pub async fn generate_report(
    allure_bin: &str,
    results_dir: &Path, // .../runs/<id>/allure-results
    report_dir: &Path,  // .../runs/<id>/report
) -> anyhow::Result<()> {
    if !results_dir.exists() {
        anyhow::bail!("results_dir does not exist: {}", results_dir.display());
    }
    if !results_dir.is_dir() {
        anyhow::bail!("results_dir is not a directory: {}", results_dir.display());
    }

    // run_dir = .../runs/<id>
    let run_dir = results_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot get run_dir from results_dir: {}", results_dir.display()))?;

    fs::create_dir_all(report_dir)
        .await
        .with_context(|| format!("create report dir: {}", report_dir.display()))?;

    // count files in results (частая причина проблем — пустые/не те данные)
    let mut file_count: usize = 0;
    if let Ok(mut rd) = fs::read_dir(results_dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Ok(ft) = ent.file_type().await {
                if ft.is_file() {
                    file_count += 1;
                }
            }
        }
    }

    info!(
        allure_bin = %allure_bin,
        results_dir = %results_dir.display(),
        report_dir = %report_dir.display(),
        results_files = file_count,
        "running allure generate (Allure 3 CLI)"
    );

    // ✅ Allure 3 CLI syntax:
    // allure generate --cwd <dir> --output <report_dir> "<pattern>"
    // Default pattern: ./**/allure-results
    // Мы задаём cwd = run_dir и pattern на allure-results
    let mut cmd = Command::new(allure_bin);
    cmd.arg("generate")
        .arg("--cwd")
        .arg(run_dir)
        .arg("--output")
        .arg(report_dir)
        .arg("./**/allure-results");

    debug!(command = ?cmd, "spawn allure command");

    let out = cmd
        .output()
        .await
        .with_context(|| format!("spawn allure generate: {}", allure_bin))?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);

        error!(
            exit_code = code,
            stdout = %clip(&stdout, 20_000),
            stderr = %clip(&stderr, 20_000),
            "allure generate failed"
        );

        anyhow::bail!(
            "allure generate failed (exit_code={}) stdout={} stderr={}",
            code,
            clip(&stdout, 4000),
            clip(&stderr, 4000)
        );
    }

    debug!(
        stdout = %clip(&stdout, 2000),
        stderr = %clip(&stderr, 2000),
        "allure generate succeeded"
    );

    Ok(())
}
