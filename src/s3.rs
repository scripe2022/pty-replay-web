use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use bytes::Bytes;
pub async fn s3_proxy(State(state): State<AppState>, Path((bucket, key)): Path<(String, String)>) -> impl IntoResponse {
    match state.minio.get_object_stream(&bucket, &key).await {
        Ok(byte_stream) => match byte_stream.collect().await {
            Ok(aggregated) => {
                let bytes: Bytes = aggregated.into_bytes();
                let body = Body::from(bytes);

                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/octet-stream")],
                    body,
                )
                    .into_response()
            }
            Err(e) => {
                eprintln!("S3 read error: {e:#}");
                (StatusCode::BAD_GATEWAY, "read error").into_response()
            }
        },
        Err(err) => {
            eprintln!("S3 proxy error: {err:#}");
            (StatusCode::NOT_FOUND, "object not found").into_response()
        }
    }
}
