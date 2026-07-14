use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::{Connection, params};
use sqlite_vec::sqlite3_vec_init;

use crate::chunk::{Chunk, ContentType};

static INIT_SQLITE_EXT: std::sync::Once = std::sync::Once::new();

/// Loads the database from the given path and initializes the vec0 extension.
///
/// # Errors
///
/// Returns an error if the database cannot be loaded or the vec0 extension cannot be initialized.
pub fn load_database(path: &str) -> Result<rusqlite::Connection, anyhow::Error> {
    INIT_SQLITE_EXT.call_once(|| unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Verify the extension is registered before applying vec0 schema objects.
    let _: String = conn.query_row("SELECT vec_version()", [], |row| row.get(0))?;
    init_db(&conn)?;
    Ok(conn)
}

fn init_db(conn: &Connection) -> Result<(), anyhow::Error> {
    conn.execute_batch(include_str!("../schema.sql"))
        .map_err(|e| anyhow::anyhow!(e))
}

/// Stores the given chunks in the database.
///
/// # Errors
///
/// Returns an error if the transaction fails or if the chunks cannot be inserted.
pub fn store_chunks(conn: &mut Connection, chunks: &[Chunk]) -> Result<(), anyhow::Error> {
    let tx = conn.transaction()?;
    {
        let mut statement = tx.prepare_cached(
            "INSERT INTO chunks (
                id, chunk_index, document, part, part_title, doc_version,
                section_number, section_title, breadcrumb, header_number,
                header_name, page_start, page_end, sub_index, sub_total,
                content_type, char_count, token_count, text
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;

        for chunk in chunks {
            let breadcrumb: String = chunk.breadcrumb.join(" :: ");
            let content_type = match chunk.content_type {
                ContentType::Command => "command",
                ContentType::Section => "section",
                ContentType::Table => "table",
                ContentType::Preamble => "preamble",
            };

            statement.execute(params![
                chunk.id,
                i64::try_from(chunk.index)?,
                chunk.document,
                chunk.part,
                chunk.part_title,
                chunk.doc_version,
                chunk.section_number,
                chunk.section_title,
                breadcrumb,
                chunk.header_number,
                chunk.header_name,
                i64::try_from(chunk.page_start)?,
                i64::try_from(chunk.page_end)?,
                i64::try_from(chunk.sub_index)?,
                i64::try_from(chunk.sub_total)?,
                content_type,
                i64::try_from(chunk.char_count)?,
                i64::try_from(chunk.token_count)?,
                chunk.text,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Stores the embeddings for the given chunks in the database.
///
/// # Errors
///
/// Returns an error if the chunk/embedding count mismatch or if the database operation fails.
pub fn store_embeddings(
    conn: &mut Connection,
    chunks: &[Chunk],
    embeddings: &[Vec<f32>],
) -> Result<(), anyhow::Error> {
    anyhow::ensure!(
        chunks.len() == embeddings.len(),
        "chunk/embedding count mismatch: {} chunks, {} embeddings",
        chunks.len(),
        embeddings.len()
    );
    let tx = conn.transaction()?;
    {
        let mut statement =
            tx.prepare("INSERT INTO embeddings (chunk_id, embedding) VALUES (?, ?)")?;
        for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
            anyhow::ensure!(
                embedding.len() == 384,
                "embedding for {} has {} dimensions; expected 384",
                chunk.id,
                embedding.len()
            );
            statement.execute(params![chunk.id, vec_to_blob(embedding)])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub chunk: Chunk,
    pub distance: f64, // cosine distance: 0 = identical, ~1 = unrelated
}

/// Searches the database for the top `k` chunks most similar to the given query.
///
/// # Errors
///
/// Returns an error if the search query fails.
pub fn search(conn: &Connection, query: &[f32], k: usize) -> Result<Vec<SearchHit>, anyhow::Error> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.chunk_index, c.document, c.part, c.part_title,
                    c.doc_version, c.section_number, c.section_title,
                    c.breadcrumb, c.header_number, c.header_name,
                    c.page_start, c.page_end, c.sub_index, c.sub_total,
                    c.content_type, c.char_count, c.token_count, c.text,
                    e.distance
             FROM embeddings e
             JOIN chunks c ON c.id = e.chunk_id
             WHERE e.embedding MATCH ?1 AND k = ?2
             ORDER BY e.distance",
    )?;

    let k = i64::try_from(k)?;
    let hits = stmt
        .query_map(params![vec_to_blob(query), k], |row| {
            let usize_at = |index| {
                let value: i64 = row.get(index)?;
                usize::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
            };
            let content_type = match row.get::<_, String>(15)?.as_str() {
                "command" => ContentType::Command,
                "section" => ContentType::Section,
                "table" => ContentType::Table,
                "preamble" => ContentType::Preamble,
                value => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        15,
                        rusqlite::types::Type::Text,
                        format!("unknown content type: {value}").into(),
                    ));
                }
            };
            Ok(SearchHit {
                chunk: Chunk {
                    id: row.get(0)?,
                    index: usize_at(1)?,
                    document: row.get(2)?,
                    part: row.get(3)?,
                    part_title: row.get(4)?,
                    doc_version: row.get(5)?,
                    section_number: row.get(6)?,
                    section_title: row.get(7)?,
                    breadcrumb: row
                        .get::<_, String>(8)?
                        .split(" :: ")
                        .map(str::to_string)
                        .collect(),
                    header_number: row.get(9)?,
                    header_name: row.get(10)?,
                    page_start: usize_at(11)?,
                    page_end: usize_at(12)?,
                    sub_index: usize_at(13)?,
                    sub_total: usize_at(14)?,
                    content_type,
                    char_count: usize_at(16)?,
                    token_count: usize_at(17)?,
                    text: row.get(18)?,
                },
                distance: row.get(19)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::load_database;

    #[test]
    fn initializes_schema_and_indexes_inserted_chunks() -> Result<(), anyhow::Error> {
        let conn = load_database(":memory:")?;

        conn.execute(
            "INSERT INTO chunks (
                id, chunk_index, document, part, part_title, doc_version,
                section_number, section_title, breadcrumb, header_number,
                header_name, page_start, page_end, sub_index, sub_total,
                content_type, char_count, token_count, text
             ) VALUES (
                'part-1-section-1-0', 0, 'part-1.pdf', 1, 'Part one', '4.7',
                '1', 'Introduction', '[\"Introduction\"]', NULL,
                NULL, 1, 1, 0, 1, 'section', 13, 2, 'Coin protocol'
             )",
            [],
        )?;

        let vector_table_count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_schema
             WHERE type = 'table' AND name = 'embeddings'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(vector_table_count, 1);

        Ok(())
    }
}
