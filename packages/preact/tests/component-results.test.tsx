// Ported from the react adapter's test of the same name: this package is a
// hand-maintained fork of that serializer, so it carries the same bugs.
import { describe, it, expect } from 'vitest';
import {
  Document,
  Page,
  View,
  Text,
  Fixed,
  UnorderedList,
  ListItem,
  serialize,
  serializeTemplate,
  createDataProxy,
} from '../src/index';

// Issue #159: a function component whose result is an ARRAY or a FRAGMENT
// serialized to nothing, because the unknown-component fallback only
// serialized results that passed `isValidElement` (an array fails it) and
// then handed the element to a dispatch with no arm for `Fragment`. Inline
// arrays and fragments worked because child flattening unwraps them, so the
// fix is to give a component's result that same flattening.

type AnyNode = Record<string, unknown>;

/** Every text content string in a serialized tree, in document order. */
function texts(node: unknown): unknown[] {
  const out: unknown[] = [];
  const walk = (n: unknown) => {
    if (Array.isArray(n)) {
      n.forEach(walk);
      return;
    }
    if (n === null || typeof n !== 'object') return;
    const obj = n as AnyNode;
    const kind = obj.kind as AnyNode | undefined;
    if (kind && (kind.type === 'Text' || kind.type === 'Heading')) {
      out.push(kind.content);
    }
    if (obj.children) walk(obj.children);
    if (obj.template) walk(obj.template);
  };
  walk(node);
  return out;
}

const AsArray = () => [<Text key="a">A</Text>, <Text key="b">B</Text>];
const AsFragment = () => (
  <>
    <Text>A</Text>
    <Text>B</Text>
  </>
);
const AsNestedFragment = () => (
  <>
    <AsArray />
    <>
      <Text>C</Text>
    </>
  </>
);

// Both paths must agree, so every case runs through both.
const PATHS = {
  serialize: (el: any) => serialize(el) as unknown as AnyNode,
  serializeTemplate: (el: any) => serializeTemplate(el) as AnyNode,
};

describe.each(Object.entries(PATHS))('#159 component returning array/fragment (%s)', (_name, run) => {
  it('array result as the only child of <Page>', () => {
    const doc = run(<Document><Page><AsArray /></Page></Document>);
    const page = (doc.children as AnyNode[])[0];
    expect(page.children).toHaveLength(2);
    expect(texts(page)).toEqual(['A', 'B']);
  });

  it('fragment result as the only child of <Page>', () => {
    const doc = run(<Document><Page><AsFragment /></Page></Document>);
    const page = (doc.children as AnyNode[])[0];
    expect(page.children).toHaveLength(2);
    expect(texts(page)).toEqual(['A', 'B']);
  });

  it('nested fragments and components flatten in order', () => {
    const doc = run(<Document><Page><AsNestedFragment /></Page></Document>);
    const page = (doc.children as AnyNode[])[0];
    expect(page.children).toHaveLength(3);
    expect(texts(page)).toEqual(['A', 'B', 'C']);
  });

  it('inside <View>, <Fixed> and <ListItem>', () => {
    const doc = run(
      <Document>
        <Page>
          <Fixed position="footer"><AsFragment /></Fixed>
          <View><AsArray /></View>
          <UnorderedList><ListItem><AsFragment /></ListItem></UnorderedList>
        </Page>
      </Document>,
    );
    const [fixed, view, list] = (doc.children as AnyNode[])[0].children as AnyNode[];
    expect(fixed.children).toHaveLength(2);
    expect(view.children).toHaveLength(2);
    const item = (list.children as AnyNode[])[0];
    expect(item.children).toHaveLength(2);
    expect(texts(doc)).toEqual(['A', 'B', 'A', 'B', 'A', 'B']);
  });

  it('a component returning several <Page>s at Document level yields pages', () => {
    const Pages = () => (
      <>
        <Page><Text>one</Text></Page>
        <Page><Text>two</Text></Page>
      </>
    );
    const doc = run(<Document><Pages /></Document>);
    const pages = doc.children as AnyNode[];
    expect(pages).toHaveLength(2);
    expect(pages.map((p) => (p.kind as AnyNode).type)).toEqual(['Page', 'Page']);
    expect(texts(doc)).toEqual(['one', 'two']);
  });

  it('a component returning a single element is unchanged', () => {
    const One = () => <Text>solo</Text>;
    const doc = run(<Document><Page><One /></Page></Document>);
    const page = (doc.children as AnyNode[])[0];
    expect(page.children).toHaveLength(1);
    expect(texts(page)).toEqual(['solo']);
  });

  it('a component returning null contributes nothing', () => {
    const Nothing = () => null;
    const doc = run(<Document><Page><Nothing /><Text>x</Text></Page></Document>);
    const page = (doc.children as AnyNode[])[0];
    expect(page.children).toHaveLength(1);
  });
});

describe('#159 multi-node result where only one node fits (template .map())', () => {
  it('a .map() callback returning a component that returns a fragment keeps both nodes', () => {
    const Pair = ({ label }: { label: string }) => (
      <>
        <Text>{label}</Text>
        <Text>!</Text>
      </>
    );
    const data = createDataProxy() as { items: string[] };
    const doc = serializeTemplate(
      <Document>
        <View>{data.items.map((s: string) => <Pair label={s} />)}</View>
      </Document>,
    );
    const view = (doc.children as AnyNode[])[0];
    const each = (view.children as AnyNode[])[0];
    expect(each.$each).toEqual({ $ref: 'items' });
    // An $each template is ONE node, so the pair is grouped in a View
    // rather than losing its second half.
    const template = each.template as AnyNode;
    expect((template.kind as AnyNode).type).toBe('View');
    expect(texts(template)).toEqual([{ $ref: '$item' }, '!']);
  });
});
