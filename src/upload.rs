use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use time::{Duration, OffsetDateTime};
use tokio::try_join;
use uuid::Uuid;

use crate::AppState;
use crate::models::{AppError, Cast, Heartbeats, UploadJson, UploadResp};

#[derive(Debug, Deserialize, Serialize)]
struct Header {
    version: u8,
    width: u16,
    height: u16,
    timestamp: i64,
    env: Value,
}

// return: timestamp, height, width, notes
fn update_cast(src: String) -> anyhow::Result<(i64, u16, u16, String)> {
    let mut lines = src.lines().map(str::to_owned).collect::<Vec<_>>();
    let header_line = lines.first().context("empty asciinema log")?.as_str();
    let mut header: Header = serde_json::from_str(header_line).context("invalid header JSON")?;

    let timestamp = header.timestamp;
    let mut max_w = header.width;
    let mut max_h = header.height;

    for line in &lines[1..] {
        let evt: Vec<Value> = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if evt.len() == 3 && evt[1] == "r" {
            if let Some(dim) = evt[2].as_str() {
                if let Some((w, h)) = dim.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<u16>(), h.parse::<u16>()) {
                        max_w = max_w.max(w);
                        max_h = max_h.max(h);
                    }
                }
            }
        }
    }
    header.width = max_w;
    header.height = max_h;
    lines[0] = serde_json::to_string(&header)?;

    Ok((timestamp, max_h, max_w, lines.join("\n")))
}

fn parse_log(json: UploadJson) -> anyhow::Result<(String, Heartbeats, Vec<Cast>, String)> {
    let mut hb_map = HashMap::<usize, Vec<OffsetDateTime>>::new();
    let note = json.notes;
    let hb_raw = String::from_utf8(general_purpose::STANDARD.decode(json.heartbeat)?)?;
    hb_raw.lines().for_each(|line| {
        if let Some((session, ts)) = line.split_once(' ') {
            if let (Ok(session), Ok(ts)) = (session.parse::<usize>(), ts.parse::<i64>()) {
                if let Ok(ts) = OffsetDateTime::from_unix_timestamp(ts) {
                    hb_map.entry(session).or_default().push(ts);
                }
            }
        }
    });
    let mut hb_itvs = Vec::<(usize, OffsetDateTime, OffsetDateTime)>::new();
    let gap = Duration::seconds(10);
    for (session, hbs) in hb_map.iter() {
        let itvs = hbs
            .iter()
            .copied()
            .fold(Vec::<(OffsetDateTime, OffsetDateTime)>::new(), |mut acc, x| {
                match acc.last_mut() {
                    Some((_, end)) if x - *end <= gap => *end = x,
                    _ => acc.push((x, x)),
                }
                acc
            });
        hb_itvs.extend(itvs.into_iter().map(|(start, end)| (*session, start, end)));
    }
    let casts = json
        .casts
        .into_iter()
        .map(|cast| {
            let filename = cast.filename;
            let content = String::from_utf8(general_purpose::STANDARD.decode(cast.content)?)?;
            let (timestamp, height, width, content) = update_cast(content)?;
            let datetime = OffsetDateTime::from_unix_timestamp(timestamp).context("invalid timestamp")?;
            anyhow::Ok(Cast {
                filename,
                content,
                datetime,
                height,
                width
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::Ok((note, hb_itvs, casts, hb_raw))
}

pub async fn upload(
    State(app): State<AppState>,
    Json(payload): Json<UploadJson>,
) -> Result<impl IntoResponse, AppError> {

    let uuid = payload.uuid.unwrap_or(Uuid::new_v4());

    let (note, hb_itvs, casts, hb_raw) = parse_log(payload).map_err(AppError::BadRequest)?;

    try_join!(
        async {
            app.minio.upload_casts(&uuid, &casts).await?;
            Ok::<_, AppError>(())
        },
        async {
            app.minio.upload_heartbeats(&uuid, &hb_raw).await?;
            Ok::<_, AppError>(())
        },
        async {
            app.db.insert(&uuid, &note, &hb_itvs, &casts).await?;
            Ok::<_, AppError>(())
        },
    )?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResp {
            ok: true,
            url: format!("/replay/view/{}", uuid)
        }),
    ))
}
