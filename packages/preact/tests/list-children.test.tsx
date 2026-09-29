// Ported from the react adapter's test of the same name: this package is a
// hand-maintained fork of that serializer, so it carries the same bugs.
import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  Document,
  Page,
  View,
  Text,
  UnorderedList,
  OrderedList,
  ListItem,
  serialize,
  serializeTemplate,
  createDataProxy,
} from '../src/index';

// Issue #161: lists kept only children whose type was `ListItem`, so the
// template proxy's `.map()` marker (and any component that returns
// ListItems) was filtered out with everything it would have produced.

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

// Both paths must agree, so every case runs through both.
const PATHS = {
  serialize: (el: any) => serialize(el) as unknown as AnyNode,
  serializeTemplate: (el: any) => serializeTemplate(el) as AnyNode,
};

describe.each(Object.entries(PATHS))('#161 list children that expand to ListItems (%s)', (_name, run) => {
  const Items = () => (
    <>
      <ListItem><Text>one</Text></ListItem>
      <ListItem><Text>two</Text></ListItem>
    </>
  );

  it('a component returning ListItems inside <UnorderedList>', () => {
    const doc = run(<Document><Page><UnorderedList><Items /></UnorderedList></Page></Document>);
    const list = ((doc.children as AnyNode[])[0].children as AnyNode[])[0];
    expect(list.children).toHaveLength(2);
    expect((list.children as AnyNode[]).map((c) => (c.kind as AnyNode).type)).toEqual(['ListItem', 'ListItem']);
    expect(texts(list)).toEqual(['one', 'two']);
  });

  it('a component returning ListItems inside <OrderedList> nested in a <ListItem>', () => {
    const doc = run(
      <Document>
        <Page>
          <UnorderedList>
            <ListItem>
              <Text>outer</Text>
              <OrderedList><Items /></OrderedList>
            </ListItem>
          </UnorderedList>
        </Page>
      </Document>,
    );
    expect(texts(doc)).toEqual(['outer', 'one', 'two']);
  });
});

describe('#161 template .map() inside lists', () => {
  it('.map() inside <UnorderedList> produces an $each of ListItems', () => {
    const data = createDataProxy() as { items: string[] };
    const doc = serializeTemplate(
      <Document>
        <Page>
          <UnorderedList>
            {data.items.map((item: string) => (
              <ListItem key={item}>
                <Text>{item}</Text>
              </ListItem>
            ))}
          </UnorderedList>
        </Page>
      </Document>,
    );
    const list = ((doc.children as AnyNode[])[0].children as AnyNode[])[0];
    expect((list.kind as AnyNode).type).toBe('List');
    expect(list.children).toHaveLength(1);
    const each = (list.children as AnyNode[])[0];
    expect(each.$each).toEqual({ $ref: 'items' });
    const template = each.template as AnyNode;
    expect((template.kind as AnyNode).type).toBe('ListItem');
    expect(texts(template)).toEqual([{ $ref: '$item' }]);
  });

  it('.map() inside an <OrderedList> nested in a <ListItem>', () => {
    const data = createDataProxy() as { groups: Array<{ name: string; items: string[] }> };
    const doc = serializeTemplate(
      <Document>
        <Page>
          <UnorderedList>
            {data.groups.map((g: { name: string; items: string[] }) => (
              <ListItem>
                <Text>{g.name}</Text>
                <OrderedList>
                  {g.items.map((i: string) => <ListItem><Text>{i}</Text></ListItem>)}
                </OrderedList>
              </ListItem>
            ))}
          </UnorderedList>
        </Page>
      </Document>,
    );
    const list = ((doc.children as AnyNode[])[0].children as AnyNode[])[0];
    const outer = (list.children as AnyNode[])[0];
    expect(outer.$each).toEqual({ $ref: 'groups' });
    const outerItem = outer.template as AnyNode;
    const inner = (outerItem.children as AnyNode[])[1];
    expect((inner.kind as AnyNode).type).toBe('List');
    const innerEach = (inner.children as AnyNode[])[0];
    expect(innerEach.$each).toEqual({ $ref: '$item.items' });
    expect(((innerEach.template as AnyNode).kind as AnyNode).type).toBe('ListItem');
  });

  it('.map() directly inside a <ListItem> keeps its nodes', () => {
    const data = createDataProxy() as { tags: string[] };
    const doc = serializeTemplate(
      <Document>
        <Page>
          <UnorderedList>
            <ListItem>{data.tags.map((t: string) => <Text>{t}</Text>)}</ListItem>
          </UnorderedList>
        </Page>
      </Document>,
    );
    const list = ((doc.children as AnyNode[])[0].children as AnyNode[])[0];
    const item = (list.children as AnyNode[])[0];
    const each = (item.children as AnyNode[])[0];
    expect(each.$each).toEqual({ $ref: 'tags' });
    expect(texts(each)).toEqual([{ $ref: '$item' }]);
  });

  it('a .map() in a list that yields non-ListItems is dropped with a warning', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      const data = createDataProxy() as { items: string[] };
      const doc = serializeTemplate(
        <Document>
          <Page>
            <UnorderedList>{data.items.map((i: string) => <Text>{i}</Text>)}</UnorderedList>
          </Page>
        </Document>,
      );
      const list = ((doc.children as AnyNode[])[0].children as AnyNode[])[0];
      expect(list.children).toHaveLength(0);
      expect(warn).toHaveBeenCalledTimes(1);
      expect(String(warn.mock.calls[0][0])).toMatch(/UnorderedList.*\.map\(\)/);
    } finally {
      warn.mockRestore();
    }
  });
});

describe.each(Object.entries(PATHS))('#161 what a list still drops, it warns about (%s)', (_name, run) => {
  afterEach(() => vi.restoreAllMocks());

  it('a non-ListItem element in a list is dropped with a warning naming it', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const doc = run(
      <Document>
        <Page>
          <UnorderedList>
            <ListItem><Text>kept</Text></ListItem>
            <View><Text>stray</Text></View>
          </UnorderedList>
        </Page>
      </Document>,
    );
    expect(texts(doc)).toEqual(['kept']);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0][0])).toMatch(/UnorderedList.*View/);
  });

  it('null, booleans and whitespace in a list are dropped without a warning', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    run(
      <Document>
        <Page>
          <OrderedList>
            {null}
            {false}
            {' '}
            <ListItem><Text>kept</Text></ListItem>
          </OrderedList>
        </Page>
      </Document>,
    );
    expect(warn).not.toHaveBeenCalled();
  });
});
