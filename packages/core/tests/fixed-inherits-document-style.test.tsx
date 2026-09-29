/**
 * Issue #160, in the reporter's terms: the text lines of a render with a
 * <Fixed> footer and a Document `style`. The body inherited the document
 * font, size and colour; the footer came out Helvetica 12 black. The cause
 * was in the engine (fixed content is laid out again per page without its
 * parent's style), so this runs the real WASM render, not the serializer.
 *
 * Courier stands in for the reporter's Liberation Sans: it is a standard
 * font, so the test needs no font registration, and it differs from the
 * Helvetica fallback the bug produced.
 */
import { describe, it, expect } from 'vitest';
import { Document, Page, Fixed, Text, Watermark } from '@formepdf/react';
import { renderDocumentWithLayout, type ElementInfo } from '../src/index.js';

function lines(pages: { elements: ElementInfo[] }[]): Map<string, ElementInfo> {
  const out = new Map<string, ElementInfo>();
  const walk = (els: ElementInfo[]) => {
    for (const el of els) {
      if (el.nodeType === 'TextLine' && el.textContent) out.set(el.textContent, el);
      walk(el.children);
    }
  };
  for (const p of pages) walk(p.elements);
  return out;
}

const pick = (el: ElementInfo | undefined) =>
  el && { fontFamily: el.style.fontFamily, fontSize: el.style.fontSize, color: el.style.color };

describe('#160 <Fixed> content inherits the Document style', () => {
  it('header and footer text lines match the body line', async () => {
    const { layout } = await renderDocumentWithLayout(
      <Document style={{ fontFamily: 'Courier', fontSize: 9, color: '#cc0000' }}>
        <Page>
          <Fixed position="header">
            <Text>header text</Text>
          </Fixed>
          <Fixed position="footer">
            <Text>footer text</Text>
          </Fixed>
          <Watermark text="DRAFT" />
          <Text>body text</Text>
        </Page>
      </Document>,
    );
    const byText = lines(layout.pages);
    const body = pick(byText.get('body text'));
    expect(body).toMatchObject({ fontFamily: 'Courier', fontSize: 9 });
    expect(pick(byText.get('header text'))).toEqual(body);
    expect(pick(byText.get('footer text'))).toEqual(body);
  });
});
