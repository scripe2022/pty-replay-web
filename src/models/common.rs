use aws_config::BehaviorVersion;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::put_object::PutObjectError;
use aws_sdk_s3::{Client, config::Builder as S3ConfBuilder, primitives::ByteStream};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use sqlx::QueryBuilder;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::{Row, MySql, Pool};
use std::ops::DerefMut;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub type Heartbeats = Vec<(usize, OffsetDateTime, OffsetDateTime)>;

#[derive(Debug, Deserialize, Clone)]
pub struct CastJson {
    pub content: String,
    pub filename: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UploadJson {
    pub casts: Vec<CastJson>,
    pub heartbeat: String,
    pub notes: String,
    pub uuid: Option<Uuid>,
}

#[derive(Serialize)]
pub struct UploadResp {
    pub ok: bool,
    pub url: String,
}

#[derive(Debug)]
pub struct Cast {
    pub filename: String,
    pub content: String,
    pub datetime: OffsetDateTime,
    pub height: u16,
    pub width: u16,
}

#[derive(Clone)]
pub struct MariaDB {
    pool: Pool<MySql>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LogMeta {
    pub uuid: String,
    pub note: String,
    pub uploaded_at: OffsetDateTime,
    pub visible: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HeartbeatMeta {
    pub session: usize,
    pub started_at: OffsetDateTime,
    pub ended_at: OffsetDateTime,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CastMeta {
    pub id: u32,
    pub bucket: String,
    pub path: String,
    pub size_byte: u32,
    pub started_at: OffsetDateTime,
    pub height: u16,
    pub width: u16,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MarkMeta {
    pub id: u32,
    pub second: f64,
    pub note: String,
}

impl MariaDB {
    pub async fn new() -> anyhow::Result<Self> {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let pool = MySqlPoolOptions::new()
            .max_connections(255)
            .min_connections(1)
            .connect(&url)
            .await?;
        anyhow::Ok(Self { pool })
    }

    pub async fn insert(
        &self,
        uuid: &Uuid,
        note: &String,
        heartbeats: &Heartbeats,
        casts: &[Cast],
    ) -> anyhow::Result<()> {
        let uuid_str = uuid.to_string();

        let mut tx = self.pool.begin().await?;
        sqlx::query!(r#"INSERT INTO logs (uuid, note) VALUES (?, ?)"#, &uuid_str, note)
            .execute(tx.deref_mut())
            .await?;

        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new(r#"INSERT INTO heartbeats (uuid, session, started_at, ended_at)"#);

        qb.push_values(heartbeats.iter(), |mut b, hb| {
            b.push_bind(&uuid_str);
            b.push_bind(hb.0 as u32);
            b.push_bind(hb.1);
            b.push_bind(hb.2);
        });
        qb.build().execute(tx.deref_mut()).await?;

        let bucket = std::env::var("S3_BUCKET").unwrap();
        let key = format!("{}/{}", std::env::var("S3_KEY_PREFIX").unwrap_or_default(), &uuid_str);
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new(r#"INSERT INTO casts (uuid, bucket, path, size_byte, started_at, height, width)"#);
        qb.push_values(casts.iter(), |mut b, cast| {
            b.push_bind(&uuid_str);
            b.push_bind(&bucket);
            b.push_bind(format!("{}/{}", key, cast.filename));
            b.push_bind(cast.content.len() as u32);
            b.push_bind(cast.datetime);
            b.push_bind(cast.height);
            b.push_bind(cast.width);
        });
        qb.build().execute(tx.deref_mut()).await?;

        tx.commit().await?;
        anyhow::Ok(())
    }

    pub async fn query_logs(&self) -> anyhow::Result<Vec<LogMeta>> {
        let rows = sqlx::query_as!(
            LogMeta,
            r#"
            SELECT
                uuid        AS `uuid!: String`,
                note        AS `note!: String`,
                uploaded_at AS `uploaded_at!: OffsetDateTime`,
                visible     AS `visible!: bool`
            FROM logs
            ORDER BY uploaded_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn query_single_log(&self, uuid: &Uuid) -> anyhow::Result<Option<LogMeta>> {
        let row = sqlx::query_as!(
            LogMeta,
            r#"
            SELECT
                uuid        AS `uuid!: String`,
                note        AS `note!: String`,
                uploaded_at AS `uploaded_at!: OffsetDateTime`,
                visible     AS `visible!: bool`
            FROM logs
            WHERE uuid=?
            ORDER BY uploaded_at DESC
            "#,
            uuid.to_string()
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn query_heartbeats(&self, uuid: &Uuid) -> anyhow::Result<Vec<HeartbeatMeta>> {
        let rows = sqlx::query_as!(
            HeartbeatMeta,
            r#"
            SELECT
                session    AS `session!: u16`,
                started_at AS `started_at!: OffsetDateTime`,
                ended_at   AS `ended_at!: OffsetDateTime`
            FROM heartbeats
            WHERE uuid=?
            ORDER BY session, started_at
            "#,
            uuid.to_string()
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn query_casts(&self, uuid: &Uuid) -> anyhow::Result<Vec<CastMeta>> {
        let rows = sqlx::query_as!(
            CastMeta,
            r#"
            SELECT
                id         AS `id!: u32`,
                bucket     AS `bucket!: String`,
                path       AS `path!: String`,
                size_byte  AS `size_byte!: u32`,
                started_at AS `started_at!: OffsetDateTime`,
                height     AS `height!: u16`,
                width      AS `width!: u16`
            FROM casts
            WHERE uuid=?
            ORDER BY started_at
            "#,
            uuid.to_string()
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn query_marks(&self, id: u32) -> anyhow::Result<Vec<MarkMeta>> {
        let rows = sqlx::query_as!(
            MarkMeta,
            r#"
            SELECT
                id         AS `id!: u32`,
                second     AS `second!: f64`,
                note       AS `note!: String`
            FROM marks
            WHERE cast_id=?
            ORDER BY second
            "#,
            id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete_mark(&self, mark_id: u32) -> anyhow::Result<()> {
        sqlx::query!("DELETE FROM marks WHERE id=?", mark_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_mark(&self, cast_id: u32, second: f64, note: String) -> anyhow::Result<u32> {
        let row = sqlx::query!(
            r#"
            INSERT INTO marks (cast_id, second, note)
                VALUES (?, ?, ?)
            RETURNING id
            "#,
            cast_id,
            second,
            note
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<u32, _>(0))
    }
}

#[derive(Clone)]
pub struct MinIO {
    client: Arc<Client>,
    bucket: String,
}

impl MinIO {
    pub async fn new() -> anyhow::Result<Self> {
        let bucket = std::env::var("S3_BUCKET")?;
        let endpoint = std::env::var("S3_ENDPOINT")?;
        let shared = aws_config::defaults(BehaviorVersion::latest()).load().await;

        let conf = S3ConfBuilder::from(&shared)
            .force_path_style(true)
            .endpoint_url(endpoint)
            .build();

        Ok(Self {
            client: Arc::new(Client::from_conf(conf)),
            bucket,
        })
    }

    async fn upload(
        &self,
        key: &str,
        body: impl Into<ByteStream>,
    ) -> Result<(), SdkError<aws_sdk_s3::operation::put_object::PutObjectError>> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body.into())
            .send()
            .await
            .map(|_| ())
    }

    pub async fn upload_casts(
        &self,
        uuid: &Uuid,
        casts: &[Cast],
    ) -> Result<(), SdkError<aws_sdk_s3::operation::put_object::PutObjectError>> {
        let prefix = std::env::var("S3_KEY_PREFIX").unwrap_or_default();
        let tasks = casts.iter().map(|c| {
            let key = format!("{}/{}/{}", prefix, &uuid, c.filename);
            let body = c.content.clone().into_bytes();
            async move { self.upload(&key, body).await }
        });
        try_join_all(tasks).await.map(|_| ())
    }

    pub async fn upload_heartbeats(
        &self,
        uuid: &Uuid,
        hb_raw: &str,
    ) -> Result<(), SdkError<aws_sdk_s3::operation::put_object::PutObjectError>> {
        let prefix = std::env::var("S3_KEY_PREFIX").unwrap_or_default();
        self.upload(
            &format!("{}/{}/heartbeats.log", prefix, uuid),
            hb_raw.as_bytes().to_vec(),
        )
        .await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(anyhow::Error),

    #[error("db ctx: {0}")]
    DbCtx(#[from] anyhow::Error),

    #[error("storage: {0}")]
    Storage(#[from] SdkError<PutObjectError>),

    #[error("log {0} not found")]
    LogNotFound(Uuid),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()).into_response(),
            AppError::LogNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()).into_response(),
            AppError::DbCtx(_) | AppError::Storage(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
            }
        }
    }
}

pub mod filters {
    use askama::{Result, Values};
    use time::{OffsetDateTime, UtcOffset, format_description::FormatItem, macros::format_description};

    const HUMAN_FMT: &[FormatItem<'static>] =
        format_description!("[month repr:short] [day] [hour repr:24]:[minute]:[second] [period case:upper]");

    pub fn human(dt: &OffsetDateTime, _vals: &dyn Values) -> Result<String> {
        let local = UtcOffset::current_local_offset()
            .map(|off| dt.to_offset(off))
            .unwrap_or(*dt);
        local.format(HUMAN_FMT).map_err(askama::Error::custom)
    }
}
