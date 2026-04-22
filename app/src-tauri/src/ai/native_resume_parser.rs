//! Native resume text extraction for imported resume files.
//!
//! Dispatches on the file extension:
//!   .pdf   → `pdf-extract` crate (pure-Rust text extraction)
//!   .docx  → unzip archive, walk `word/document.xml`, concatenate `<w:t>` text nodes
//!   .txt   → plain UTF-8 read
//!
//! All paths run on tokio's blocking pool since they perform CPU-bound work
//! and/or synchronous file I/O.

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::io::Read;
use std::path::{Path, PathBuf};
use tokio::task;

/// Extract plain text from a resume file located at `path`.
///
/// Runs on the blocking thread pool. Returns a user-facing error message on
/// failure — callers can surface it directly or fall back to an HTTP service.
pub async fn extract_text(path: PathBuf) -> Result<String, String> {
    task::spawn_blocking(move || extract_text_sync(&path))
        .await
        .map_err(|e| format!("join error: {e}"))?
}

fn extract_text_sync(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("file does not exist: {}", path.display()));
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => extract_pdf(path),
        "docx" => extract_docx(path),
        "txt" => extract_plain_text(path),
        other => Err(format!(
            "unsupported file type '{other}'. Supported: .pdf, .docx, .txt"
        )),
    }
}

fn extract_pdf(path: &Path) -> Result<String, String> {
    pdf_extract::extract_text(path)
        .map_err(|e| format!("pdf-extract failed: {e}"))
        .map(normalize_whitespace)
}

fn extract_plain_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("read failed: {e}"))
        .map(normalize_whitespace)
}

/// Extract text from a DOCX file.
///
/// DOCX is a ZIP archive containing `word/document.xml`. Inside that XML,
/// every visible text run lives inside a `<w:t>` element. We walk the XML
/// with quick-xml and concatenate all `<w:t>` text contents. Paragraphs
/// (`<w:p>`) and line breaks (`<w:br>`) are translated to newlines for
/// readability.
fn extract_docx(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open failed: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip open failed: {e}"))?;

    let mut xml_bytes = Vec::new();
    {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|e| format!("docx missing word/document.xml: {e}"))?;
        entry
            .read_to_end(&mut xml_bytes)
            .map_err(|e| format!("read docx xml failed: {e}"))?;
    }

    let mut reader = Reader::from_reader(xml_bytes.as_slice());
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text_node = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"w:t" => in_text_node = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"w:t" => in_text_node = false,
                    b"w:p" => out.push('\n'),
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"w:br" {
                    out.push('\n');
                }
            }
            Ok(Event::Text(t)) => {
                if in_text_node {
                    let text = t
                        .unescape()
                        .map_err(|e| format!("xml unescape failed: {e}"))?;
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("docx xml parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(normalize_whitespace(out))
}

/// Collapse runs of whitespace while preserving paragraph breaks. The
/// downstream embedder normalizes further, but keeping blank lines here
/// helps humans read the raw text stored in the resume profile.
fn normalize_whitespace(raw: String) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_blank_line = false;
    for line in raw.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            if !last_was_blank_line && !out.is_empty() {
                out.push('\n');
                last_was_blank_line = true;
            }
        } else {
            out.push_str(&trimmed);
            out.push('\n');
            last_was_blank_line = false;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_collapses_runs() {
        // Multiple spaces collapse to one; multiple blank lines collapse to a
        // single paragraph break; trailing whitespace is trimmed.
        let input = "hello    world\n\n\n\nfoo bar   baz\n".to_string();
        let got = normalize_whitespace(input);
        assert_eq!(got, "hello world\n\nfoo bar baz");
    }

    #[test]
    fn normalize_keeps_single_newlines() {
        let input = "line one\nline two\nline three\n".to_string();
        let got = normalize_whitespace(input);
        assert_eq!(got, "line one\nline two\nline three");
    }

    #[test]
    fn unsupported_extension_errors() {
        let err = extract_text_sync(Path::new("/tmp/fake.xyz")).unwrap_err();
        assert!(err.contains("unsupported") || err.contains("does not exist"));
    }

    #[test]
    fn extensionless_files_are_rejected() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("resume");
        std::fs::write(&path, "plain text").expect("write extensionless resume");

        let err = extract_text_sync(&path).expect_err("extensionless files must be rejected");
        assert!(err.contains("unsupported file type"));
    }
}
