use anyhow::Context;
use axum::Router;
use axum::routing::{delete, get, post};
use dotenvy::dotenv;
use tower_http::services::ServeDir;

mod models;
use models::{MariaDB, MinIO};

mod index;
use index::index;

mod list;
use list::list;

mod upload;
use upload::upload;

mod view;
use view::view;

mod mark;
use mark::{add_mark, del_mark};

mod note;
use note::note_update;

mod visible;
use visible::visible;

mod s3;
use s3::s3_proxy;

#[derive(Clone)]
struct AppState {
    db: MariaDB,
    minio: MinIO,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let address = format!("0.0.0.0:{port}");
    let dir = std::env::var("STATIC_DIR").unwrap();

    println!("Listening on {address}");

    let db = MariaDB::new().await.context("init DB")?;
    let minio = MinIO::new().await.context("init MinIO")?;

    let state = AppState { db, minio };

    let api_router = Router::new()
        .route("/mark", post(add_mark))
        .route("/mark", delete(del_mark))
        .route("/note", post(note_update))
        .route("/upload", post(upload))
        .route("/visible", post(visible));

    let core_router = Router::new()
        .route("/", get(index))
        .route("/list", get(list))
        .route("/view/{id}", get(view))
        .nest("/api", api_router);

    let app = Router::new()
        .nest_service("/static", ServeDir::new(dir))
        .merge(core_router)
        .route("/s3/{bucket}/{*key}", get(s3_proxy))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(address).await.unwrap();

    axum::serve(listener, app).await.context("service")
}
