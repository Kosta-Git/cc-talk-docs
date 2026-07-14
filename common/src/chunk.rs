/// The kind of content a chunk holds, used as a retrieval filter hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    /// A ccTalk command definition (`Header NNN - …`).
    Command,
    /// A numbered specification section.
    Section,
    /// A section dominated by tabular/numeric rows.
    Table,
    /// Front matter appearing before the first numbered section.
    Preamble,
}

/// A single embedding-ready chunk with full provenance metadata.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Chunk {
    // Identity
    pub id: String,
    pub index: usize,

    // Source document
    pub document: String,
    pub part: u8,
    pub part_title: String,
    pub doc_version: String,

    // Structural location
    pub section_number: Option<String>,
    pub section_title: Option<String>,
    pub breadcrumb: Vec<String>,

    // ccTalk command
    pub header_number: Option<u16>,
    pub header_name: Option<String>,

    // Pagination (1-based)
    pub page_start: usize,
    pub page_end: usize,

    // Sub-splitting bookkeeping
    pub sub_index: usize,
    pub sub_total: usize,

    // Content
    pub content_type: ContentType,
    pub char_count: usize,
    pub token_count: usize,
    pub text: String,
}

impl Chunk {
    #[must_use]
    pub fn embedding_input(&self) -> String {
        format!("passage: {}\n{}", self.breadcrumb.join(" "), self.text)
    }
}
