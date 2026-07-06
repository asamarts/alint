// Parity tests for strip-since.mjs. These mirror the Rust unit tests for
// `strip_unreleased_prose` (xtask/src/docs_export/tests.rs) so the standalone
// port cannot drift from the source semantics, and add byte-exact output
// assertions (stronger than the Rust `contains` checks) plus code-fence and
// parseVersion coverage. Run: `node --test ci/scripts/strip-since.test.mjs`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { stripUnreleasedProse, parseVersion } from './strip-since.mjs';

const BODY = [
  'Intro line, always shown.',
  '',
  '<!-- alint:since=0.14 -->',
  '**Optional `root_only`** requires the match to be at the repo root.',
  '<!-- /alint:since -->',
  '',
  'Trailer line, always shown.',
  '',
].join('\n');

test('released 0.13.0: since=0.14 block dropped, markers gone, surrounds kept (byte-exact)', () => {
  const out = stripUnreleasedProse(BODY, [0, 13, 0]);
  assert.equal(out, 'Intro line, always shown.\n\n\nTrailer line, always shown.\n');
  assert.ok(!out.includes('root_only') && !out.includes('alint:since'));
});

test('released 0.14.0: block content kept, markers still stripped (byte-exact)', () => {
  const out = stripUnreleasedProse(BODY, [0, 14, 0]);
  assert.equal(
    out,
    'Intro line, always shown.\n\n**Optional `root_only`** requires the match to be at the repo root.\n\nTrailer line, always shown.\n',
  );
  assert.ok(out.includes('root_only') && !out.includes('alint:since'));
});

test('released null (local/dev): content kept, markers stripped', () => {
  const out = stripUnreleasedProse(BODY, null);
  assert.ok(out.includes('root_only') && !out.includes('alint:since'));
});

test('a body with no markers is returned byte-for-byte (fast path)', () => {
  const plain = 'no markers here\n';
  assert.equal(stripUnreleasedProse(plain, [0, 13, 0]), plain);
  // No trailing newline is likewise preserved untouched by the fast path.
  assert.equal(stripUnreleasedProse('abc', [0, 13, 0]), 'abc');
});

test('markers inside a fenced code block stay literal (not treated as gates)', () => {
  const fenced = [
    'Before.',
    '```md',
    '<!-- alint:since=99.0 -->',
    'inside fence',
    '<!-- /alint:since -->',
    '```',
    'After.',
    '',
  ].join('\n');
  const out = stripUnreleasedProse(fenced, [0, 13, 0]);
  // The example survives verbatim even though 99.0 > 0.13 — it is inside a fence.
  assert.equal(out, fenced);
  assert.ok(out.includes('<!-- alint:since=99.0 -->') && out.includes('inside fence'));
});

test('multiple blocks: each gated independently', () => {
  const body = [
    'A',
    '<!-- alint:since=0.14 -->',
    'future',
    '<!-- /alint:since -->',
    'B',
    '<!-- alint:since=0.13 -->',
    'released',
    '<!-- /alint:since -->',
    'C',
    '',
  ].join('\n');
  const out = stripUnreleasedProse(body, [0, 13, 0]);
  assert.equal(out, 'A\nB\nreleased\nC\n');
});

test('parseVersion: lenient dotted parse matching the Rust tuple', () => {
  assert.deepEqual(parseVersion('0.14'), [0, 14, 0]);
  assert.deepEqual(parseVersion('v0.13.0'), [0, 13, 0]);
  assert.deepEqual(parseVersion('1'), [1, 0, 0]);
  assert.deepEqual(parseVersion('2.5.9'), [2, 5, 9]);
  assert.deepEqual(parseVersion('garbage'), [0, 0, 0]);
  assert.deepEqual(parseVersion(' v1.2 '), [1, 2, 0]);
});
