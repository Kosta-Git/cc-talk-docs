CREATE TABLE IF NOT EXISTS chunks (
    id              TEXT    PRIMARY KEY,
    chunk_index     INTEGER NOT NULL CHECK (chunk_index >= 0),
    document        TEXT    NOT NULL,
    part            INTEGER NOT NULL CHECK (part BETWEEN 0 AND 255),
    part_title      TEXT    NOT NULL,
    doc_version     TEXT    NOT NULL,
    section_number  TEXT,
    section_title   TEXT,
    breadcrumb      TEXT    NOT NULL,
    header_number   INTEGER CHECK (header_number BETWEEN 0 AND 65535),
    header_name     TEXT,
    page_start      INTEGER NOT NULL CHECK (page_start >= 1),
    page_end        INTEGER NOT NULL CHECK (page_end >= page_start),
    sub_index       INTEGER NOT NULL CHECK (sub_index >= 0),
    sub_total       INTEGER NOT NULL CHECK (sub_total >= 1 AND sub_index < sub_total),
    content_type    TEXT    NOT NULL
                            CHECK (content_type IN
                                   ('command', 'section', 'table', 'preamble')),
    char_count      INTEGER NOT NULL CHECK (char_count >= 0),
    token_count     INTEGER NOT NULL CHECK (token_count >= 0),
    text            TEXT    NOT NULL,
    UNIQUE (document, chunk_index)
);

CREATE INDEX IF NOT EXISTS chunks_document_order_idx
    ON chunks (document, chunk_index);
CREATE INDEX IF NOT EXISTS chunks_header_number_idx
    ON chunks (header_number) WHERE header_number IS NOT NULL;
CREATE INDEX IF NOT EXISTS chunks_section_idx
    ON chunks (part, section_number);
CREATE INDEX IF NOT EXISTS chunks_content_type_idx
    ON chunks (content_type);

-- bge-small-en-v1.5 emits 384-dimensional vectors.
CREATE VIRTUAL TABLE IF NOT EXISTS embeddings USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding FLOAT[384] distance_metric=cosine
);
