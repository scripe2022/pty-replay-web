use crate::AppState;
use crate::models::{AppError, LogMeta, filters};
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::State;

#[derive(Template, WebTemplate)]
#[template(path = "list.html")]
pub struct ListTemplate {
    logs: Vec<LogMeta>,
}

pub async fn list(State(app): State<AppState>) -> Result<ListTemplate, AppError> {
    let logs = app.db.query_logs().await?;
    Ok(ListTemplate { logs })
}
