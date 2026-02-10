mod app;
mod state;
mod util;
mod storage;
mod unzip;
mod allure;
mod handlers;

use crate::state::AppState;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, debug};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("starting allure3-docker-service");


    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let listen = std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let allure_bin = std::env::var("ALLURE_BIN").unwrap_or_else(|_| "allure".to_string());

    debug!(%data_dir, %listen, %allure_bin, "configuration");

    let state = AppState::new(PathBuf::from(&data_dir), allure_bin);
    let router = app::router(state);

    let addr: SocketAddr = listen.parse()?;
    info!(%addr, "binding listener");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("listener bound, starting HTTP server");

    axum::serve(listener, router).await?;

    info!("server stopped");
    Ok(())
}
