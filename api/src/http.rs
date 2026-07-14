use crate::{SharedState, document, query};
use axum::{Router, routing::get};

pub fn create_app(state: SharedState) -> Router {
    Router::new()
        .route("/cc-talk-docs", get(query::query))
        .route("/cc-talk-docs/pages", get(document::document))
        .with_state(state)
        .route("/health", get(health_check))
}

async fn health_check() -> &'static str {
    r#"{ "status": "ok" }"#
}
