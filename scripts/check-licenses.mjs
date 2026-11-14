// Fails when a bundled (production) frontend dependency has a license outside the allowlist.
// The allowlist is read from docs/decisions.md (row "相依套件授權"), so docs and check cannot drift.
// Dev-only tooling is skipped (`pnpm licenses list --prod`): it is not shipped.
// Unknown, missing or "SEE LICENSE IN ..." values fail. Never loosen the list to make a failure pass.
import { execSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const rowMarker = '| 相依套件授權 |';
// Non-license words that appear in the same table cell.
const notLicenses = new Set(['Rust', 'cargo-deny']);

// The cell lists the Rust allowlist, then "the above plus ..." for the frontend, then the blocked GPL family.
// shortcut: parses prose, not data; a reworded cell fails loudly (empty list or unknown license), never passes silently.
export function parseAllowlist(decisionsMd) {
  const row = decisionsMd.split('\n').find((l) => l.startsWith(rowMarker));
  if (!row) throw new Error(`docs/decisions.md: row "${rowMarker}" not found`);
  const cell = row.slice(rowMarker.length).split('GPL')[0]
    .replace(/BSD-(\d)／(\d)-Clause/, 'BSD-$1-Clause、BSD-$2-Clause');
  const ids = (cell.match(/[A-Za-z0-9][A-Za-z0-9.+-]*/g) ?? []).filter((t) => !notLicenses.has(t));
  if (ids.length === 0) throw new Error('docs/decisions.md: no license ids found in the allowlist row');
  return new Set(ids);
}

// SPDX expression: ids joined by AND / OR (AND binds tighter), parentheses allowed.
// WITH exceptions, "+" suffixes and anything else not in the allowlist are rejected.
export function isAllowed(expression, allowed) {
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  let i = 0;
  const parseOr = () => {
    let ok = parseAnd();
    while (tokens[i] === 'OR') { i++; ok = parseAnd() || ok; }
    return ok;
  };
  const parseAnd = () => {
    let ok = parseAtom();
    while (tokens[i] === 'AND') { i++; ok = parseAtom() && ok; }
    return ok;
  };
  const parseAtom = () => {
    const t = tokens[i++];
    if (t === '(') {
      const ok = parseOr();
      if (tokens[i++] !== ')') throw new SyntaxError('missing )');
      return ok;
    }
    return t !== undefined && allowed.has(t);
  };
  try {
    const ok = parseOr();
    return ok && i === tokens.length;
  } catch {
    return false;
  }
}

// `licenses` is the JSON of `pnpm licenses list --json`: { "<license>": [{ name, versions }] }.
export function findViolations(licenses, allowed) {
  return Object.entries(licenses)
    .filter(([license]) => !isAllowed(license, allowed))
    .flatMap(([license, pkgs]) => pkgs.map((p) => `${p.name}@${p.versions.join(',')}: ${license}`));
}

function main() {
  const allowed = parseAllowlist(fs.readFileSync(`${root}docs/decisions.md`, 'utf8'));
  // execSync resolves pnpm.cmd on Windows; the command is fixed.
  let licenses;
  try {
    licenses = JSON.parse(execSync('pnpm licenses list --prod --json', {
      cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
    }));
  } catch (e) {
    console.error(`check-licenses: \`pnpm licenses list\` failed:\n${e.stdout || e.message}`);
    process.exit(1);
  }
  const count = Object.values(licenses).reduce((n, pkgs) => n + pkgs.length, 0);
  if (count === 0) {
    console.error('check-licenses: pnpm reported no production dependencies (is pnpm install done?)');
    process.exit(1);
  }
  const violations = findViolations(licenses, allowed);
  if (violations.length > 0) {
    console.error('Bundled dependencies outside the license allowlist (docs/decisions.md):');
    for (const v of violations) console.error(`  ${v}`);
    process.exit(1);
  }
  console.log(`check-licenses: ${count} bundled packages, all licenses allowed`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
