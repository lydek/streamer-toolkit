// Fails unless the release tag (v<version>) matches the app version in all three places that carry it.
// Usage: node scripts/check-version.mjs <tag>   (run by .github/workflows/release.yml before building)
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

// `versions` maps each file to the version it declares (undefined when none was found).
export function versionProblems(tag, versions) {
  const want = /^v(\d+\.\d+\.\d+)$/.exec(tag ?? '')?.[1];
  if (!want) return [`tag "${tag}" is not v<major>.<minor>.<patch>`];
  return Object.entries(versions)
    .filter(([, v]) => v !== want)
    .map(([file, v]) => `${file}: version ${v ?? '(none)'} does not match tag ${tag}`);
}

export function readVersions(read) {
  return {
    'apps/desktop/tauri.conf.json': JSON.parse(read('apps/desktop/tauri.conf.json')).version,
    // The [package] table comes first in this manifest.
    'apps/desktop/Cargo.toml': /^version\s*=\s*"([^"]*)"/m.exec(read('apps/desktop/Cargo.toml'))?.[1],
    'package.json': JSON.parse(read('package.json')).version,
  };
}

function main() {
  const problems = versionProblems(process.argv[2], readVersions((f) => fs.readFileSync(`${root}${f}`, 'utf8')));
  if (problems.length > 0) {
    for (const p of problems) console.error(`check-version: ${p}`);
    process.exit(1);
  }
  console.log(`check-version: ${process.argv[2]} matches all versions`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
