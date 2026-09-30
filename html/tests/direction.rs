//! `dir` and CSS `direction` reach the engine. The mapper carried neither,
//! so a `dir="rtl"` paragraph was laid out left to right: left-aligned,
//! where Chrome right-aligns it, and with an LTR base direction for BiDi.

use forme_pdf_html::{render_html_with_layout, HtmlOptions};

/// (left edge, right edge) of the first text line, and the content box's
/// right edge (page 595.28 - 72pt margins).
fn first_line(body: &str) -> (f64, f64) {
    let out = render_html_with_layout(
        &format!(
            "<html><head><style>@page {{ size: A4; margin: 72pt }} body {{ margin: 0 }}</style></head><body>{body}</body></html>"
        ),
        &HtmlOptions::default(),
    )
    .expect("render");
    fn find(els: &[forme::layout::ElementInfo]) -> Option<(f64, f64)> {
        for e in els {
            if e.node_type == "TextLine" {
                return Some((e.x, e.x + e.width));
            }
            if let Some(f) = find(&e.children) {
                return Some(f);
            }
        }
        None
    }
    find(&out.layout.pages[0].elements).expect("a text line")
}

const RIGHT: f64 = 595.28 - 72.0;

#[test]
fn dir_rtl_right_aligns_like_chrome() {
    let (_, right) = first_line(r#"<p dir="rtl">Hello world</p>"#);
    assert!(
        (right - RIGHT).abs() < 1.0,
        "dir=rtl line ends at the right edge, got {right:.1}"
    );
}

#[test]
fn dir_rtl_is_inherited() {
    let (_, right) = first_line(r#"<div dir="rtl"><p>Hello world</p></div>"#);
    assert!((right - RIGHT).abs() < 1.0, "inherited rtl, got {right:.1}");
}

#[test]
fn css_direction_rtl_right_aligns() {
    let (_, right) = first_line(r#"<p style="direction: rtl">Hello world</p>"#);
    assert!(
        (right - RIGHT).abs() < 1.0,
        "direction: rtl, got {right:.1}"
    );
}

/// `dir` is a presentational hint: author CSS overrides it.
#[test]
fn css_direction_overrides_the_dir_attribute() {
    let (left, _) = first_line(r#"<p dir="rtl" style="direction: ltr">Hello world</p>"#);
    assert!(
        (left - 72.0).abs() < 1.0,
        "CSS ltr wins over dir=rtl, got {left:.1}"
    );
}

#[test]
fn no_dir_stays_left_aligned() {
    let (left, _) = first_line("<p>Hello world</p>");
    assert!((left - 72.0).abs() < 1.0, "got {left:.1}");
}
