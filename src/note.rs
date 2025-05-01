use crate::AppState;
use crate::models::AppError;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct NoteReq {
    uuid: Uuid,
    note: String,
}

#[derive(Serialize)]
struct NoteResp {
    ok: bool,
}

pub async fn note_update(
    State(app): State<AppState>,
    Json(payload): Json<NoteReq>,
) -> Result<impl IntoResponse, AppError> {
    let note_id = payload.uuid;
    app.db.update_note(note_id, payload.note).await?;
    Ok((StatusCode::CREATED, Json(NoteResp { ok: true })))
}
