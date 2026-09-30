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

/// Widths of the margin-box cells in the page-one top band.
fn top_band_cell_widths(boxes: &str) -> Vec<f64> {
    let out = render_html_with_layout(
        &format!(
            "<html><head><style>@page {{ size: Letter; margin: 72pt 54pt; {boxes} }}</style></head><body><p>x</p></body></html>"
        ),
        &HtmlOptions::default(),
    )
    .expect("render");
    let header = out.layout.pages[0]
        .elements
        .iter()
        .find(|e| e.node_type == "FixedHeader")
        .expect("a top band");
    // FixedHeader > band > row > cells
    let row = &header.children[0].children[0];
    row.children.iter().map(|c| c.width).collect()
}

/// A slot that gets no width is not built: an empty zero-width cell paints
/// nothing, and review tools reported it moving and shrinking to a point
/// at the centre of the page. An empty slot that still has width is a
/// spacer that keeps a centre box centred, so it stays.
#[test]
fn zero_width_margin_box_cells_are_not_built() {
    let two = top_band_cell_widths("@top-left { content: \"L\" } @top-right { content: \"R\" }");
    assert_eq!(two.len(), 2, "left + right: two cells, got {two:?}");

    let one = top_band_cell_widths("@top-center { content: \"C\" }");
    assert_eq!(one.len(), 1, "centre alone: one cell, got {one:?}");

    let spacer =
        top_band_cell_widths("@top-center { content: \"C\" } @top-right { content: \"R\" }");
    assert_eq!(
        spacer.len(),
        3,
        "centre + right keeps the empty left spacer, got {spacer:?}"
    );
    assert!(
        (spacer[0] - spacer[2]).abs() < 0.01,
        "the spacer matches the right cell so the centre stays centred, got {spacer:?}"
    );
}
