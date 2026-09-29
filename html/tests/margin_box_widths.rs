//! Margin-box widths follow which boxes on the edge are present (CSS
//! Paged Media 6.3.2). Each box used to be a fixed third, so a lone
//! centre header wider than a third wrapped onto two lines where Chrome
//! keeps it on one: with no side boxes the centre box may use the whole
//! edge. It only fitted before because line breaking compressed its
//! spaces and drew it past its cell.

use forme_pdf_html::{render_html_with_layout, HtmlOptions};

/// Text lines drawn in the top margin of page one.
fn top_margin_lines(html: &str) -> Vec<String> {
    let out = render_html_with_layout(html, &HtmlOptions::default()).expect("render");
    fn collect(els: &[forme::layout::ElementInfo], out: &mut Vec<String>) {
        for e in els {
            if e.node_type == "TextLine" && e.y < 72.0 {
                out.push(e.text_content.clone().unwrap_or_default());
            }
            collect(&e.children, out);
        }
    }
    let mut lines = Vec::new();
    collect(&out.layout.pages[0].elements, &mut lines);
    lines
}

const HEADER: &str = "ACME WIDGET CO. CONFIDENTIAL AND PROPRIETARY";

#[test]
fn a_lone_centre_box_uses_the_whole_edge() {
    let lines = top_margin_lines(&format!(
        "<html><head><style>@page {{ size: Letter; margin: 72pt 54pt;
           @top-center {{ content: \"{HEADER}\"; font-size: 9pt }}
         }}</style></head><body><p>x</p></body></html>"
    ));
    assert_eq!(
        lines,
        vec![HEADER.to_string()],
        "a centre box with no side boxes must stay on one line (Chrome does)"
    );
}

#[test]
fn a_lone_side_box_uses_the_whole_edge() {
    let lines = top_margin_lines(&format!(
        "<html><head><style>@page {{ size: Letter; margin: 72pt 54pt;
           @top-right {{ content: \"{HEADER}\"; font-size: 9pt }}
         }}</style></head><body><p>x</p></body></html>"
    ));
    assert_eq!(lines, vec![HEADER.to_string()]);
}
