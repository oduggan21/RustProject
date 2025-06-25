//! src/routes.rs
use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    email_agent::{FollowUp, InviteAccepted},
    engine::run_goal,
    memory::{ProspectMem, Status},
};

/// Same concrete state type as in `main.rs`
pub type TaskList = Arc<Mutex<Vec<String>>>;

// ── incoming JSON payloads ─────────────────────────────────────────────
#[derive(Deserialize)]
struct ProspectInfo {
    name:    String,
    email:   String,
    company: String,
    role:    String,
}

#[derive(Deserialize)]
pub struct GoalSpec {
    name:     String,
    interval: u64,
    prospect: ProspectInfo,
}

// ── POST /goal ─────────────────────────────────────────────────────────
pub async fn post_goal(
    State(tasks): State<TaskList>,
    Json(spec): Json<GoalSpec>,
) -> impl IntoResponse {
    let job_name = spec.name.clone();
    tasks.lock().await.push(job_name.clone());

    if spec.name == "email_followup" {
        let mem = ProspectMem {
            name:    spec.prospect.name.clone(),
            email:   spec.prospect.email.clone(),
            company: spec.prospect.company.clone(),
            role:    spec.prospect.role.clone(),
            status:  Status::Waiting,
            ..Default::default()
        };

        tokio::spawn(run_goal(
            job_name,
            spec.interval,
            mem,
            FollowUp,
            InviteAccepted,
        ));
    } else {
        eprintln!("unknown goal {}", spec.name);
    }

    // Return 200 OK with no body
    ().into_response()
}

// ── GET /status ────────────────────────────────────────────────────────
pub async fn get_status(
    State(tasks): State<TaskList>,
) -> Json<Vec<String>> {
    let list = tasks.lock().await.clone();
    Json(list)
}
