//! `<a href>` inside a paragraph gets a PDF link annotation (issue #157).
//!
//! The mapper turns an inline `<a>` into a text run carrying `href`. The
//! engine kept that href on the run's glyphs and never read it, so the link
//! was dropped from the PDF while a block-level link (one covering a whole
//! element) worked. These pin the HTML path end to end.

use forme_pdf_html::{render_html, HtmlOptions};

/// Every `/Subtype /Link` annotation's `/Rect` and `/URI`. Annotation
/// dictionaries are written uncompressed, so a byte scan is enough.
fn links(pdf: &[u8]) -> Vec<([f64; 4], Option<String>)> {
    let text = String::from_utf8_lossy(pdf);
    text.split("/Subtype /Link")
        .skip(1)
        .map(|chunk| {
            let dict = &chunk[..chunk.find("endobj").unwrap_or(chunk.len())];
            let r = dict.find("/Rect [").unwrap() + 7;
            let r_end = r + dict[r..].find(']').unwrap();
            let n: Vec<f64> = dict[r..r_end]
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            let uri = dict.find("/URI (").map(|s| {
                let s = s + 6;
                dict[s..s + dict[s..].find(')').unwrap()].to_string()
            });
            ([n[0], n[1], n[2], n[3]], uri)
        })
        .collect()
}

#[test]
fn an_inline_anchor_in_a_paragraph_gets_a_span_sized_annotation() {
    let html = r#"<!DOCTYPE html><html><body>
        <p>See <a href="https://example.com/inline">docs</a> now.</p>
        <p><a href="https://example.com/inline">See docs now.</a></p>
        </body></html>"#;
    let pdf = render_html(html, &HtmlOptions::default())
        .expect("renders")
        .pdf;
    let found = links(&pdf);
    assert_eq!(
        found.len(),
        2,
        "one annotation for the inline link, one for the whole-paragraph link: {found:?}"
    );
    for (_, uri) in &found {
        assert_eq!(uri.as_deref(), Some("https://example.com/inline"));
    }
    // The inline one covers "docs" only; the whole-paragraph one covers
    // "See docs now.", so it is several times wider.
    let widths: Vec<f64> = found.iter().map(|(r, _)| r[2] - r[0]).collect();
    let (narrow, wide) = if widths[0] < widths[1] {
        (widths[0], widths[1])
    } else {
        (widths[1], widths[0])
    };
    assert!(
        narrow > 5.0 && narrow * 2.0 < wide,
        "the inline annotation spans the word, not the line: {widths:?}"
    );
}
