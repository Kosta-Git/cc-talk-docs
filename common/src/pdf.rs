use std::range::Range;

use pdfium_render::prelude::*;

/// Binds to the pdfium library, preferring the copy bundled under `./shared`
/// and falling back to a system-installed library.
///
/// pdfium can only be initialised once per process, so the returned [`Pdfium`]
/// instance should be created once and reused for every document.
///
/// # Errors
///
/// Returns an error if neither the bundled nor the system pdfium library can be bound.
pub fn bind() -> Result<Pdfium, PdfiumError> {
    Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./shared"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .map(Pdfium::new)
}

/// Converts a PDF file to text, returning one `String` per page.
///
/// The returned `Vec` is 0-indexed, so element `i` corresponds to the
/// 1-based page number `i + 1`.
///
/// # Errors
///
/// Returns an error if the PDF file cannot be loaded or the text of any page
/// cannot be extracted.
pub fn pdf_to_text(pdfium: &Pdfium, pdf_path: &str) -> Result<Vec<String>, PdfiumError> {
    pdfium
        .load_pdf_from_file(pdf_path, None)?
        .pages()
        .iter()
        .map(|page| Ok(page.text()?.all()))
        .collect()
}

/// Extracts a range of pages from a PDF file, returning one `String` per page.
///
/// The returned `Vec` is 0-indexed, so element `i` corresponds to the
/// 1-based page number `i + 1`.
///
/// # Arguments
///
/// * `pdfium` - A reference to the `PDFium` library.
/// * `pdf_path` - The path to the PDF file to extract pages from.
/// * `page_range` - The range of pages to extract, 1-based (e.g. `1..=3` for pages 1, 2, and 3).
///
/// # Errors
///
/// Returns an error if the PDF file cannot be loaded or the text of any page
/// cannot be extracted.
pub fn extract_pages(
    pdfium: &Pdfium,
    pdf_path: &str,
    page_range: Range<usize>,
) -> Result<Vec<String>, PdfiumError> {
    pdfium
        .load_pdf_from_file(pdf_path, None)?
        .pages()
        .iter()
        .skip(page_range.start - 1)
        .take(page_range.end - page_range.start + 1)
        .map(|page| Ok(page.text()?.all()))
        .collect()
}
