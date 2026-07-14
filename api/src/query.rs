use axum::{
    Json,
    extract::{Query, State},
};
use common::{
    embed,
    storage::{self, SearchHit},
};
use rmcp::schemars;

use crate::{SharedState, error::ApiError};

#[derive(serde::Deserialize, schemars::JsonSchema, Debug)]
pub struct QueryParams {
    /// The natural language query string to search for in the ccTalk docs.
    query: String,
    /// The maximum number of results to return. Defaults to 3.
    limit: Option<usize>,
}

pub async fn query(
    Query(params): Query<QueryParams>,
    State(state): State<SharedState>,
) -> Result<Json<Vec<SearchHit>>, ApiError> {
    search_docs(&params, &state).map(Json)
}

pub fn search_docs(params: &QueryParams, state: &SharedState) -> Result<Vec<SearchHit>, ApiError> {
    let mut state = state
        .try_lock()
        .map_err(|e| ApiError::InternalServerError(anyhow::anyhow!(e.to_string())))?;

    let query_embedding = embed::embed_query(&mut state.model, &params.query)
        .map_err(ApiError::InternalServerError)?;

    storage::search(&state.conn, &query_embedding, params.limit.unwrap_or(3))
        .map_err(ApiError::InternalServerError)
}
