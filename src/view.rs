use tokio::try_join;
use crate::AppState;
use crate::models::{AppError, CastMeta, filters};
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::{Path, State};
use std::collections::BTreeMap;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Template, WebTemplate)]
#[template(path = "view.html")]
pub struct ViewTemplate {
    uploaded_at: OffsetDateTime,
    endpoint: String,
    note: String,
    heartbeats: BTreeMap<usize, Vec<(OffsetDateTime, OffsetDateTime)>>,
    casts: Vec<CastMeta>,
}

pub async fn view(State(app): State<AppState>, Path(id): Path<Uuid>) -> Result<ViewTemplate, AppError> {
    let log = app
        .db
        .query_single_log(&id)
        .await?
        .ok_or_else(|| AppError::LogNotFound(id))?;
    let (heartbeats, casts) = try_join!(app.db.query_heartbeats(&id), app.db.query_casts(&id),)?;

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

    let mut heartbeats = BTreeMap::<usize, Vec<(OffsetDateTime, OffsetDateTime)>>::new();

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
        heartbeats.insert(session, itvs);
    }

    let endpoint = std::env::var("S3_BUCKET_ENDPOINT").unwrap_or_default();
    Ok(ViewTemplate {
        note: log.note,
        endpoint,
        uploaded_at: log.uploaded_at,
        heartbeats,
        casts,
    })
}
