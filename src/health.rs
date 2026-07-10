use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthStatus {
    status: &'static str,
}

pub async fn health() -> (StatusCode, Json<HealthStatus>) {
    (StatusCode::OK, Json(HealthStatus { status: "ok" }))
}
