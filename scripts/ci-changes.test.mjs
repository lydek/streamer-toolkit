import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WEB_READS_RUST, classify, isDocs } from './ci-changes.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const rel = (p) => path.relative(root, p).split(path.sep).join('/');
const all = { web: true, rust: true, fakes_ui: true };

test('runs each job only when the files can affect it', () => {
  const cases = [
    [['README.md', 'docs/dev.md', 'prototype/ui-v2.html', '.claude/settings.json'], { web: false, rust: false, fakes_ui: false }],
    [['apps/web/src/admin/HomeView.vue', 'apps/web/e2e/home.e2e.ts'], { web: true, rust: false, fakes_ui: false }],
    [['apps/web/src/locales/zh-TW/common.json'], { web: true, rust: false, fakes_ui: false }],
    [['docs/decisions.md'], { web: true, rust: false, fakes_ui: false }],
    [['crates/modules/chat/src/lib.rs', 'Cargo.lock'], { web: false, rust: true, fakes_ui: true }],
    [['crates/host/src/admin.rs', 'apps/desktop/src/tray.rs'], { web: false, rust: true, fakes_ui: false }],
    [['crates/fakes/src/obs.rs'], { web: false, rust: true, fakes_ui: true }],
    [['crates/integrations/src/obs.rs'], { web: false, rust: true, fakes_ui: true }],
    [['apps/web/src/overlay/chat/templates/default/template.html'], { web: true, rust: true, fakes_ui: false }],
    [['apps/web/src/api/schema.d.ts', 'crates/host/src/openapi.rs'], { web: true, rust: true, fakes_ui: false }],
    [['package.json'], { web: true, rust: true, fakes_ui: false }],
    [['scripts/bench.mjs'], { web: true, rust: true, fakes_ui: false }],
    [['lefthook.yml'], all],
    [['clippy.toml'], all],
    [['.github/workflows/pages.yml'], all],
    [['scripts/ci-changes.mjs'], all],
    [['apps/web/src/admin/HomeView.vue', 'scripts/ci-changes.test.mjs'], all],
    [[], all],
  ];
  for (const [files, want] of cases) assert.deepEqual(classify(files), want, files.join(' '));
});

test('a Rust file that frontend checks read runs web, also when moved', () => {
  for (const f of WEB_READS_RUST) assert.equal(classify([f]).web, true, f);
  // git diff --no-renames lists a moved file under its old and new path.
  assert.equal(classify(['crates/host/src/lib.rs', 'crates/host/src/server.rs']).web, true);
});

test('every tracked file outside the docs runs at least one job', () => {
  const files = execFileSync('git', ['-c', 'core.quotepath=false', 'ls-files'], { cwd: root, encoding: 'utf8' })
    .split(/\r?\n/)
    .filter((f) => f && !isDocs(f));
  assert.ok(files.length > 0);
  for (const f of files) assert.ok(Object.values(classify([f])).some(Boolean), f);
});

function walk(dir, keep, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (!['node_modules', 'target', 'dist', 'dist-port', '.git'].includes(e.name)) walk(p, keep, out);
    } else if (keep(p)) out.push(p);
  }
  return out;
}

// Quoted paths in `file` that resolve to a repo file or folder, from the file's folder, `base` or the repo root. `exists`
// filters out strings that only look like paths; build outputs such as apps/web/dist may not exist yet.
function quotedPaths(file, pattern, base, exists = fs.existsSync) {
  const found = new Set();
  for (const m of fs.readFileSync(file, 'utf8').matchAll(pattern)) {
    for (const from of [path.dirname(file), base, root]) {
      const p = path.resolve(from, m[2]);
      if (p.startsWith(root) && exists(p)) found.add(rel(p));
    }
  }
  return found;
}

// Keep cross-reads as plain string literals: a path built from pieces (path.join('crates', x)) escapes these scans.
test('the files frontend and script code reads from the Rust side are exactly WEB_READS_RUST', () => {
  const sources = [
    ...walk(path.join(root, 'apps/web'), (p) => /\.(ts|vue|js|mjs)$/.test(p) && !rel(p).startsWith('apps/web/src/api/')),
    // These rules and their tests name paths without reading them.
    ...walk(path.join(root, 'scripts'), (p) => p.endsWith('.mjs') && !/ci-changes(\.test)?\.mjs$/.test(p)),
  ];
  const found = new Set();
  for (const f of sources) {
    for (const p of quotedPaths(f, /(['"`])([^'"`\n]*(?:crates|desktop)\/[^'"`\n]*)\1/g, root)) found.add(p);
  }
  assert.deepEqual([...found].sort(), [...WEB_READS_RUST].sort());
});

test('every apps/web path the Rust side reads runs rust', () => {
  const crateRoot = (file) => {
    let dir = path.dirname(file);
    while (!fs.existsSync(path.join(dir, 'Cargo.toml'))) dir = path.dirname(dir);
    return dir;
  };
  const sources = [
    ...walk(path.join(root, 'crates'), (p) => p.endsWith('.rs')),
    ...walk(path.join(root, 'apps/desktop'), (p) => p.endsWith('.rs') || p.endsWith('tauri.conf.json')),
  ];
  const found = new Set();
  for (const f of sources) {
    // Rust strings and JSON values use double quotes; rust-embed folders resolve from the crate root.
    const underWeb = (p) => rel(p).startsWith('apps/web/');
    for (const p of quotedPaths(f, /(")([^"\n]*web\/[^"\n]*)\1/g, crateRoot(f), underWeb)) found.add(p);
  }
  for (const p of ['apps/web/dist', 'apps/web/dist-port', 'apps/web/src/overlay/chat/templates']) {
    assert.ok(found.has(p), `${p} not found in ${[...found].join(' ')}`);
  }
  for (const p of found) assert.equal(classify([`${p}/x`]).rust, true, p);
  assert.equal(classify(['apps/web/src/api/openapi.json']).rust, true, 'api:gen output');
});
