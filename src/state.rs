use dashmap::DashMap;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub data_dir: PathBuf,
    pub allure_bin: String,
    /// Lock per project to avoid race on run_id and latest.
    pub project_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, allure_bin: String) -> Self {
        Self {
            data_dir,
            allure_bin,
            project_locks: Arc::new(DashMap::new()),
        }
    }

    pub fn project_lock(&self, project: &str) -> Arc<Mutex<()>> {
        self.project_locks
            .entry(project.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}
