use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use time::{Duration, OffsetDateTime};
use tokio::try_join;
use uuid::Uuid;
use binrw::BinRead;

use crate::AppState;
use crate::models::log::{CastRaw, parse_log};
use crate::models::{AppError, Cast, Heartbeats, UploadResp};

#[derive(Debug, Deserialize, Serialize)]
struct Header {
    version: u8,
    width: u16,
    height: u16,
    timestamp: i64,
    env: Value,
}

#[derive(Debug)]
enum Event {
    Input { elapsed: f32, data: String },
    Output { elapsed: f32, data: String },
    Resize { elapsed: f32, cols: u16, rows: u16 },
}

impl Event {
    fn to_json(&self) -> anyhow::Result<String> {
        match self {
            Event::Input { elapsed, data } => {
                serde_json::to_string(&json!([elapsed, "i", data])).context("failed to serialize input event")
            }
            Event::Output { elapsed, data } => {
                serde_json::to_string(&json!([elapsed, "o", data])).context("failed to serialize output event")
            }
            Event::Resize { elapsed, cols, rows } => {
                serde_json::to_string(&json!([elapsed, "r", format!("{}x{}", cols, rows)]))
                    .context("failed to serialize resize event")
            }
        }
    }
    fn get_elapsed(&self) -> f32 {
        match self {
            Event::Input { elapsed, .. } => *elapsed,
            Event::Output { elapsed, .. } => *elapsed,
            Event::Resize { elapsed, .. } => *elapsed,
        }
    }
    fn set_elapsed(&mut self, new: f32) {
        match self {
            Event::Input { elapsed, .. } | Event::Output { elapsed, .. } | Event::Resize { elapsed, .. } => {
                *elapsed = new
            }
        }
    }
}

use unsigned_varint::io::read_u32;
impl BinRead for Event {
    type Args<'a> = ();
    fn read_options<R: std::io::Read + binrw::io::Seek>(
        reader: &mut R,
        _endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let elapsed = f32::read_le(reader)?;
        let kind = u8::read_le(reader)?;

        Ok(match kind {
            0 | 1 => {
                let len = read_u32(&mut *reader).map_err(|e| binrw::Error::Io(e.into()))? as usize;
                let mut buf = vec![0; len];
                reader.read_exact(&mut buf)?;
                let data = String::from_utf8(buf).map_err(|e| binrw::Error::AssertFail {
                    pos: reader.stream_position().unwrap(),
                    message: format!("utf-8 error: {e:?}"),
                })?;
                if kind == 0 {
                    Event::Input { elapsed, data }
                } else {
                    Event::Output { elapsed, data }
                }
            }
            2 => {
                let rows = u16::read_le(reader)?;
                let cols = u16::read_le(reader)?;
                Event::Resize { elapsed, cols, rows }
            }
            _ => {
                return Err(binrw::Error::AssertFail {
                    pos: reader.stream_position()?,
                    message: format!("unknown kind {kind}"),
                });
            }
        })
    }
}

struct CastPartial {
    timestamp: i64,
    duration: Duration,
    active_duration: Duration,
    event_count: u32,
    content: String,
}

fn convert_cast(src: Vec<u8>) -> anyhow::Result<CastPartial> {
    #[derive(Debug, BinRead)]
    #[brw(little)]
    struct CastHeader {
        ts: u128,
    }
    let length = src.len();
    let mut cur = binrw::io::Cursor::new(src);
    let cast_header: CastHeader = CastHeader::read_le(&mut cur)?;
    let mut events = Vec::new();
    while (cur.position() as usize) < length {
        let event = Event::read_le(&mut cur)?;
        events.push(event);
    }

    let event_count = events.len();

    let timestamp = cast_header.ts;

    let duration = events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Output { elapsed, .. } => Some(Duration::seconds_f32(*elapsed)),
            _ => None,
        })
        .unwrap_or(Duration::ZERO);

    let duration_active = events
        .windows(2)
        .rev()
        .find_map(|w| match (&w[0], &w[1]) {
            (Event::Output { .. }, Event::Output { elapsed, .. }) => Some(Duration::seconds_f32(*elapsed)),
            _ => None,
        })
        .unwrap_or(Duration::ZERO);

    let header = json!({
        "version": 3,
        "term": {
            "cols": 80,
            "rows": 24,
            "type": "xterm-color"
        },
        "timestamp": timestamp,
        "env": {
            "SHELL": "/bin/bash",
            "TERM": "xterm-color",
        },
    });
    let header = serde_json::to_string(&header).context("failed to serialize header")?;

    let mut prev = 0.0;
    for ev in events.iter_mut() {
        let cur = ev.get_elapsed();
        ev.set_elapsed(cur - prev);
        prev = cur;
    }
    let body = events
        .iter()
        .filter_map(|e| e.to_json().ok())
        .collect::<Vec<String>>()
        .join("\n");
    let content = format!("{header}\n{body}\n");

    Ok(CastPartial {
        timestamp: (timestamp / 1000) as i64,
        duration,
        active_duration: duration_active,
        event_count: event_count as u32,
        content,
    })
}

fn process(hbs_raw: &[OffsetDateTime], casts_raw: &[CastRaw]) -> anyhow::Result<(Heartbeats, Vec<Cast>)> {
    let gap = Duration::seconds(10);
    let itvs = hbs_raw
        .iter()
        .copied()
        .fold(Vec::<(OffsetDateTime, OffsetDateTime)>::new(), |mut acc, x| {
            match acc.last_mut() {
                Some((_, end)) if x - *end <= gap => *end = x,
                _ => acc.push((x, x)),
            }
            acc
        });

    let casts = casts_raw
        .iter()
        .map(|cast| {
            let filename = Path::new(&cast.filename)
                .file_name()
                .context("invalid filename")?
                .to_string_lossy()
                .to_string();
            let content = cast.content.clone();
            let cast_partial = convert_cast(content)?;
            let datetime = OffsetDateTime::from_unix_timestamp(cast_partial.timestamp).context("invalid timestamp")?;
            anyhow::Ok(Cast {
                filename,
                started_at: datetime,
                content: cast_partial.content,
                duration: cast_partial.duration,
                active_duration: cast_partial.active_duration,
                event_count: cast_partial.event_count,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((itvs, casts))
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
            url: format!("/view/{}", uuid),
        }),
    ))
}
