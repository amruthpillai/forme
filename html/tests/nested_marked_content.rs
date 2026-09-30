//! Tagged output must not open a marked-content sequence with an MCID inside
//! another. Every element used to keep its own sequence open while its
//! children were written, so on the invoice fixture 133 of 134 sequences
//! were nested, nearly all inside empty wrappers (TD, TR, Span, Div). A
//! cell's text then belonged to its TD and to the TR around it at once.

use forme_pdf_html::{render_html, HtmlOptions};

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn content_streams(pdf: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = pdf;
    while let Some(i) = find(rest, b"stream") {
        let after = &rest[i + b"stream".len()..];
        let body = match after.first() {
            Some(b'\r') => &after[2..],
            Some(b'\n') => &after[1..],
            _ => after,
        };
        let end = find(body, b"endstream").unwrap_or(body.len());
        if let Ok(text) = miniz_oxide::inflate::decompress_to_vec_zlib(&body[..end]) {
            let text = String::from_utf8_lossy(&text).into_owned();
            if text.contains(" Tj") || text.contains(" TJ") {
                out.push(text);
            }
        }
        rest = &rest[i + b"stream".len()..];
    }
    out
}

const PAGE: &str = r#"<html><head><style>
  table { border-collapse: collapse } tr.shade { background: #eeeeee }
  td { border: 1px solid #999999; padding: 4pt } .box { border: 1pt solid #333333; padding: 6pt }
</style></head><body>
  <h1>Statement</h1>
  <div class="box"><div><p>Nested paragraph</p></div></div>
  <table>
    <tr class="shade"><td>Item</td><td>Amount</td></tr>
    <tr><td>Widget</td><td>12.00</td></tr>
  </table>
  <ul><li>First</li><li>Second</li></ul>
</body></html>"#;

#[test]
fn no_mcid_sequence_opens_inside_another() {
    let out = render_html(
        PAGE,
        &HtmlOptions {
            tagged: true,
            ..Default::default()
        },
    )
    .expect("render");
    let (mut nested, mut shown, mut untagged_text) = (0, 0, 0);
    for stream in content_streams(&out.pdf) {
        // Each open sequence: does it carry an MCID?
        let mut stack: Vec<bool> = Vec::new();
        for line in stream.lines().map(str::trim) {
            if line.ends_with(" BDC") || line.ends_with(" BMC") {
                let has_mcid = line.contains("/MCID");
                if has_mcid && stack.iter().any(|m| *m) {
                    nested += 1;
                }
                stack.push(has_mcid);
            } else if line == "EMC" {
                stack.pop();
            } else if line.ends_with(" Tj") || line.ends_with(" TJ") {
                shown += 1;
                if !stack.iter().any(|m| *m) {
                    untagged_text += 1;
                }
            }
        }
    }
    assert!(shown > 0, "precondition: text was drawn");
    assert_eq!(nested, 0, "MCID sequences nested inside another");
    assert_eq!(untagged_text, 0, "every text show is still tagged content");

    let raw = String::from_utf8_lossy(&out.pdf);
    for role in ["/S /H1", "/S /Table", "/S /TR", "/S /TD", "/S /L", "/S /LI", "/S /P"] {
        assert!(raw.contains(role), "structure keeps {role}");
    }
}
