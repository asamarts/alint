#!/usr/bin/env node
// Strip release-gated prose blocks from Markdown, in place.
//
// A block delimited by `<!-- alint:since=X -->` ... `<!-- /alint:since -->` is
// DROPPED when its version X is newer than the released version passed on the
// command line (the prose describes an unreleased capability); otherwise the
// block content is KEPT. The marker comments themselves are ALWAYS removed, so
// they never reach a published page. Markers inside a fenced code example stay
// literal.
//
// WHY THIS EXISTS (docs-bundle.yml): the docs bundle is built by the latest
// RELEASE TAG's docs-export, which predates the P-REF stripper, so it cannot
// gate the `docs/site/reference` (+ design) docs that docs-bundle.yml overlays
// from main. This script runs right after that overlay and before the tag's
// docs-export, keeping unreleased since-blocks out of the bundle (and off the
// live site). Once the stripper ships in a release tag the tag's own
// docs-export does this and the step is idempotent.
//
// MIRROR: this is a line-for-line port of `strip_unreleased_prose`
// (xtask/src/docs_export.rs) + `parse_version` (xtask/src/rule_options_table.rs).
// Keep the two in sync; ci/scripts/strip-since.test.mjs pins the same cases the
// Rust unit tests cover. ADR-0007; docs/design/v0.14/documentation-drift.md P-REF.

import { readdirSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const SINCE_OPEN = '<!-- alint:since=';
const SINCE_CLOSE = '<!-- /alint:since -->';

/**
 * Parse a dotted version string (`"0.14"`, `"v0.13.0"`) into a
 * `[major, minor, patch]` tuple, treating a missing or unparsable component as
 * 0. Lenient by design, exactly like the Rust `parse_version`.
 */
export function parseVersion(s) {
  const parts = s.trim().replace(/^v/, '').split('.');
  const at = (i) => {
    const n = Number.parseInt((parts[i] ?? '').trim(), 10);
    return Number.isNaN(n) ? 0 : n;
  };
  return [at(0), at(1), at(2)];
}

/** `a > b` for `[major, minor, patch]` tuples (lexicographic). */
function versionGt(a, b) {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
}

/**
 * Strip unreleased since-blocks from a markdown body. `released` is a
 * `[major, minor, patch]` tuple or `null` (null keeps every block, only
 * unwrapping the markers). Returns the transformed body.
 */
export function stripUnreleasedProse(body, released) {
  // Fast path: almost every file carries no marker, so leave it byte-for-byte
  // untouched (no line-ending normalisation), matching the Rust fast path.
  if (!body.includes('alint:since')) return body;

  // Rust's `str::lines()` splits on `\n`/`\r\n`, drops the `\r`, and does NOT
  // yield a trailing empty element for a final newline. Replicate that so the
  // output is byte-identical.
  const lines = body.split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') lines.pop();

  const out = [];
  let inCodeFence = false;
  let dropping = false;
  for (const raw of lines) {
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    const trimmed = line.trim();
    if (trimmed.startsWith('```')) {
      inCodeFence = !inCodeFence;
      if (!dropping) out.push(line);
      continue;
    }
    if (!inCodeFence) {
      if (trimmed.startsWith(SINCE_OPEN)) {
        const ver = trimmed.slice(SINCE_OPEN.length).trim().replace(/(?:-->)+$/, '').trim();
        dropping = released !== null && versionGt(parseVersion(ver), released);
        continue; // never emit the marker line itself
      }
      if (trimmed === SINCE_CLOSE) {
        dropping = false;
        continue;
      }
    }
    if (!dropping) out.push(line);
  }
  return out.map((l) => `${l}\n`).join('');
}

/** Recursively collect `.md` files under `path` (or `path` itself if a file). */
function collectMarkdown(path) {
  const st = statSync(path);
  if (st.isFile()) return path.endsWith('.md') ? [path] : [];
  const found = [];
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) found.push(...collectMarkdown(child));
    else if (entry.isFile() && entry.name.endsWith('.md')) found.push(child);
  }
  return found;
}

function main(argv) {
  let released = null;
  const paths = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--released-version') {
      const v = argv[++i];
      if (v === undefined) {
        console.error('strip-since: --released-version needs a value');
        process.exit(2);
      }
      released = parseVersion(v);
    } else {
      paths.push(argv[i]);
    }
  }
  if (paths.length === 0) {
    console.error('usage: strip-since.mjs [--released-version X] <path>...');
    process.exit(2);
  }

  let changed = 0;
  let scanned = 0;
  for (const path of paths) {
    for (const file of collectMarkdown(path)) {
      scanned++;
      const before = readFileSync(file, 'utf8');
      const after = stripUnreleasedProse(before, released);
      if (after !== before) {
        writeFileSync(file, after);
        changed++;
        console.log(`  stripped since-block(s): ${file}`);
      }
    }
  }
  const rel = released ? released.join('.') : '(none)';
  console.log(`strip-since: released=${rel}, scanned ${scanned} .md file(s), rewrote ${changed}.`);
}

// Run only as a CLI, not when imported by the test.
if (import.meta.url === `file://${process.argv[1]}`) {
  main(process.argv.slice(2));
}
