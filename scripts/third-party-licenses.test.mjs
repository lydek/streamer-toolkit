import assert from 'node:assert/strict';
import fs from 'node:fs';
import { test } from 'node:test';
import { creditsSection, frontendSection } from './third-party-licenses.mjs';

test('lists every installed copy of each package with its license texts, sorted', () => {
  const licenses = {
    MIT: [{ name: 'vue', paths: ['/v'], author: 'Evan You', homepage: 'https://vuejs.org' }],
    'Apache-2.0 OR MIT': [{ name: 'a-pkg', paths: ['/a2', '/a1'] }],
  };
  const installed = {
    '/v': { version: '3.5.0', texts: ['MIT License\n(c) Evan You'] },
    '/a1': { version: '1.2.0', texts: ['APACHE', 'MIT'] },
    '/a2': { version: '1.10.0', texts: [] },
  };
  const text = frontendSection(licenses, (dir) => installed[dir]);
  const heads = text.split('\n').filter((l) => / \d/.test(l) && !l.startsWith('(c)'));
  assert.deepEqual(heads, ['a-pkg 1.2.0 (Apache-2.0 OR MIT)', 'a-pkg 1.10.0 (Apache-2.0 OR MIT)', 'vue 3.5.0 (MIT)']);
  assert.match(text, /APACHE\n\nMIT/);
  assert.match(text, /a-pkg 1\.10\.0 \(Apache-2\.0 OR MIT\)\n\nNo license file in the package; licensed under Apache-2\.0 OR MIT\./);
  assert.match(text, /vue 3\.5\.0 \(MIT\)\nEvan You\nhttps:\/\/vuejs\.org\n\nMIT License\n\(c\) Evan You/);
});

test('lists every asset row of assets/CREDITS.md', () => {
  const md = fs.readFileSync(new URL('../assets/CREDITS.md', import.meta.url), 'utf8');
  const text = creditsSection(md);
  assert.match(text, /^-{80}\napps\/web\/src\/overlay\/alerts\/chime\.wav\nSource: .+\nLicense: MIT/m);
  const rows = md.split('\n').filter((l) => l.startsWith('|')).length - 2;
  assert.equal(text.split('\nSource: ').length - 1, rows);
});

test('fails loudly when CREDITS.md has no asset rows', () => {
  assert.throws(() => creditsSection('# Credits\n'), /no asset rows/);
});
