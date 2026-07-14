//! Structure-aware chunking of the ccTalk documentation for embedding.
//!
//! The ccTalk specification is split into four parts. Each part is a sequence of
//! numbered sections (`3.2`, `7.8.1`, …) and, in the command specification
//! (Part 2), each command is a numbered section whose title takes the form
//! `Header NNN - Command name`. These sections are self-contained semantic units,
//! so we chunk *by section*: one chunk per section when it fits the token budget,
//! and a token-bounded sub-split (with overlap and a metadata breadcrumb prefix)
//! for the rare oversized section.
//!
//! The output is a `Vec<Chunk>` carrying rich provenance metadata, serialisable to
//! JSONL for the downstream embedding step (bge-small-en-v1.5 via fastembed).

use std::io::{self};
use std::path::Path;
use std::sync::LazyLock;

use common::chunk::{Chunk, ContentType};
use pdfium_render::prelude::{Pdfium, PdfiumError};
use regex::Regex;
use text_splitter::{ChunkConfig, TextSplitter};
use tokenizers::Tokenizer;

use crate::pdf;

/// Target token budget for a chunk. Sections larger than this are sub-split.
/// Chosen well under bge-small-en-v1.5's 512-token hard cap for sharper vectors.
const MAX_TOKENS: usize = 256;
/// Token overlap between adjacent sub-chunks (~15% of `MAX_TOKENS`).
const OVERLAP_TOKENS: usize = 38;
/// The absolute model limit; every emitted chunk must stay at or below this.
pub const _MODEL_TOKEN_LIMIT: usize = 512;
/// Minimum trimmed length for front-matter (preamble) to be worth emitting.
const MIN_PREAMBLE_CHARS: usize = 200;

/// Section headings: multi-level numbers (`3.2`, `7.8.1`, `1.1.1.`) or a
/// single-level number *with a trailing period* (`3.`). Requiring the trailing
/// period on single-level numbers avoids matching prose such as
/// "1 or more - maximum number of coins". Group 1 = number, group 2 = title.
static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(\d+(?:\.\d+)+\.?|\d+\.)\s+(\S.*?)\s*$").expect("valid section regex")
});

/// Extracts the ccTalk command header from a section title of the form
/// `Header NNN - Command name`. Group 1 = header number, group 2 = command name.
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Header\s+(\d{1,3})\s*[-\x{2013}]\s*(.+?)\s*$").expect("valid header regex")
});

/// Repeated page header/footer and cover boilerplate (matched against a trimmed
/// line, anchored at its start) that must never enter a chunk or its embedding.
static BOILERPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^(?:",
        r"public domain document",
        r"|cctalk generic specification\b.*page\s+\d+\s+of\s+\d+",
        r"|while every effort has been made",
        r"|accepted or implied for any errors",
        r"|crane payment solutions (?:does not|shall not)",
        r"|contained within this document",
        r"|one issue to the next",
        r"|arising out of the adherence",
        r"|issue\s+\d+\.\d+\s*$",
        r")"
    ))
    .expect("valid boilerplate regex")
});

/// Table-of-contents entries: dot-leaders followed by a page number.
static TOC_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.{4,}\s*\d+\s*$").expect("valid toc regex"));

/// Parses the part number and version out of a filename stem such as
/// `cctalk-part-3-v4-7`.
static FILENAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"part-(\d+)-v(\d+)-(\d+)").expect("valid filename regex"));

/// Errors that can occur while chunking a document.
#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    /// A pdfium / PDF loading or extraction failure.
    #[error("pdf error: {0:?}")]
    Pdf(#[from] PdfiumError),
    /// The tokenizer file could not be loaded.
    #[error("tokenizer error: {0}")]
    Tokenizer(String),
    /// Invalid splitter configuration (e.g. overlap ≥ capacity).
    #[error("chunk config error: {0}")]
    Config(String),
    /// An I/O failure while writing output.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Loads the bundled bge-small-en-v1.5 tokenizer.
///
/// Looks for `./shared/bge-small-en-v1.5-tokenizer.json` (overridable with the
/// `CCTALK_TOKENIZER` environment variable), mirroring how `pdf.rs` locates the
/// pdfium library under `./shared`.
///
/// # Errors
///
/// Returns [`ChunkError::Tokenizer`] if the tokenizer file cannot be read or parsed.
pub fn load_tokenizer() -> Result<Tokenizer, ChunkError> {
    let path = std::env::var("CCTALK_TOKENIZER")
        .unwrap_or_else(|_| "./shared/bge-small-en-v1.5-tokenizer.json".to_owned());
    Tokenizer::from_file(&path).map_err(|e| ChunkError::Tokenizer(format!("{path}: {e}")))
}

/// Per-document metadata derived from the source filename.
struct DocMeta {
    document: String,
    part: u8,
    part_title: String,
    doc_version: String,
}

fn part_title(part: u8) -> String {
    match part {
        1 => "Introduction & General Description",
        2 => "Command Specification",
        3 => "Appendices & Command Cross-Reference",
        4 => "FAQ & Application Notes",
        _ => "ccTalk Generic Specification",
    }
    .to_owned()
}

fn doc_meta(pdf_path: &Path) -> DocMeta {
    let stem = pdf_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (part, doc_version) = FILENAME_RE.captures(&stem).map_or_else(
        || (0_u8, "unknown".to_owned()),
        |caps| {
            let part = caps
                .get(1)
                .and_then(|m| m.as_str().parse::<u8>().ok())
                .unwrap_or(0);
            let version = match (caps.get(2), caps.get(3)) {
                (Some(major), Some(minor)) => format!("{}.{}", major.as_str(), minor.as_str()),
                _ => "unknown".to_owned(),
            };
            (part, version)
        },
    );

    DocMeta {
        document: stem,
        part,
        part_title: part_title(part),
        doc_version,
    }
}

/// Maps byte offsets in the concatenated document text back to 1-based page numbers.
struct PageMap {
    /// `starts[i]` is the byte offset at which page `i + 1` begins.
    starts: Vec<usize>,
}

impl PageMap {
    /// Returns the 1-based page number containing the given byte offset.
    fn page_at(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i.max(1),
        }
    }
}

/// Removes boilerplate, table-of-contents and blank lines from a raw page.
fn clean_page(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            let probe = trimmed.trim_start();
            if probe.is_empty() || BOILERPLATE_RE.is_match(probe) || TOC_LINE_RE.is_match(probe) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cleans every page and concatenates them, recording per-page start offsets.
fn concatenate(pages: &[String]) -> (String, PageMap) {
    let mut text = String::new();
    let mut starts = Vec::with_capacity(pages.len());
    for page in pages {
        starts.push(text.len());
        text.push_str(&clean_page(page));
        text.push('\n');
    }
    (text, PageMap { starts })
}

/// A detected section, spanning `[start, end)` byte offsets in the cleaned text.
struct Segment {
    number: Option<String>,
    title: Option<String>,
    start: usize,
    end: usize,
    header_number: Option<u16>,
    header_name: Option<String>,
    breadcrumb: Vec<String>,
}

fn parse_header(title: &str) -> (Option<u16>, Option<String>) {
    HEADER_RE.captures(title).map_or((None, None), |caps| {
        let num = caps.get(1).and_then(|m| m.as_str().parse::<u16>().ok());
        let name = caps.get(2).map(|m| m.as_str().trim().to_owned());
        (num, name)
    })
}

/// Builds the ordered list of segments (optional leading preamble + every
/// numbered section) with breadcrumb heading paths and body ranges.
fn build_outline(text: &str) -> Vec<Segment> {
    // Collect raw heading matches with their positions.
    let mut heads: Vec<(String, String, usize)> = Vec::new();
    for caps in SECTION_RE.captures_iter(text) {
        let (Some(whole), Some(num), Some(title)) = (caps.get(0), caps.get(1), caps.get(2)) else {
            continue;
        };
        let number = num.as_str().trim_end_matches('.').to_owned();
        heads.push((number, title.as_str().trim().to_owned(), whole.start()));
    }

    let mut segments = Vec::new();

    // Preamble: everything before the first heading.
    let first_start = heads.first().map_or(text.len(), |(_, _, s)| *s);
    if text[..first_start].trim().len() >= MIN_PREAMBLE_CHARS {
        segments.push(Segment {
            number: None,
            title: Some("Preamble".to_owned()),
            start: 0,
            end: first_start,
            header_number: None,
            header_name: None,
            breadcrumb: vec!["Preamble".to_owned()],
        });
    }

    let mut stack: Vec<(usize, String)> = Vec::new();
    for (i, (number, title, start)) in heads.iter().enumerate() {
        let depth = number.matches('.').count() + 1;
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            stack.pop();
        }
        stack.push((depth, format!("{number} {title}")));
        let breadcrumb = stack.iter().map(|(_, label)| label.clone()).collect();
        let end = heads.get(i + 1).map_or(text.len(), |(_, _, s)| *s);
        let (header_number, header_name) = parse_header(title);
        segments.push(Segment {
            number: Some(number.clone()),
            title: Some(title.clone()),
            start: *start,
            end,
            header_number,
            header_name,
            breadcrumb,
        });
    }

    segments
}

/// Heuristic: a segment dominated by short numeric/tabular rows.
fn looks_like_table(text: &str) -> bool {
    let rows: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if rows.len() < 4 {
        return false;
    }
    let numeric = rows
        .iter()
        .filter(|l| {
            l.starts_with(|c: char| c.is_ascii_digit()) && l.split_whitespace().count() <= 8
        })
        .count();
    numeric * 2 > rows.len()
}

const fn base_content_type(seg: &Segment) -> ContentType {
    if seg.header_number.is_some() {
        ContentType::Command
    } else if seg.number.is_none() {
        ContentType::Preamble
    } else {
        ContentType::Section
    }
}

fn count_tokens(tokenizer: &Tokenizer, text: &str) -> usize {
    tokenizer
        .encode(text, true)
        .map_or_else(|_| text.split_whitespace().count() * 4 / 3, |enc| enc.len())
}

/// A stable, globally unique base id for a segment. The `(part, ordinal)` pair
/// guarantees uniqueness even when section numbers repeat within a document
/// (e.g. version numbers in a revision-history table); the header/section
/// suffix keeps it human-readable.
fn base_id(meta: &DocMeta, seg: &Segment, ordinal: usize) -> String {
    let suffix = seg.header_number.map_or_else(
        || {
            seg.number.as_ref().map_or_else(
                || "preamble".to_owned(),
                |n| format!("s{}", n.replace('.', "_")),
            )
        },
        |h| format!("h{h}"),
    );
    format!("part{}-{ordinal:04}-{suffix}", meta.part)
}

/// Compact context line prepended to every sub-chunk so it stays self-describing.
fn breadcrumb_prefix(meta: &DocMeta, seg: &Segment, sub_index: usize, sub_total: usize) -> String {
    let path = seg
        .breadcrumb
        .last()
        .cloned()
        .or_else(|| seg.title.clone())
        .unwrap_or_default();
    format!(
        "[Part {} | {} | {}/{}]",
        meta.part,
        path,
        sub_index + 1,
        sub_total
    )
}

/// Shared, read-only context threaded through the per-segment emit path.
struct Ctx<'a> {
    text: &'a str,
    map: &'a PageMap,
    meta: &'a DocMeta,
    tokenizer: &'a Tokenizer,
}

/// Builds the final `Chunk` records for one segment, sub-splitting if oversized.
fn emit_segment(
    seg: &Segment,
    ordinal: usize,
    ctx: &Ctx,
    next_index: &mut usize,
    out: &mut Vec<Chunk>,
) -> Result<(), ChunkError> {
    let slice = &ctx.text[seg.start..seg.end];
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    // Absolute offset of the trimmed body within the full text.
    let lead = slice.len() - slice.trim_start().len();
    let abs_start = seg.start + lead;

    let base = base_content_type(seg);
    let id = base_id(ctx.meta, seg, ordinal);

    if count_tokens(ctx.tokenizer, trimmed) <= MAX_TOKENS {
        let page_start = ctx.map.page_at(abs_start);
        let page_end = ctx
            .map
            .page_at((abs_start + trimmed.len()).saturating_sub(1));
        out.push(build_chunk(
            seg,
            ctx.meta,
            ctx.tokenizer,
            &id,
            base,
            trimmed.to_owned(),
            0,
            1,
            page_start,
            page_end,
            *next_index,
        ));
        *next_index += 1;
        return Ok(());
    }

    // Oversized: sub-split the trimmed slice on token boundaries with overlap.
    let config = ChunkConfig::new(MAX_TOKENS)
        .with_sizer(ctx.tokenizer)
        .with_overlap(OVERLAP_TOKENS)
        .map_err(|e| ChunkError::Config(e.to_string()))?;
    let splitter = TextSplitter::new(config);

    let pieces: Vec<(usize, String)> = splitter
        .chunk_indices(trimmed)
        .map(|(offset, piece)| (offset, piece.to_owned()))
        .collect();
    let sub_total = pieces.len();

    for (sub_index, (offset, piece)) in pieces.into_iter().enumerate() {
        let abs = abs_start + offset;
        let page_start = ctx.map.page_at(abs);
        let page_end = ctx.map.page_at((abs + piece.len()).saturating_sub(1));
        let body = format!(
            "{}\n{}",
            breadcrumb_prefix(ctx.meta, seg, sub_index, sub_total),
            piece
        );
        out.push(build_chunk(
            seg,
            ctx.meta,
            ctx.tokenizer,
            &id,
            base,
            body,
            sub_index,
            sub_total,
            page_start,
            page_end,
            *next_index,
        ));
        *next_index += 1;
    }

    Ok(())
}

/// Assembles one `Chunk`, classifying tables and computing counts.
#[allow(clippy::too_many_arguments)]
fn build_chunk(
    seg: &Segment,
    meta: &DocMeta,
    tokenizer: &Tokenizer,
    id: &str,
    base: ContentType,
    body: String,
    sub_index: usize,
    sub_total: usize,
    page_start: usize,
    page_end: usize,
    chunk_index: usize,
) -> Chunk {
    let content_type = if base == ContentType::Section && looks_like_table(&body) {
        ContentType::Table
    } else {
        base
    };
    Chunk {
        id: format!("{id}-{sub_index}"),
        index: chunk_index,
        document: meta.document.clone(),
        part: meta.part,
        part_title: meta.part_title.clone(),
        doc_version: meta.doc_version.clone(),
        section_number: seg.number.clone(),
        section_title: seg.title.clone(),
        breadcrumb: seg.breadcrumb.clone(),
        header_number: seg.header_number,
        header_name: seg.header_name.clone(),
        page_start,
        page_end,
        sub_index,
        sub_total,
        content_type,
        char_count: body.chars().count(),
        token_count: count_tokens(tokenizer, &body),
        text: body,
    }
}

/// Chunks a single ccTalk PDF into embedding-ready records.
///
/// `chunk_index` values are assigned per-document starting at 0; callers
/// processing multiple documents should renumber them globally.
///
/// # Errors
///
/// Returns [`ChunkError`] if the PDF cannot be read or the splitter cannot be configured.
pub fn chunk_document(
    pdfium: &Pdfium,
    pdf_path: &str,
    tokenizer: &Tokenizer,
) -> Result<Vec<Chunk>, ChunkError> {
    let pages = pdf::pdf_to_text(pdfium, pdf_path)?;
    let (text, map) = concatenate(&pages);
    let meta = doc_meta(Path::new(pdf_path));
    let segments = build_outline(&text);

    let ctx = Ctx {
        text: &text,
        map: &map,
        meta: &meta,
        tokenizer,
    };
    let mut chunks = Vec::new();
    let mut index = 0;
    for (ordinal, seg) in segments.iter().enumerate() {
        emit_segment(seg, ordinal, &ctx, &mut index, &mut chunks)?;
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_page_strips_boilerplate_and_toc() {
        let raw = "Public Domain Document\n\
                   ccTalk Generic Specification - Crane Payment Solutions - Page 7 of 80 - ccTalk Part 2 v4.7.doc\n\
                   While every effort has been made to ensure the accuracy of this document\n\
                   accepted or implied for any errors or omissions that are contained herein.\n\
                   3.2 Header 254 - Simple poll\n\
                   Transmitted data : <none>\n\
                   1.1 Core Commands....................................................................3";
        let cleaned = clean_page(raw);
        assert!(cleaned.contains("Header 254 - Simple poll"));
        assert!(cleaned.contains("Transmitted data"));
        assert!(!cleaned.contains("Public Domain"));
        assert!(!cleaned.contains("Page 7 of 80"));
        assert!(!cleaned.contains("While every effort"));
        assert!(!cleaned.contains("Core Commands"));
    }

    #[test]
    fn parse_header_extracts_number_and_name() {
        assert_eq!(
            parse_header("Header 254 - Simple poll"),
            (Some(254), Some("Simple poll".to_owned()))
        );
        assert_eq!(parse_header("Notation"), (None, None));
    }

    #[test]
    fn section_regex_ignores_prose_numbers() {
        assert!(SECTION_RE.is_match("3.2 Header 254 - Simple poll"));
        assert!(SECTION_RE.is_match("7.8.1 Add for Sorter / Diverter Support"));
        assert!(SECTION_RE.is_match("3. Command List"));
        assert!(!SECTION_RE.is_match("1 or more - maximum number of coins"));
        assert!(!SECTION_RE.is_match("B0 - Coin Type 1"));
    }

    #[test]
    fn build_outline_tracks_breadcrumb_depth() {
        let text = "3. Command List\nintro text here\n\
                    3.1 Header 255 - Factory set-up and test\nbody\n\
                    3.2 Header 254 - Simple poll\nbody two\n";
        let segments = build_outline(text);
        let simple = segments
            .iter()
            .find(|s| s.header_number == Some(254))
            .expect("simple poll segment");
        assert_eq!(simple.header_name.as_deref(), Some("Simple poll"));
        assert_eq!(
            simple.breadcrumb,
            vec![
                "3 Command List".to_owned(),
                "3.2 Header 254 - Simple poll".to_owned()
            ]
        );
    }

    #[test]
    fn page_map_resolves_pages_across_seam() {
        let map = PageMap {
            starts: vec![0, 10, 25],
        };
        assert_eq!(map.page_at(0), 1);
        assert_eq!(map.page_at(9), 1);
        assert_eq!(map.page_at(10), 2);
        assert_eq!(map.page_at(24), 2);
        assert_eq!(map.page_at(25), 3);
        assert_eq!(map.page_at(100), 3);
    }
}
