// Writes THIRD_PARTY_LICENSES: the Rust crates (cargo-about), the bundled frontend packages (`pnpm licenses list
// --prod`) and the bundled assets (assets/CREDITS.md), as plain text.
// Generated at release build time only (never committed), so it always matches the lockfiles being built.
// Usage: node scripts/third-party-licenses.mjs [out]   (default: apps/web/public/THIRD_PARTY_LICENSES.txt, which the
// frontend build copies into the app for the settings page's about section)
// Needs cargo-about: cargo install --locked cargo-about --features cli
import { execFileSync, execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const rule = '-'.repeat(80);
const licenseFile = /^(licen[cs]e|copying|notice)([.-]|$)/i;

// `licenses` is the JSON of `pnpm licenses list --json`: { "<license>": [{ name, paths, author?, homepage? }] }.
// `readPackage(dir)` returns { version, texts: [license file contents] } for one installed copy.
export function frontendSection(licenses, readPackage) {
  const entries = Object.entries(licenses).flatMap(([license, pkgs]) =>
    pkgs.flatMap((p) => p.paths.map((dir) => ({ ...p, license, ...readPackage(dir) }))));
  entries.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true }));
  return entries.map((e) => [
    rule,
    `${e.name} ${e.version} (${e.license})`,
    ...[e.author, e.homepage].filter(Boolean),
    '',
    e.texts.length > 0 ? e.texts.join('\n\n').trim() : `No license file in the package; licensed under ${e.license}.`,
    '',
  ].join('\n')).join('\n');
}

// The table rows of assets/CREDITS.md: | File | Source | License |
export function creditsSection(md) {
  const rows = md.split('\n')
    .filter((l) => l.startsWith('|'))
    .map((l) => l.split('|').slice(1, -1).map((c) => c.trim().replace(/`/g, '')))
    .slice(2); // header and separator
  if (rows.length === 0) throw new Error('assets/CREDITS.md: no asset rows found');
  return rows.map(([file, source, license]) => [rule, file, `Source: ${source}`, `License: ${license}`, ''].join('\n')).join('\n');
}

function readPackage(dir) {
  const { version } = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  const texts = fs.readdirSync(dir).filter((f) => licenseFile.test(f)).sort()
    .filter((f) => fs.statSync(path.join(dir, f)).isFile())
    .map((f) => fs.readFileSync(path.join(dir, f), 'utf8').replace(/\r\n/g, '\n'));
  return { version, texts };
}

function main() {
  const out = path.resolve(root, process.argv[2] ?? 'apps/web/public/THIRD_PARTY_LICENSES.txt');
  // Fixed commands; execSync resolves pnpm.cmd on Windows.
  const rust = execFileSync('cargo', [
    'about', 'generate', '--locked', '--fail',
    '--manifest-path', 'apps/desktop/Cargo.toml',
    '-c', 'scripts/about.toml',
    'scripts/about.hbs',
  ], { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
  const licenses = JSON.parse(execSync('pnpm licenses list --prod --json', {
    cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
  }));
  const credits = fs.readFileSync(path.join(root, 'assets/CREDITS.md'), 'utf8');
  const text = [
    'Streamer Toolkit: third-party licenses',
    '',
    'Third-party software and assets bundled in Streamer Toolkit, with their licenses.',
    'Streamer Toolkit itself is released under the MIT license (see LICENSE).',
    '',
    '== Rust crates ==',
    rust.replace(/\r\n/g, '\n').trim(),
    '',
    '== Frontend packages ==',
    frontendSection(licenses, readPackage),
    '== Bundled assets ==',
    creditsSection(credits),
  ].join('\n');
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, text);
  console.log(`third-party-licenses: wrote ${path.relative(root, out)} (${Math.round(text.length / 1024)} KB)`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
