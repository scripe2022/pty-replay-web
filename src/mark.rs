use crate::AppState;
use crate::models::AppError;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AddReq {
    cast_id: u32,
    second: f64,
    note: String,
}

#[derive(Deserialize)]
pub struct DelReq {
    mark_id: u32,
}

#[derive(Serialize)]
struct AddResp {
    ok: bool,
    mark_id: u32,
}

#[derive(Serialize)]
struct DelResp {
    ok: bool,
}

pub async fn add_mark(State(app): State<AppState>, Json(payload): Json<AddReq>) -> Result<impl IntoResponse, AppError> {
    let mark_id = app.db.add_mark(payload.cast_id, payload.second, payload.note).await?;
    Ok((StatusCode::CREATED, Json(AddResp { ok: true, mark_id })))
}

pub async fn del_mark(State(app): State<AppState>, Json(payload): Json<DelReq>) -> Result<impl IntoResponse, AppError> {
    let mark_id = payload.mark_id;
    app.db.delete_mark(mark_id).await?;
    Ok((StatusCode::CREATED, Json(DelResp { ok: true })))
}
