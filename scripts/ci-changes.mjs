// Decides which CI jobs a PR's files can affect; run by the `changes` job in .github/workflows/ci.yml.
// Usage: node scripts/ci-changes.mjs   (lists the files of the PR merge commit and writes web, rust and fakes_ui to
// $GITHUB_OUTPUT when set)
// A flag is false only when every changed file is one its job provably ignores, so any path not named here runs every
// job. With rebase-merge main is never re-checked: a wrong `false` lets broken code into main, so list only what is
// verified (ci-changes.test.mjs checks the cross-reads).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

// Docs: no check reads them but the translation check, which always runs. docs/decisions.md feeds the frontend license
// allowlist.
export const isDocs = (f) =>
  (!f.includes('/') && f.endsWith('.md')) ||
  (f.startsWith('docs/') && f !== 'docs/decisions.md') ||
  /^(prototype|\.agents|\.claude)\//.test(f);

// The workflows and these rules: a PR that edits them runs everything.
const runsAll = (f) => f.startsWith('.github/') || /^scripts\/ci-changes(\.test)?\.mjs$/.test(f);

// Rust-side files that frontend or script checks read; changing one runs `web`.
export const WEB_READS_RUST = [
  'apps/desktop/Cargo.toml', // scripts/check-version.test.mjs
  'apps/desktop/src/data_folder.rs', // scripts/dev-fake.test.mjs
  'apps/desktop/src/main.rs', // apps/web/dev-server.test.ts
  'apps/desktop/tauri.conf.json', // apps/web/src/admin/SettingsView.vue, scripts/check-version.test.mjs
  'crates/host/src/lib.rs', // apps/web/dev-server.test.ts
];

const ignored = {
  web: (f) =>
    !WEB_READS_RUST.includes(f) &&
    (/^(crates|apps\/desktop)\//.test(f) || /^(Cargo\.(toml|lock)|rust-toolchain\.toml|deny\.toml)$/.test(f)),
  // Rust reads apps/web only through dist, dist-port, the built-in templates and the api:gen output in src/api; the
  // Host tests check that built pages exist, not what they contain.
  rust: (f) => /^apps\/web\/(src\/admin|src\/locales|e2e)\//.test(f) || f === 'docs/decisions.md',
  // fakes builds against core and integrations only (dev-dependencies), and no crate depends on it.
  fakes_ui: (f) =>
    /^(apps|scripts|assets)\//.test(f) ||
    /^crates\/(host|modules|arch-tests)\//.test(f) ||
    /^(deny\.toml|package\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|\.nvmrc|docs\/decisions\.md)$/.test(f),
};

export function classify(files) {
  const code = files.filter((f) => !isDocs(f));
  // An empty list means the diff could not be read: run everything rather than guess.
  const all = files.length === 0 || code.some(runsAll);
  return Object.fromEntries(Object.entries(ignored).map(([job, skips]) => [job, all || !code.every(skips)]));
}

function main() {
  // core.quotepath=false: non-ASCII paths print as they are instead of quoted octal; --no-renames: a rename lists the
  // old path too, so moving a file another check reads still counts.
  const out = execFileSync('git', ['-c', 'core.quotepath=false', 'diff', '--name-only', '--no-renames', 'HEAD^1', 'HEAD'], {
    encoding: 'utf8',
  });
  const files = out.split(/\r?\n/).filter(Boolean);
  const lines = Object.entries(classify(files)).map(([job, run]) => `${job}=${run}`);
  console.log(`${files.length} changed file(s): ${lines.join(' ')}`);
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `${lines.join('\n')}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) main();
