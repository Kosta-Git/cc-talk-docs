mod document;
mod error;
mod http;
mod mcp;
mod query;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use common::{
    embed, pdf,
    storage::{self},
};
use fastembed::TextEmbedding;
use pdfium_render::prelude::Pdfium;
use rmcp::transport::stdio;
use rusqlite::Connection;

use crate::{http::create_app, mcp::DocsMcp};

/// Initializes the database and returns the `PDFium`, tokenizer, model, and connection.
///
/// # Errors
///
/// Returns an error if it is not able to initialize a component.
pub fn init(database_path: &str) -> Result<(TextEmbedding, Connection, Pdfium), anyhow::Error> {
    let model = embed::embedding_model()?;
    let conn = storage::load_database(database_path)?;
    let pdfium = pdf::bind()?;
    Ok((model, conn, pdfium))
}

pub type SharedState = Arc<Mutex<AppState>>;
pub struct AppState {
    pub model: TextEmbedding,
    pub conn: Connection,
    pub pdfium: Pdfium,
    pub docs_root: PathBuf,
}

impl AppState {
    #[must_use]
    pub const fn new(
        model: TextEmbedding,
        conn: Connection,
        pdfium: Pdfium,
        docs_root: PathBuf,
    ) -> Self {
        Self {
            model,
            conn,
            pdfium,
            docs_root,
        }
    }
}

/// Starts the Axum server with the given state.
///
/// MCP is also served over streamable HTTP at `/mcp`.
///
/// # Errors
///
/// Returns an error if it is not able to start the server.
pub async fn serve_http(state: SharedState) -> Result<(), anyhow::Error> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    let mcp = DocsMcp {
        state: Arc::clone(&state),
    };
    let mcp_service = StreamableHttpService::new(
        move || Ok(mcp.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .disable_allowed_hosts()
            .disable_allowed_origins(),
    );
    let app = create_app(state)
        .nest_service("/mcp", mcp_service)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::cors::CorsLayer::permissive());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("Listening on 0.0.0.0:8080");
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

/// Starts the MCP server with the given state.
///
/// # Errors
///
/// Returns an error if it is not able to start the server.
pub async fn serve_mcp(state: SharedState) -> Result<(), anyhow::Error> {
    use rmcp::ServiceExt as _;

    let service = DocsMcp { state }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
