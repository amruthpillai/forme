//! A margin box inherits from the PAGE context, never from <body>.
//!
//! CSS Paged Media makes the page context the parent of every margin box,
//! and the page context inherits from the root element. So `body { color }`
//! must not reach a running header or footer. Measured in Chrome
//! 2026-09-29 on html/tests/fixtures/report.html: body text paints
//! #1f2430, the @bottom-center page counter paints black.
//!
//! This held by accident while the engine laid Fixed content out with no
//! parent style at all. Once Fixed content inherited its parent's style
//! (#160, which is correct for JSX, where <Fixed> is a child of the
//! document), the bands the mapper nested under <body> picked up body's
//! colour, and every running header and footer changed with it.

use forme::layout::ElementInfo;
use forme_pdf_html::{render_html_with_layout, HtmlOptions};

fn find<'a>(
    els: &'a [ElementInfo],
    pred: &dyn Fn(&ElementInfo) -> bool,
) -> Option<&'a ElementInfo> {
    for e in els {
        if pred(e) {
            return Some(e);
        }
        if let Some(f) = find(&e.children, pred) {
            return Some(f);
        }
    }
    None
}

#[test]
fn a_margin_box_does_not_inherit_body_colour_or_font() {
    let html = r#"<!doctype html><html><head><style>
        @page { size: Letter; margin: 72pt; @bottom-center { content: "FOOTER" } }
        body { color: #cc0000; font-family: Times; font-size: 20pt; }
      </style></head><body><p>BODY</p></body></html>"#;
    let out = render_html_with_layout(html, &HtmlOptions::default()).expect("renders");
    let page = &out.layout.pages[0];

    let line = |needle: &'static str| {
        find(&page.elements, &move |e: &ElementInfo| {
            e.node_type == "TextLine"
                && e.text_content
                    .as_deref()
                    .is_some_and(|t| t.contains(needle))
        })
        .unwrap_or_else(|| panic!("no text line containing {needle:?}"))
    };
    let body = line("BODY");
    let footer = line("FOOTER");

    // Precondition: body really does carry the styles under test, or the
    // footer assertions below would pass for the wrong reason.
    assert!(
        (body.style.color.r - 0.8).abs() < 0.01 && body.style.font_size == 20.0,
        "precondition: body must be red 20pt, got {:?} {}pt",
        body.style.color,
        body.style.font_size
    );

    assert!(
        footer.style.color.r < 0.01 && footer.style.color.g < 0.01 && footer.style.color.b < 0.01,
        "the margin box must not inherit body's colour: got {:?}",
        footer.style.color
    );
    assert!(
        footer.style.font_size != 20.0,
        "the margin box must not inherit body's font size: got {}pt",
        footer.style.font_size
    );
    assert!(
        !footer.style.font_family.contains("Times"),
        "the margin box must not inherit body's font family: got {}",
        footer.style.font_family
    );
}
