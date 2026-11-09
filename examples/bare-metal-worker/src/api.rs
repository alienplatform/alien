//! The HTTP API: health, submitting jobs, reading them back.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::worker::{SubmitError, Task, Worker, VERSION};

pub fn router(worker: Arc<Worker>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/jobs", get(list_jobs).post(create_job))
        .route("/jobs/{id}", get(get_job))
        .with_state(worker)
}

async fn healthz(State(worker): State<Arc<Worker>>) -> Response {
    if worker.is_draining() {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "draining", "version": VERSION })),
        )
            .into_response()
    } else {
        Json(json!({ "status": "ok", "version": VERSION })).into_response()
    }
}

async fn create_job(State(worker): State<Arc<Worker>>, body: Bytes) -> Response {
    let task: Task = match serde_json::from_slice(&body) {
        Ok(task) => task,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match worker.submit(task) {
        Ok(job) => (
            StatusCode::ACCEPTED,
            Json(json!({ "id": job.id, "status": job.status })),
        )
            .into_response(),
        Err(SubmitError::Draining) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "draining",
                "error": "the worker is shutting down and takes no new jobs",
            })),
        )
            .into_response(),
        Err(SubmitError::QueueFull) => error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "too many jobs are waiting, try again shortly".to_string(),
        ),
        Err(SubmitError::Invalid(message)) => error_response(StatusCode::BAD_REQUEST, message),
    }
}

async fn list_jobs(State(worker): State<Arc<Worker>>) -> Response {
    Json(worker.recent_jobs()).into_response()
}

async fn get_job(State(worker): State<Arc<Worker>>, Path(id): Path<String>) -> Response {
    match worker.job(&id) {
        Some(job) => Json(job).into_response(),
        None => error_response(StatusCode::NOT_FOUND, format!("no job {id}")),
    }
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;
    use crate::worker::Config;

    fn app() -> (Router, Arc<Worker>) {
        let worker = Worker::start(Config {
            concurrency: 1,
            queue_capacity: 2,
        });
        (router(worker.clone()), worker)
    }

    async fn call(app: &Router, method: Method, path: &str, body: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    async fn until_running(app: &Router, id: &str) {
        for _ in 0..1_000 {
            let (_, job) = call(app, Method::GET, &format!("/jobs/{id}"), "").await;
            if job["status"] == "running" {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("job {id} never started");
    }

    #[tokio::test(start_paused = true)]
    async fn healthz_reports_ok_with_the_version() {
        let (app, _) = app();

        let (status, body) = call(&app, Method::GET, "/healthz", "").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "status": "ok", "version": "0.1.0" }));
    }

    #[tokio::test(start_paused = true)]
    async fn submitting_and_reading_a_job() {
        let (app, _) = app();

        let (status, body) = call(
            &app,
            Method::POST,
            "/jobs",
            r#"{"task":"wait","seconds":300}"#,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body, json!({ "id": "j-1", "status": "queued" }));

        until_running(&app, "j-1").await;
        let (status, job) = call(&app, Method::GET, "/jobs/j-1", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(job["id"], "j-1");
        assert_eq!(job["task"], "wait");
        assert_eq!(job["seconds"], 300);
        assert_eq!(job["status"], "running");
        assert_eq!(job["version"], "0.1.0");
        assert!(job["started_at"].is_string());
        assert_eq!(job["finished_at"], Value::Null);

        tokio::time::advance(Duration::from_secs(300)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        let (_, job) = call(&app, Method::GET, "/jobs/j-1", "").await;
        assert_eq!(job["status"], "finished");
        assert!(job["finished_at"].is_string());

        let (status, jobs) = call(&app, Method::GET, "/jobs", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(jobs.as_array().expect("array").len(), 1);
        assert_eq!(jobs[0]["id"], "j-1");
    }

    #[tokio::test(start_paused = true)]
    async fn draining_turns_health_and_submissions_into_503() {
        let (app, worker) = app();
        call(
            &app,
            Method::POST,
            "/jobs",
            r#"{"task":"wait","seconds":60}"#,
        )
        .await;
        until_running(&app, "j-1").await;

        let draining = tokio::spawn({
            let worker = worker.clone();
            async move { worker.drain().await }
        });
        while !worker.is_draining() {
            tokio::task::yield_now().await;
        }

        let (status, body) = call(&app, Method::GET, "/healthz", "").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({ "status": "draining", "version": "0.1.0" }));

        let (status, body) = call(
            &app,
            Method::POST,
            "/jobs",
            r#"{"task":"wait","seconds":1}"#,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "draining");

        let (status, job) = call(&app, Method::GET, "/jobs/j-1", "").await;
        assert_eq!(status, StatusCode::OK, "running jobs stay readable");
        assert_eq!(job["status"], "running");

        tokio::time::advance(Duration::from_secs(60)).await;
        draining.await.expect("drain task");
        let (_, job) = call(&app, Method::GET, "/jobs/j-1", "").await;
        assert_eq!(job["status"], "finished");
    }

    #[tokio::test(start_paused = true)]
    async fn bad_requests_get_a_json_error() {
        let (app, _) = app();

        let (status, body) = call(&app, Method::POST, "/jobs", r#"{"task":"nap"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("unknown variant `nap`"),
            "{body}"
        );

        let (status, body) = call(
            &app,
            Method::POST,
            "/jobs",
            r#"{"task":"wait","seconds":0}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "error": "seconds must be 1 to 86400" }));

        let (status, body) = call(&app, Method::GET, "/jobs/j-99", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({ "error": "no job j-99" }));
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_queue_answers_429() {
        let (app, _) = app();
        let submit = r#"{"task":"wait","seconds":60}"#;
        call(&app, Method::POST, "/jobs", submit).await;
        until_running(&app, "j-1").await;
        call(&app, Method::POST, "/jobs", submit).await;
        call(&app, Method::POST, "/jobs", submit).await;

        let (status, body) = call(&app, Method::POST, "/jobs", submit).await;

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(body["error"].is_string());
    }
}
