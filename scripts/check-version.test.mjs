import assert from 'node:assert/strict';
import fs from 'node:fs';
import { test } from 'node:test';
import { readVersions, versionProblems } from './check-version.mjs';

const all = (v) => ({ 'apps/desktop/tauri.conf.json': v, 'apps/desktop/Cargo.toml': v, 'package.json': v });

test('passes when the tag matches all three versions', () => {
  assert.deepEqual(versionProblems('v1.2.3', all('1.2.3')), []);
});

test('names each file whose version differs from the tag', () => {
  const versions = { ...all('1.2.3'), 'package.json': '1.2.2', 'apps/desktop/Cargo.toml': undefined };
  assert.deepEqual(versionProblems('v1.2.3', versions), [
    'apps/desktop/Cargo.toml: version (none) does not match tag v1.2.3',
    'package.json: version 1.2.2 does not match tag v1.2.3',
  ]);
});

test('rejects tags that are not v<major>.<minor>.<patch>', () => {
  for (const tag of ['1.2.3', 'v1.2', 'v1.2.3-rc.1', undefined]) {
    assert.equal(versionProblems(tag, all('1.2.3')).length, 1, String(tag));
  }
});

test('the repo declares one version in all three places', () => {
  const versions = readVersions((f) => fs.readFileSync(new URL(`../${f}`, import.meta.url), 'utf8'));
  const [first] = Object.values(versions);
  assert.match(first, /^\d+\.\d+\.\d+$/);
  assert.deepEqual(versionProblems(`v${first}`, versions), []);
});
