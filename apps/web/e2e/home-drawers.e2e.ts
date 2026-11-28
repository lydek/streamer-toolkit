// Home drawers' quick controls, with the admin API and the live socket mocked.
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, toLiveHome, zh } from './fixtures'

/** Home with `extra` in the live snapshot (each a list of one event of that type). */
async function openHome(page: Page, modules: string[], extra: Record<string, unknown>) {
  await mockAdmin(page, { modules: modules.map((id) => ({ id, enabled: true, closed: false })) })
  const payload: Record<string, unknown[]> = { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'twitch.contest': [], 'chat.text_size': [] }
  for (const [type, p] of Object.entries(extra)) payload[type] = [{ id: type, type, version: 1, seq: 1, occurredAt: '', source: 'e2e', payload: p }]
  await page.routeWebSocket(/\/ws\/admin$/, (ws) => ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 1, payload })))
  await page.goto('/admin/')
  await toLiveHome(page)
}

test('the counter drawer counts down and up, as the counter page does', async ({ page }) => {
  await openHome(page, ['counter'], { 'counter.values': { counters: [{ id: 'c1', name: 'Deaths', value: 3 }] } })
  const steps: unknown[] = []
  await page.route('**/api/admin/counters/*/add', (r) => {
    steps.push([new URL(r.request().url()).pathname, r.request().postDataJSON()])
    return r.fulfill({ json: { id: 'c1', name: 'Deaths', value: 2 } })
  })
  await page.locator('[data-drawer-tile="counter"]').click()
  const drawer = page.locator('#drawer-counter')
  await drawer.getByRole('button', { name: zh.counter.minusOneFor.replace('{name}', 'Deaths') }).click()
  await drawer.getByRole('button', { name: `${zh.counter.plusOne}：Deaths` }).click()
  await expect.poll(() => steps).toEqual([
    ['/api/admin/counters/c1/add', { delta: -1 }],
    ['/api/admin/counters/c1/add', { delta: 1 }],
  ])
})
