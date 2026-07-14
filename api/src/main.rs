use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use api::{AppState, SharedState, init, serve_http, serve_mcp};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        return Err(anyhow::anyhow!("Usage: {} <http|stdio>", args[0]));
    }
    let mode = &args[1];

    let docs_root: PathBuf = std::env::var("DOCS_ROOT")
        .unwrap_or_else(|_| "./docs".to_string())
        .into();
    let docs_root = docs_root.canonicalize()?;

    let (model, conn, pdfium) = init("database.db")?;

    let state: SharedState = Arc::new(Mutex::new(AppState::new(model, conn, pdfium, docs_root)));
    match mode.as_str() {
        "http" => serve_http(state).await?,
        "stdio" => serve_mcp(state).await?,
        _ => return Err(anyhow::anyhow!("Invalid mode: {mode}")),
    }
    Ok(())
}
