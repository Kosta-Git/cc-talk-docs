use common::{embed, pdf, storage};
use fastembed::TextEmbedding;
use pdfium_render::prelude::Pdfium;
use rusqlite::Connection;
use tokenizers::Tokenizer;

mod chunk_cc_talk;

pub mod cc_talk_chunk {
    pub use crate::chunk_cc_talk::chunk_document;
}

/// Initializes the database and returns the `PDFium`, tokenizer, model, and connection.
///
/// # Errors
///
/// Returns an error if it is not able to initialize a component.
pub fn init(
    database_path: &str,
) -> Result<(Pdfium, Tokenizer, TextEmbedding, Connection), anyhow::Error> {
    let pdfium = pdf::bind()?;
    let tokenizer = chunk_cc_talk::load_tokenizer()?;
    let model = embed::embedding_model()?;
    let conn = storage::load_database(database_path)?;
    Ok((pdfium, tokenizer, model, conn))
}
