use std::{
    path::{Path, PathBuf},
    range::Range,
};

use axum::{
    Json,
    extract::{Query, State},
};
use common::pdf;
use rmcp::schemars;

use crate::{SharedState, error::ApiError};

#[derive(serde::Deserialize, schemars::JsonSchema, Debug)]
pub struct DocumentParams {
    /// The path to the document to extract pages from.
    /// Can be without the `.pdf` extension.
    document: String,
    /// The starting page index (1-based).
    page_start: usize,
    /// The number of pages to extract.
    /// If not specified, defaults to 1.
    count: Option<usize>,
}

pub async fn document(
    Query(params): Query<DocumentParams>,
    State(state): State<SharedState>,
) -> Result<Json<Vec<String>>, ApiError> {
    get_document(&params, &state).map(Json)
}

pub fn get_document(params: &DocumentParams, state: &SharedState) -> Result<Vec<String>, ApiError> {
    let state = state
        .try_lock()
        .map_err(|e| ApiError::InternalServerError(anyhow::anyhow!(e.to_string())))?;

    let document = resolve_document(&state.docs_root, &params.document)?;
    let document = document
        .to_str()
        .ok_or_else(|| ApiError::BadRequest(anyhow::anyhow!("invalid path")))?;
    let page_range = Range::from(params.page_start..params.page_start + params.count.unwrap_or(1));

    pdf::extract_pages(&state.pdfium, document, page_range)
        .map_err(|e| ApiError::InternalServerError(anyhow::anyhow!(e.to_string())))
}

fn resolve_document(docs_root: &Path, requested: &str) -> Result<PathBuf, ApiError> {
    let mut requested = PathBuf::from(requested);

    if requested.extension().is_none() {
        requested.set_extension("pdf");
    }

    if requested.is_absolute()
        || requested.components().count() != 1
        || requested.extension().and_then(|ext| ext.to_str()) != Some("pdf")
    {
        return Err(ApiError::BadRequest(anyhow::anyhow!(
            "invalid document name"
        )));
    }

    let path = docs_root
        .join(requested)
        .canonicalize()
        .map_err(|_| ApiError::NotFound)?;

    if !path.starts_with(docs_root) || !path.is_file() {
        return Err(ApiError::NotFound);
    }

    Ok(path)
}
