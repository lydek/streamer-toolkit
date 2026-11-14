import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { findViolations, isAllowed, parseAllowlist } from './check-licenses.mjs';

const decisions = fs.readFileSync(fileURLToPath(new URL('../docs/decisions.md', import.meta.url)), 'utf8');
const allowed = parseAllowlist(decisions);

// Pinned on purpose: the parser reads prose, so any change to the allowlist row must be
// mirrored here and shows up in review. Never add an id just to make a dependency pass.
test('allowlist parsed from docs/decisions.md', () => {
  assert.deepEqual([...allowed].sort(), [
    '0BSD', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'BlueOak-1.0.0', 'CC-BY-4.0', 'CC0-1.0',
    'CDLA-Permissive-2.0', 'ISC', 'MIT', 'MPL-2.0', 'Python-2.0', 'Unicode-3.0', 'Zlib',
  ]);
});

test('script runs against the installed dependencies and passes', () => {
  const out = execFileSync(process.execPath, [fileURLToPath(new URL('./check-licenses.mjs', import.meta.url))], { encoding: 'utf8' });
  assert.match(out, /all licenses allowed/);
});

test('parseAllowlist fails loudly on a missing row', () => {
  assert.throws(() => parseAllowlist('| 授權 | MIT |'));
});

test('single ids', () => {
  assert.ok(isAllowed('MIT', allowed));
  assert.ok(!isAllowed('GPL-3.0-only', allowed));
  assert.ok(!isAllowed('SEE LICENSE IN LICENSE.md', allowed)); // PrimeVue 5 / primeicons 8 / @primeuix/themes 3
  assert.ok(!isAllowed('Unknown', allowed));
  assert.ok(!isAllowed('', allowed));
  assert.ok(!isAllowed('MIT+', allowed));
  assert.ok(!isAllowed('MIT WITH Classpath-exception-2.0', allowed));
});

test('SPDX expressions', () => {
  assert.ok(isAllowed('Apache-2.0 OR MIT', allowed));
  assert.ok(isAllowed('(MIT OR GPL-3.0-only)', allowed));
  assert.ok(isAllowed('MIT AND ISC', allowed));
  assert.ok(!isAllowed('MIT AND GPL-3.0-only', allowed));
  assert.ok(!isAllowed('GPL-2.0-only OR LGPL-3.0-only', allowed));
  assert.ok(isAllowed('GPL-3.0-only OR MIT AND ISC', allowed)); // AND binds tighter
  assert.ok(!isAllowed('(MIT', allowed));
  assert.ok(!isAllowed('MIT OR', allowed));
});

test('findViolations lists offending packages', () => {
  const pnpmOutput = {
    MIT: [{ name: 'vue', versions: ['3.5.45'] }],
    'SEE LICENSE IN LICENSE.md': [
      { name: 'primevue', versions: ['5.0.0'] },
      { name: 'primeicons', versions: ['8.0.0'] },
    ],
  };
  assert.deepEqual(findViolations(pnpmOutput, allowed), [
    'primevue@5.0.0: SEE LICENSE IN LICENSE.md',
    'primeicons@8.0.0: SEE LICENSE IN LICENSE.md',
  ]);
  assert.deepEqual(findViolations({ MIT: pnpmOutput.MIT }, allowed), []);
});
