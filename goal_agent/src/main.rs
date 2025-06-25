//! src/main.rs
mod memory;
mod email_agent;
mod engine;
mod routes;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    routing::{get, post},
    Router, Server,
};
use routes::{get_status, post_goal, TaskList};
use tokio::sync::Mutex;

/// Shared application state: a growable list of job names.
pub type AppState = TaskList; // alias from routes.rs

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── 1. create state ────────────────────────────────────────────────
    let state: AppState = Arc::new(Mutex::new(Vec::new()));

    // ── 2. build router ────────────────────────────────────────────────
    let app = Router::new()
        .route("/goal",   post(post_goal))
        .route("/status", get(get_status))
        .with_state(state);

    // ── 3. serve & retain client SocketAddr in ConnectInfo extractor ───
    Server::bind(&"0.0.0.0:3000".parse::<SocketAddr>()?)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}
