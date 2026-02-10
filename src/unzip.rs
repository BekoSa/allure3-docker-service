use anyhow::Context;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub struct UnzipLimits {
    /// Max number of files in zip.
    pub max_files: usize,
    /// Max total uncompressed bytes across all files.
    pub max_total_uncompressed: u64,
    /// Max size of a single extracted file.
    pub max_single_file: u64,
}

impl Default for UnzipLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_total_uncompressed: 2 * 1024 * 1024 * 1024, // 2 GiB
            max_single_file: 512 * 1024 * 1024,             // 512 MiB
        }
    }
}

/// Extract zip safely into dest_dir:
/// - rejects absolute paths
/// - rejects ".." path traversal
/// - limits number of files
/// - limits uncompressed sizes (per-file and total)
pub async fn unzip_safely(
    zip_bytes: Vec<u8>,
    dest_dir: PathBuf,
    limits: UnzipLimits,
) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || unzip_safely_blocking(&zip_bytes, &dest_dir, limits))
        .await
        .context("join unzip task")??;
    Ok(())
}

fn unzip_safely_blocking(zip_bytes: &[u8], dest_dir: &Path, limits: UnzipLimits) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest_dir).context("create dest dir")?;

    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).context("open zip")?;

    let mut total_uncompressed: u64 = 0;
    let mut files_count: usize = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("read entry")?;
        let name = file.name().to_string();

        files_count += 1;
        if files_count > limits.max_files {
            anyhow::bail!("zip has too many files (>{})", limits.max_files);
        }

        let is_dir = file.is_dir();

        let rel = sanitize_zip_entry_path(&name)
            .with_context(|| format!("bad zip entry path: {}", name))?;

        let out_path = dest_dir.join(&rel);

        if !is_within_dir(&out_path, dest_dir)? {
            anyhow::bail!("zip entry escapes destination: {}", name);
        }

        if is_dir {
            std::fs::create_dir_all(&out_path).with_context(|| format!("mkdir {:?}", out_path))?;
            continue;
        }

        let declared = file.size();
        if declared > limits.max_single_file {
            anyhow::bail!("zip entry too large: {} ({} bytes)", name, declared);
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("mkdir {:?}", parent))?;
        }

        let mut out = std::fs::File::create(&out_path).with_context(|| format!("create {:?}", out_path))?;

        let mut written: u64 = 0;
        let mut buf = [0u8; 64 * 1024];

        loop {
            let n = file.read(&mut buf).context("read zip entry")?;
            if n == 0 {
                break;
            }

            written = written.saturating_add(n as u64);
            if written > limits.max_single_file {
                anyhow::bail!("zip entry exceeds max_single_file: {}", name);
            }

            total_uncompressed = total_uncompressed.saturating_add(n as u64);
            if total_uncompressed > limits.max_total_uncompressed {
                anyhow::bail!("zip exceeds max_total_uncompressed");
            }

            out.write_all(&buf[..n]).context("write extracted file")?;
        }

        out.flush().ok();
    }

    Ok(())
}

fn sanitize_zip_entry_path(name: &str) -> anyhow::Result<PathBuf> {
    let name = name.replace('\\', "/");

    if name.starts_with('/') {
        anyhow::bail!("absolute path not allowed");
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        anyhow::bail!("drive letter path not allowed");
    }

    let mut out = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            anyhow::bail!("path traversal not allowed");
        }
        out.push(part);
    }

    if out.as_os_str().is_empty() {
        anyhow::bail!("empty entry path");
    }

    Ok(out)
}

fn is_within_dir(out_path: &Path, base: &Path) -> anyhow::Result<bool> {
    let out = normalize_lexical(out_path);
    let base = normalize_lexical(base);
    Ok(out.starts_with(&base))
}

fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        use std::path::Component;
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
