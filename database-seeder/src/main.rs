use common::{embed, storage};
use database_seeder::{cc_talk_chunk, init};

fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = std::env::args().collect();
    let pdf_path = args
        .get(1)
        .ok_or("usage: seeder <folder>")
        .map_err(|e| anyhow::anyhow!(e))?;

    let (pdfium, tokenizer, mut model, mut conn) = init("database.db")?;

    let entries = std::fs::read_dir(pdf_path)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        println!("Processing: {}", path.to_string_lossy());
        if path.is_file() {
            let chunks =
                cc_talk_chunk::chunk_document(&pdfium, &path.to_string_lossy(), &tokenizer)?;
            let embeddings = embed::embed_chunks(&mut model, &chunks)?;

            storage::store_chunks(&mut conn, &chunks)?;
            storage::store_embeddings(&mut conn, &chunks, &embeddings)?;
        }
    }

    Ok(())
}
