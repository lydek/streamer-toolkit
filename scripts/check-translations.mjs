// Fails when a zh-TW doc (README.md, docs/**/*.md except docs/archive) has no *.en.md,
// when a *.en.md in those roots has no matching source,
// or when an English translation is older than its zh-TW source.
// Each *.en.md must start with: <!-- source: <path from repo root> source-sha256: <hex> -->
// and the source path must be its sibling zh-TW file.
// Line endings are normalized so CRLF/LF checkouts hash the same.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const skipNames = new Set(['node_modules', '.git', 'target']);
const skipPaths = new Set(['docs/archive']);
const header = /^<!--\s*source:\s*(\S+)\s+source-sha256:\s*([0-9a-f]{64})\s*-->/;
const rel = (p) => path.relative(root, p).split(path.sep).join('/');
const read = (file) => fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
const sha = (file) => createHash('sha256').update(read(file).replace(/\r\n/g, '\n')).digest('hex');

function walk(dir, onFile) {
  if (!fs.existsSync(dir)) return;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (skipNames.has(e.name) || skipPaths.has(rel(p))) continue;
    if (e.isDirectory()) walk(p, onFile);
    else onFile(p, e.name);
  }
}

let failed = 0;
const fail = (msg) => { console.error(msg); failed++; };

// Sources and translations come from the same roots: README.md and docs/.
const sources = [path.join(root, 'README.md')];
const translations = [];
if (fs.existsSync(path.join(root, 'README.en.md'))) translations.push(path.join(root, 'README.en.md'));
walk(path.join(root, 'docs'), (p, name) => {
  if (name.endsWith('.en.md')) translations.push(p);
  else if (name.endsWith('.md')) sources.push(p);
});

// Every zh-TW source must have a sibling translation.
for (const src of sources) {
  const en = src.replace(/\.md$/, '.en.md');
  if (!fs.existsSync(en)) fail(`${rel(en)}: missing translation of ${rel(src)}`);
}

// Every translation must point at its sibling source and match its hash.
for (const file of translations) {
  const m = read(file).match(header);
  if (!m) { fail(`${rel(file)}: missing source header on line 1`); continue; }
  const expected = rel(file.replace(/\.en\.md$/, '.md'));
  if (m[1] !== expected) { fail(`${rel(file)}: source must be ${expected}, found ${m[1]}`); continue; }
  const source = path.join(root, expected);
  if (!fs.existsSync(source)) { fail(`${rel(file)}: source ${expected} not found`); continue; }
  const actual = sha(source);
  if (actual !== m[2]) {
    fail(`${rel(file)}: out of date, ${expected} changed. Re-translate, review, then set source-sha256: ${actual}`);
  }
}

console.log(`${sources.length} source(s), ${translations.length} translation(s) checked, ${failed} problem(s)`);
process.exit(failed ? 1 : 0);
