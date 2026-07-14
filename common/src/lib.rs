pub mod chunk;
pub mod embed;
pub mod pdf;

mod database;

pub mod storage {
    pub use crate::database::SearchHit;
    pub use crate::database::load_database;
    pub use crate::database::search;
    pub use crate::database::store_chunks;
    pub use crate::database::store_embeddings;
}
