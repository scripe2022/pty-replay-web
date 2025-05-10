use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize, de};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use time::{Duration, OffsetDateTime};
use tokio::try_join;
use uuid::Uuid;

use crate::AppState;
use crate::models::log::{CastRaw, HBRaw, parse_log};
use crate::models::{AppError, Cast, Heartbeats, UploadResp};

#[derive(Debug, Deserialize, Serialize)]
struct Header {
    version: u8,
    width: u16,
    height: u16,
    timestamp: i64,
    env: Value,
}

#[derive(Debug, Serialize)]
enum AsciinemaEvent {
    O(Duration),
    R(Duration, u16, u16),
}

impl<'de> Deserialize<'de> for AsciinemaEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (sec, tag, payload): (f64, String, String) = Deserialize::deserialize(deserializer)?;
        let d = Duration::seconds_f64(sec);

        match tag.as_str() {
            "o" => Ok(AsciinemaEvent::O(d)),
            "r" => {
                let (h, w) = payload
                    .split_once('x')
                    .ok_or_else(|| de::Error::custom("stty size wrong"))?;
                Ok(AsciinemaEvent::R(
                    d,
                    h.parse().map_err(de::Error::custom)?,
                    w.parse().map_err(de::Error::custom)?,
                ))
            }
            _ => Err(de::Error::custom(format!("unknown event tag {tag:?}"))),
        }
    }
}

struct CastPartial {
    timestamp: i64,
    duration: Duration,
    active_duration: Duration,
    event_count: u32,
    height: u16,
    width: u16,
    content: String,
}

fn update_cast(src: String) -> anyhow::Result<CastPartial> {
    let mut lines = src.lines().map(str::to_owned).collect::<Vec<_>>();

    let events = lines
        .iter()
        .skip(1)
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect::<Vec<AsciinemaEvent>>();

    let event_count = events.len();

    let header_line = lines.first().context("empty asciinema cast")?.as_str();
    let mut header: Header = serde_json::from_str(header_line).context("invalid header JSON")?;
    let timestamp = header.timestamp;

    let resizes = events
        .iter()
        .filter_map(|e| match *e {
            AsciinemaEvent::R(_, w, h) => Some((w, h)),
            _ => None,
        })
        .collect::<Vec<(u16, u16)>>();

    let max_w = resizes.iter().map(|&(w, _)| w).max().unwrap_or(header.width);
    let max_h = resizes.iter().map(|&(_, h)| h).max().unwrap_or(header.height);

    let duration = events
        .iter()
        .rev()
        .find_map(|e| match e {
            AsciinemaEvent::O(d) => Some(*d),
            _ => None,
        })
        .unwrap_or(Duration::ZERO);

    let duration_active = events
        .windows(2)
        .rev()
        .find_map(|w| match (&w[0], &w[1]) {
            (AsciinemaEvent::O(_), AsciinemaEvent::O(d)) => Some(*d),
            _ => None,
        })
        .unwrap_or(Duration::ZERO);

    header.width = max_w;
    header.height = max_h;
    lines[0] = serde_json::to_string(&header)?;

    Ok(CastPartial {
        timestamp,
        duration,
        active_duration: duration_active,
        event_count: event_count as u32,
        height: max_h,
        width: max_w,
        content: lines.join("\n"),
    })
}

fn process(hbs_raw: &[HBRaw], casts_raw: &[CastRaw]) -> anyhow::Result<(Heartbeats, Vec<Cast>)> {
    let mut hb_map = HashMap::<usize, Vec<OffsetDateTime>>::new();
    hbs_raw.iter().for_each(|x| {
        hb_map.entry(x.session).or_default().push(x.time);
    });
    let mut hb_itvs = Vec::<(usize, OffsetDateTime, OffsetDateTime)>::new();
    let gap = Duration::seconds(10);
    for (session, hbs) in hb_map.into_iter() {
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
        hb_itvs.extend(itvs.into_iter().map(|(start, end)| (session, start, end)));
    }

    let casts = casts_raw
        .iter()
        .map(|cast| {
            let filename = Path::new(&cast.filename)
                .file_name()
                .context("invalid filename")?
                .to_string_lossy()
                .to_string();
            let content = cast.content.clone();
            let cast_partial = update_cast(content)?;
            let datetime = OffsetDateTime::from_unix_timestamp(cast_partial.timestamp).context("invalid timestamp")?;
            anyhow::Ok(Cast {
                filename,
                started_at: datetime,
                content: cast_partial.content,
                duration: cast_partial.duration,
                active_duration: cast_partial.active_duration,
                event_count: cast_partial.event_count,
                height: cast_partial.height,
                width: cast_partial.width,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((hb_itvs, casts))
}

#[derive(Debug, Deserialize, Clone)]
pub struct UploadMeta {
    notes: String,
    logs: String,
    uuid: Option<Uuid>,
}

pub async fn upload(
    State(app): State<AppState>,
    Json(payload): Json<UploadMeta>,
) -> Result<impl IntoResponse, AppError> {
    let uuid = payload.uuid.unwrap_or(Uuid::new_v4());
    let notes = payload.notes;
    let (hbs_raw, casts_raw) = parse_log(&payload.logs);
    let (hb_itvs, casts) = process(&hbs_raw, &casts_raw).map_err(AppError::BadRequest)?;
    let hbs_raw = format!("{:?}", hbs_raw);

    try_join!(
        async {
            app.minio.upload_casts(&uuid, &casts).await?;
            Ok::<_, AppError>(())
        },
        async {
            app.minio.upload_heartbeats(&uuid, &hbs_raw).await?;
            Ok::<_, AppError>(())
        },
        async {
            app.db.insert(&uuid, &notes, &hb_itvs, &casts).await?;
            Ok::<_, AppError>(())
        },
    )?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResp {
            ok: true,
            url: format!("/replay/view/{}", uuid),
        }),
    ))
}
