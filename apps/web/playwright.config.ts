import { randomUUID } from 'node:crypto'
import { rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'
import { defineConfig, devices } from '@playwright/test'
import { HOST_PORT, VITE_PORT } from './e2e/ports'

// The runner and every worker load this file; only the first load (the runner) starts from a clean data folder
// and picks the token, and the workers it starts inherit both through the environment.
if (!process.env.E2E_DATA_DIR) {
  process.env.E2E_DATA_DIR = resolve(tmpdir(), 'streamer-toolkit-e2e')
  rmSync(process.env.E2E_DATA_DIR, { recursive: true, force: true })
  // Random per run: the test Host's admin token is never a fixed value in the repo.
  process.env.E2E_ADMIN_TOKEN = randomUUID()
}

export default defineConfig({
  testDir: 'e2e',
  // Not *.test.ts / *.spec.ts, which Vitest would pick up.
  testMatch: '*.e2e.ts',
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [['list'], ['github']] : 'list',
  use: { ...devices['Desktop Chrome'], baseURL: `http://localhost:${VITE_PORT}`, trace: 'retain-on-failure' },
  webServer: [
    {
      // The admin UI with its API mocked per test (e2e/fixtures.ts); also serves e2e/harness/.
      command: `pnpm exec vite --port ${VITE_PORT} --strictPort`,
      url: `http://localhost:${VITE_PORT}/@vite/client`,
    },
    {
      // A real Host for the overlay tests; embeds apps/web/dist, so the frontend must be built first.
      command: `cargo run -q -p host --example e2e_host -- ${HOST_PORT} "${process.env.E2E_DATA_DIR}"`,
      url: `http://127.0.0.1:${HOST_PORT}/api/health`,
      timeout: 300_000,
    },
  ],
})
