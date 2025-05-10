use crate::AppState;
use crate::models::AppError;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct VisibleReq {
    uuid: Uuid,
    visible: bool,
}
#[derive(Serialize)]
struct VisibleResp {
    ok: bool,
}

pub async fn visible(
    State(app): State<AppState>,
    Json(payload): Json<VisibleReq>,
) -> Result<impl IntoResponse, AppError> {
    app.db.update_visible(payload.uuid, payload.visible).await?;
    Ok((StatusCode::CREATED, Json(VisibleResp { ok: true })))
}
