#!/usr/bin/env node
// Print one CHANGELOG.md entry as GitHub release notes.
//
//   node scripts/release-notes.mjs 0.26.0 | gh release create v0.26.0 --notes-file -
//
// The changelog is hard-wrapped at 80 columns, and GitHub renders a single
// newline in a release body as a line break, so a pasted entry shows ragged
// lines. This joins wrapped paragraphs and list items back into one line each,
// and demotes the entry's `###` headings to `##` since the release title
// stands in for the version heading. The words are unchanged.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = process.argv[2];
if (!version) {
  console.error('usage: node scripts/release-notes.mjs <version>');
  process.exit(2);
}

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const lines = readFileSync(join(root, 'CHANGELOG.md'), 'utf8').split('\n');
const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
if (start < 0) {
  console.error(`CHANGELOG.md has no "## [${version}]" entry`);
  process.exit(1);
}
let end = lines.findIndex((l, i) => i > start && l.startsWith('## '));
if (end < 0) end = lines.length;

// A line starts a new block (rather than continuing the previous one) when it
// opens a list item, a heading or a table row.
const startsBlock = /^\s*([-*]|\d+\.)\s|^#+ |^\|/;
const out = [];
let inFence = false;
for (let line of lines.slice(start + 1, end)) {
  if (line.startsWith('```')) {
    inFence = !inFence;
    out.push(line);
    continue;
  }
  if (inFence) {
    out.push(line);
    continue;
  }
  if (line.startsWith('### ')) line = '## ' + line.slice(4);
  const prev = out[out.length - 1];
  const continues =
    prev !== undefined &&
    prev.trim() !== '' &&
    line.trim() !== '' &&
    !prev.startsWith('#') &&
    !prev.startsWith('|') &&
    !startsBlock.test(line);
  if (continues) out[out.length - 1] = prev.trimEnd() + ' ' + line.trim();
  else out.push(line);
}

process.stdout.write(out.join('\n').trim() + '\n');
