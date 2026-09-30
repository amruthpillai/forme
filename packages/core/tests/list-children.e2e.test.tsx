/**
 * End to end, in the reporters' own terms: the text lines a render lays out.
 *
 * #161: in template mode, a `.map()` inside a list rendered no items.
 *
 * The serializer-level tests in packages/react pin the JSON shapes; these pin
 * what the engine then paints from them.
 */
import { describe, it, expect } from 'vitest';
import {
  Document,
  Page,
  Text,
  UnorderedList,
  ListItem,
  serializeTemplate,
  createDataProxy,
} from '@formepdf/react';
import { renderDocumentWithLayout, renderTemplateWithLayout, type ElementInfo } from '../src/index.js';

function textLines(pages: { elements: ElementInfo[] }[]): string[] {
  const out: string[] = [];
  const walk = (els: ElementInfo[]) => {
    for (const el of els) {
      if (el.nodeType === 'TextLine' && el.textContent) out.push(el.textContent);
      walk(el.children);
    }
  };
  for (const p of pages) walk(p.elements);
  return out;
}

describe('#161 .map() inside a list (template vs direct)', () => {
  type Model = { items: string[] };
  const layoutFn = (d: Model) => (
    <Document>
      <Page>
        <UnorderedList>
          {d.items.map((item) => (
            <ListItem key={item}>
              <Text>{item}</Text>
            </ListItem>
          ))}
        </UnorderedList>
      </Page>
    </Document>
  );
  const model = { items: ['one', 'two'] };

  it('template mode renders the same text lines as direct mode', async () => {
    const direct = await renderDocumentWithLayout(layoutFn(model));
    const template = await renderTemplateWithLayout(
      JSON.stringify(serializeTemplate(layoutFn(createDataProxy() as Model))),
      JSON.stringify(model),
    );
    const directLines = textLines(direct.layout.pages);
    expect(directLines.filter((l) => l === 'one' || l === 'two')).toEqual(['one', 'two']);
    expect(textLines(template.layout.pages)).toEqual(directLines);
  });
});
