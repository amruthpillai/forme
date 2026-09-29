/**
 * End to end, in the reporters' own terms: the text lines a render lays out.
 *
 * #159: a function component returning an array or a fragment rendered no
 *       text at all.
 *
 * The serializer-level tests in packages/react pin the JSON shapes; these pin
 * what the engine then paints from them.
 */
import { describe, it, expect } from 'vitest';
import {
  Document,
  Page,
  View,
  Text,
} from '@formepdf/react';
import { renderDocumentWithLayout, type ElementInfo } from '../src/index.js';

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

describe('#159 component results render (renderDocumentWithLayout)', () => {
  const AsArray = () => [<Text key="a">A</Text>, <Text key="b">B</Text>];
  const AsFragment = () => (
    <>
      <Text>A</Text>
      <Text>B</Text>
    </>
  );
  const AsView = () => (
    <View>
      <Text>A</Text>
      <Text>B</Text>
    </View>
  );

  for (const [name, C] of [['array', AsArray], ['fragment', AsFragment], ['view (control)', AsView]] as const) {
    it(`a component returning ${name}`, async () => {
      const { layout } = await renderDocumentWithLayout(<Document><Page><C /></Page></Document>);
      expect(textLines(layout.pages)).toEqual(['A', 'B']);
    });
  }
});
