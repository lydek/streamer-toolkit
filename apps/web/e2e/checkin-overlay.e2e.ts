// The stamp card overlay of a real Host, driven by the check-in simulator (no records are written), and the
// simulator form in the admin UI (API mocked).
import { randomUUID } from 'node:crypto'
import { expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'
import { HOST_PORT } from './ports'

const HOST = `http://127.0.0.1:${HOST_PORT}`
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }

test('the simulator in the check-in drawer sends the name and stamps', async ({ page }) => {
  await openAdmin(page, { modules: [{ id: 'checkin', enabled: true, closed: false }] })
  const bodies: unknown[] = []
  await page.route('**/api/admin/simulate/checkin', (r) => {
    bodies.push(r.request().postDataJSON())
    return r.fulfill({ status: 204 })
  })
  await page.locator('[data-drawer-tile="checkin"]').click()
  const drawer = page.locator('#drawer-checkin')
  await drawer.getByLabel(zh.simulate.userName).fill('Viewer')
  await drawer.getByLabel(zh.simulate.stamps).fill('10')
  await drawer.getByRole('button', { name: zh.simulate.sendCheckin }).click()
  await expect(page.getByText(zh.simulate.checkinSent)).toBeVisible()
  expect(bodies).toEqual([{ userName: 'Viewer', stamps: 10 }])
})

test('simulated check-ins show one card at a time, in order, with the full-card effect', { tag: '@host' }, async ({ page, request }) => {
  const name = `E2E-${randomUUID().slice(0, 8)}`
  await page.addInitScript(() => {
    const w = window as unknown as { shown: string[]; most: number; full: string[] }
    Object.assign(w, { shown: [], most: 0, full: [] })
    new MutationObserver(() => {
      const cards = [...document.querySelectorAll('[data-checkin-card]')]
      w.most = Math.max(w.most, cards.length)
      for (const c of cards) {
        const text = `${c.querySelector('.name')?.textContent}:${c.querySelectorAll('.slots .stamped').length}`
        if (w.shown.at(-1) !== text) {
          w.shown.push(text)
          if (c.classList.contains('full')) w.full.push(text)
        }
      }
    }).observe(document, { childList: true, subtree: true, attributes: true })
  })
  await page.goto(`${HOST}/overlay/checkin/`)
  await expect.poll(() => page.evaluate(() => document.readyState)).toBe('complete')

  for (const [n, stamps] of [
    [1, 3],
    [2, 10],
    [3, 1],
  ]) {
    const res = await request.post(`${HOST}/api/admin/simulate/checkin`, {
      headers: admin,
      data: { userName: `${name}-${n}`, stamps },
    })
    expect(res.status()).toBe(204)
  }
  const seen = () =>
    page.evaluate(() => {
      const { shown, most, full } = window as unknown as { shown: string[]; most: number; full: string[] }
      return { shown, most, full }
    })
  await expect
    .poll(async () => (await seen()).shown.filter((t) => t.startsWith(name)), { timeout: 20_000 })
    .toEqual([`${name}-1:3`, `${name}-2:10`, `${name}-3:1`])
  const { most, full } = await seen()
  expect(most, 'one card at a time').toBe(1)
  expect(full).toEqual([`${name}-2:10`])
  await expect(page.locator('[data-checkin-card]')).toHaveCount(0, { timeout: 5_000 })

  // Simulations never reach the records.
  const viewers = await request.get(`${HOST}/api/admin/checkin/viewers?search=${name}`, { headers: admin })
  expect((await viewers.json()).total).toBe(0)
})
