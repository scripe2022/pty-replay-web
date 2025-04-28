// run  := cargo run
// dir  := .
// kid  :=

use anyhow::Context;
use axum::Router;
use axum::routing::{get, post};
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

    println!("Listening on {address}");

    let db = MariaDB::new().await.context("init DB")?;
    let minio = MinIO::new().await.context("init MinIO")?;

    let state = AppState { db, minio };

    let app = Router::new()
        .nest_service(
            "/replay/static",
            ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static")),
        )
        .route("/replay/view/{id}", get(view))
        .route("/replay", get(index))
        .route("/replay/list", get(list))
        .route("/replay/upload", post(upload))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();

    axum::serve(listener, app).await.context("service")
}
