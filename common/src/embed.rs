use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::chunk::Chunk;

/// Returns the embedding model to use for chunk embedding.
///
/// # Errors
///
/// Returns an error if the model fails to initialize.
pub fn embedding_model() -> Result<TextEmbedding, anyhow::Error> {
    TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(true),
    )
}

pub type Embeddings = Vec<Vec<f32>>;

/// Embeds the given chunks using the provided model.
///
/// # Errors
///
/// Returns an error if the model fails to embed the chunks.
pub fn embed_chunks(
    model: &mut TextEmbedding,
    chunks: &[Chunk],
) -> Result<Embeddings, anyhow::Error> {
    let embeddings = model.embed(
        chunks
            .iter()
            .map(Chunk::embedding_input)
            .collect::<Vec<String>>(),
        Some(32),
    )?;
    anyhow::ensure!(
        embeddings.len() == chunks.len(),
        "embedding count mismatch: expected {}, got {}",
        chunks.len(),
        embeddings.len()
    );
    debug_assert!(
        embeddings.iter().all(|v| {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            (n - 1.0).abs() < 1e-3
        }),
        "embeddings not L2-normalized"
    );
    Ok(embeddings)
}

/// Embeds the given query using the provided model.
///
/// # Errors
///
/// Returns an error if the model fails to embed the query.
pub fn embed_query(model: &mut TextEmbedding, query: &str) -> Result<Vec<f32>, anyhow::Error> {
    let embeddings = model.embed(vec![format!("query: {query}")], None)?;
    anyhow::ensure!(
        embeddings.len() == 1,
        "embedding count mismatch: expected 1, got {}",
        embeddings.len()
    );
    Ok(embeddings[0].clone())
}
