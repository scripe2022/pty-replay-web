use crate::AppState;
use crate::models::{AppError, MarkMeta, filters};
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::{Path, State};
use serde::Serialize;
use std::collections::BTreeMap;
use time::{Duration, OffsetDateTime};
use tokio::try_join;
use uuid::Uuid;

#[derive(Serialize)]
struct Cast {
    id: u32,
    bucket: String,
    path: String,
    size_byte: u32,
    duration: Duration,
    active_duration: Duration,
    event_count: u32,
    started_at: OffsetDateTime,
    marks: Vec<MarkMeta>,
}

#[derive(Template, WebTemplate)]
#[template(path = "view.html")]
pub struct ViewTemplate {
    uploaded_at: OffsetDateTime,
    note: String,
    heartbeats: Vec<(usize, OffsetDateTime, OffsetDateTime)>,
    casts: Vec<Cast>,
    uuid: Uuid,
}

impl Cast {
    pub fn is_short(&self) -> bool {
        self.active_duration < Duration::seconds(3) && self.event_count < 10
    }
    pub fn duration_mmss(&self) -> String {
        let s = self.duration.whole_seconds();
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

pub async fn view(State(app): State<AppState>, Path(id): Path<Uuid>) -> Result<ViewTemplate, AppError> {
    let log = app
        .db
        .query_single_log(&id)
        .await?
        .ok_or_else(|| AppError::LogNotFound(id))?;
    let (heartbeats, casts) = try_join!(app.db.query_heartbeats(&id), app.db.query_casts(&id),)?;

    let casts: Vec<Cast> = futures::future::try_join_all(casts.into_iter().map(|cast| {
        let db = app.db.clone();
        async move {
            let marks = db.query_marks(cast.id).await?;
            anyhow::Ok(Cast {
                id: cast.id,
                bucket: cast.bucket.clone(),
                path: cast.path.clone(),
                size_byte: cast.size_byte,
                duration: cast.duration,
                active_duration: cast.active_duration,
                event_count: cast.event_count,
                started_at: cast.started_at,
                marks,
            })
        }
    }))
    .await?;

    let mut hb_map = BTreeMap::<usize, Vec<(OffsetDateTime, OffsetDateTime)>>::new();
    for itv in heartbeats {
        hb_map
            .entry(itv.session)
            .or_default()
            .push((itv.started_at, itv.ended_at));
    }
    let gap = std::env::var("INTERVAL_GAP_SECOND")
        .unwrap_or_else(|_| "30".to_string())
        .parse::<i64>()
        .unwrap_or(30);
    let gap = Duration::seconds(gap);

    let mut heartbeats = Vec::<(usize, OffsetDateTime, OffsetDateTime)>::new();

    for (&session, hbs) in hb_map.iter() {
        let itvs = hbs
            .iter()
            .copied()
            .fold(Vec::<(OffsetDateTime, OffsetDateTime)>::new(), |mut acc, itv| {
                match acc.last_mut() {
                    Some((_, end)) if itv.0 - *end <= gap => *end = itv.1,
                    _ => acc.push(itv),
                }
                acc
            });
        heartbeats.extend(itvs.into_iter().map(|(s, e)| (session, s, e)));
    }

    heartbeats.sort_by_key(|&(_, s, _)| s);

    Ok(ViewTemplate {
        note: log.note,
        uploaded_at: log.uploaded_at,
        heartbeats,
        casts,
        uuid: id,
    })
}
