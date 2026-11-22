// Alerts: the simulator in the admin UI (API mocked), and the events and overlay of a real Host.
import { randomUUID } from 'node:crypto'
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, openAdmin, scrollsSideways, zh } from './fixtures'
import { HOST_PORT } from './ports'

const HOST = `http://127.0.0.1:${HOST_PORT}`
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }
const modules = [
  { id: 'chat', enabled: true, closed: false },
  { id: 'alerts', enabled: true, closed: false },
]

async function openSimulator(page: Page) {
  await openAdmin(page, { modules })
  await page.locator('[data-drawer-tile="alerts"]').click()
  return page.locator('#drawer-alerts')
}

test('the simulator sends each kind with its name, amount and message', async ({ page }) => {
  const drawer = await openSimulator(page)
  const bodies: unknown[] = []
  await page.route('**/api/admin/simulate/alert', (r) => {
    bodies.push(r.request().postDataJSON())
    return r.fulfill({ status: 204 })
  })
  const kind = (k: keyof typeof zh.alerts.kinds) => drawer.getByRole('button', { name: zh.alerts.kinds[k], exact: true })
  const send = () => drawer.getByRole('button', { name: zh.simulate.sendAlert }).click()

  await drawer.getByLabel(zh.simulate.userName).fill('Viewer')
  await send()
  // Beside the tiles (1280px) the result is a toast.
  await expect(page.getByText(zh.simulate.alertSent)).toBeVisible()
  // Follow and new subs take no amount or message.
  await expect(drawer.getByLabel(zh.simulate.message)).toHaveCount(0)

  await kind('subscribe').click()
  await send()
  await kind('resubscribe').click()
  await drawer.getByLabel(zh.alerts.amount.resubscribe).fill('12')
  await drawer.getByLabel(zh.simulate.message).fill('hello')
  await send()
  await kind('gift').click()
  await drawer.getByLabel(zh.alerts.amount.gift).fill('20')
  await send()
  await kind('cheer').click()
  await drawer.getByLabel(zh.alerts.amount.cheer).fill('100')
  await send()
  await kind('raid').click()
  await drawer.getByLabel(zh.alerts.amount.raid).fill('42')
  await send()

  await expect
    .poll(() => bodies.map((b) => (({ kind, amount }) => `${kind}:${amount}`)(b as { kind: string; amount: number })))
    .toEqual(['follow:5', 'subscribe:5', 'resubscribe:12', 'gift:20', 'cheer:100', 'raid:42'])
  expect(bodies[2]).toEqual({ kind: 'resubscribe', userName: 'Viewer', amount: 12, message: 'hello' })
})

test('a refused simulation shows why', async ({ page }) => {
  const drawer = await openSimulator(page)
  await page.route('**/api/admin/simulate/alert', (r) =>
    r.fulfill({ status: 400, json: { code: 'simulate.invalid_amount' } }),
  )
  await drawer.getByRole('button', { name: zh.simulate.sendAlert }).click()
  await expect(page.getByText(zh.errors.simulate.invalid_amount)).toBeVisible()
})

test('the simulator drawer never scrolls sideways at 1280px, 640px and 320px', async ({ page }) => {
  const drawer = await openSimulator(page)
  // Cheer shows every field the form has.
  await drawer.getByRole('button', { name: zh.alerts.kinds.cheer, exact: true }).click()
  for (const width of [1280, 640, 320]) {
    await page.setViewportSize({ width, height: 800 })
    await expect(page.locator('#drawer-alerts')).toBeVisible()
    expect(await scrollsSideways(page), `${width}px, page`).toBe(false)
    expect(await scrollsSideways(page.locator('#drawer-alerts')), `${width}px, drawer`).toBe(false)
  }
})

test('the alerts page picks a template; each preview plays the sample alert', async ({ page }) => {
  const s = await mockAdmin(page, { modules })
  await page.goto('/admin/alerts')
  const option = (id: keyof typeof zh.alerts.templates) => page.getByRole('radio', { name: zh.alerts.templates[id] })
  await expect(option('default')).toBeChecked()
  // The sample is a resub with a message, in every preview.
  for (const id of ['default', 'banner', 'text'] as const) {
    const frame = page.frameLocator(`iframe[title="${zh.alerts.templates[id]}"]`)
    await expect(frame.locator('[data-field="title"]')).toHaveText('Viewer 續訂了 12 個月！')
    await expect(frame.locator('[data-field="message"]')).toHaveText(zh.alerts.sample)
  }
  // The iframe ignores the pointer, so a click on the preview reaches its radio.
  await page.getByTitle(zh.alerts.templates.banner).click({ force: true })
  await expect(option('banner')).toBeChecked()
  await expect.poll(() => s.alertTemplate.selected).toBe('banner')
  await option('banner').focus()
  await page.keyboard.press('ArrowRight')
  await expect(option('text')).toBeChecked()
  await expect.poll(() => s.alertTemplate.selected).toBe('text')
  for (const width of [1280, 320]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await scrollsSideways(page), `${width}px`).toBe(false)
  }
})

test('picking an alert template switches the overlay at once, without reloading it', { tag: '@host' }, async ({ page, request }) => {
  const select = async (selected: string) => {
    const res = await request.put(`${HOST}/api/admin/templates/alerts`, { headers: admin, data: { selected } })
    expect(res.status()).toBe(204)
  }
  await select('default')
  await page.goto(`${HOST}/overlay/alerts/`)
  await expect(page.locator('.card[data-alert-stage]')).toBeAttached()
  await page.evaluate(() => Object.assign(window, { notReloaded: true }))
  for (const [id, stage] of [
    ['banner', '.banner'],
    ['text', '.text'],
  ]) {
    await select(id)
    await expect(page.locator(`${stage}[data-alert-stage]`)).toBeAttached()
    await expect(page.locator('[data-alert-stage]')).toHaveCount(1)
  }
  expect(await page.evaluate(() => 'notReloaded' in window)).toBe(true)
  const status = await request.get(`${HOST}/api/admin/templates/alerts`, { headers: admin })
  expect(await status.json()).toEqual({ selected: 'text' })
  const unknown = await request.put(`${HOST}/api/admin/templates/alerts`, { headers: admin, data: { selected: 'x' } })
  expect(unknown.status()).toBe(400)
  await select('default')
})

test('the Host sends every simulated kind as an alert marked source=simulation', { tag: '@host' }, async ({ page, request }) => {
  await page.goto(`${HOST}/overlay/chat/`)
  // Listens on the overlay channel from the Host's own origin, as an overlay does.
  await page.evaluate(
    () =>
      new Promise<void>((open) => {
        const seen: unknown[] = []
        Object.assign(window, { seen })
        const ws = new WebSocket(`ws://${location.host}/ws/overlay`)
        ws.onmessage = (e) => seen.push(JSON.parse(e.data))
        ws.onopen = () => open()
      }),
  )
  for (const data of [
    { kind: 'follow' },
    { kind: 'subscribe' },
    { kind: 'resubscribe', amount: 12, message: 'hi' },
    { kind: 'gift', amount: 2 },
    { kind: 'cheer', amount: 100 },
    { kind: 'raid', amount: 42 },
  ]) {
    const res = await request.post(`${HOST}/api/admin/simulate/alert`, { headers: admin, data })
    expect(res.status()).toBe(204)
  }
  const alerts = () =>
    page.evaluate(() =>
      (window as unknown as { seen: { type: string; source: string; payload: { kind: string } }[] }).seen.filter(
        (m) => m.type === 'alert',
      ),
    )
  // The Host plays them one at a time (the E2E Host shows each for 1.5 to 2 s).
  await expect
    .poll(async () => [...new Set((await alerts()).map((a) => a.payload.kind))], { timeout: 20_000 })
    .toEqual(['follow', 'subscribe', 'resubscribe', 'gift', 'cheer', 'raid'])
  expect((await alerts()).every((a) => a.source === 'simulation')).toBe(true)

  const refused = await request.post(`${HOST}/api/admin/simulate/alert`, {
    headers: admin,
    data: { kind: 'gift', amount: 101 },
  })
  expect(refused.status()).toBe(400)
  expect(await refused.json()).toEqual({ code: 'simulate.invalid_amount' })
  // Overlays and anyone without the token get no simulator.
  const anonymous = await request.post(`${HOST}/api/admin/simulate/alert`, {
    headers: { Origin: HOST },
    data: { kind: 'follow' },
  })
  expect(anonymous.status()).toBe(401)
})

test('the overlay plays every kind in order, one at a time with a sound, and a gift of 3 once', { tag: '@host' }, async ({
  page,
  request,
}) => {
  // Alerts other tests queued on the shared Host may play first; only this test's name counts.
  test.setTimeout(60_000)
  const name = `e2e-${randomUUID().slice(0, 8)}`
  await page.addInitScript(() => {
    const w = window as unknown as { shown: string[]; sounds: number; most: number }
    Object.assign(w, { shown: [], sounds: 0, most: 0 })
    // Headless Chromium refuses autoplay; count the attempts instead.
    HTMLMediaElement.prototype.play = () => (w.sounds++, Promise.resolve())
    new MutationObserver(() => {
      const titles = [...document.querySelectorAll('[data-alert-stage] [data-field="title"]')]
      w.most = Math.max(w.most, titles.length)
      for (const t of titles) if (w.shown.at(-1) !== t.textContent) w.shown.push(t.textContent!)
    }).observe(document, { childList: true, subtree: true })
  })
  await page.goto(`${HOST}/overlay/alerts/`)
  await expect(page.locator('[data-alert-stage]')).toBeAttached()

  for (const data of [
    { kind: 'gift', amount: 3 },
    { kind: 'follow' },
    { kind: 'subscribe' },
    { kind: 'resubscribe', amount: 12, message: 'hi' },
    { kind: 'cheer', amount: 100 },
    { kind: 'raid', amount: 42 },
  ]) {
    const res = await request.post(`${HOST}/api/admin/simulate/alert`, { headers: admin, data: { ...data, userName: name } })
    expect(res.status()).toBe(204)
  }
  const seen = () =>
    page.evaluate(() => {
      const { shown, sounds, most } = window as unknown as { shown: string[]; sounds: number; most: number }
      return { shown, sounds, most }
    })
  await expect
    .poll(async () => (await seen()).shown.filter((t) => t.includes(name)), { timeout: 40_000 })
    .toEqual([
      `${name} 送了 3 份訂閱！`,
      `${name} 追隨了！`,
      `${name} 訂閱了！`,
      `${name} 續訂了 12 個月！`,
      `${name} 送出 100 小奇點！`,
      `${name} 帶著 42 人揪團來了！`,
    ])
  const { shown, sounds, most } = await seen()
  expect(most, 'one alert at a time').toBe(1)
  expect(sounds, 'a sound per alert').toBe(shown.length)
  // Each alert leaves once its time is up.
  await expect(page.locator('[data-alert-stage] [data-field="title"]')).toHaveCount(0, { timeout: 5_000 })
})

test('an emergency stop takes the alert off the overlay at once and nothing waiting plays', { tag: '@host' }, async ({
  page,
  request,
}) => {
  // Alerts other tests queued on the shared Host may play first; only this test's name counts.
  test.setTimeout(60_000)
  const name = `e2e-${randomUUID().slice(0, 8)}`
  await page.goto(`${HOST}/overlay/alerts/`)
  await expect(page.locator('[data-alert-stage]')).toBeAttached()
  for (let n = 0; n < 3; n++) {
    const res = await request.post(`${HOST}/api/admin/simulate/alert`, { headers: admin, data: { kind: 'follow', userName: name } })
    expect(res.status()).toBe(204)
  }
  const title = page.locator('[data-alert-stage] [data-field="title"]', { hasText: name })
  await expect(title).toBeVisible({ timeout: 30_000 })

  const res = await request.post(`${HOST}/api/admin/queue/stop`, { headers: admin })
  expect(await res.json()).toEqual({ stopped: true, cancelled: 2 })
  // Well before the alert's own 1.5 s are up.
  await expect(title).toHaveCount(0, { timeout: 500 })
  await page.waitForTimeout(2500)
  await expect(title).toHaveCount(0)
})

test('跳過 takes the alert playing off the overlay and the next one follows', { tag: '@host' }, async ({ page, request }) => {
  test.setTimeout(60_000)
  const name = `e2e-${randomUUID().slice(0, 8)}`
  await page.goto(`${HOST}/overlay/alerts/`)
  await expect(page.locator('[data-alert-stage]')).toBeAttached()
  for (let n = 0; n < 2; n++) {
    const res = await request.post(`${HOST}/api/admin/simulate/alert`, { headers: admin, data: { kind: 'follow', userName: name } })
    expect(res.status()).toBe(204)
  }
  const title = page.locator('[data-alert-stage] [data-field="title"]', { hasText: name })
  await expect(title).toBeVisible({ timeout: 30_000 })

  const res = await request.post(`${HOST}/api/admin/queue/skip`, { headers: admin })
  expect(await res.json()).toEqual({ skipped: true })
  await expect(title).toHaveCount(0, { timeout: 500 })
  // The second one plays after the gap.
  await expect(title).toBeVisible({ timeout: 5_000 })
})

test('最近事件 says what became of each alert and 重播 queues it again', async ({ page }) => {
  await mockAdmin(page)
  const payload = (kind: string, userName: string, extra: object = {}) => ({
    kind,
    userId: '7',
    userLogin: userName.toLowerCase(),
    userName,
    durationMs: 6000,
    sound: { kind: 'built_in', volume: 100 },
    ...extra,
  })
  const replays: string[] = []
  await page.route('**/api/admin/alerts/recent', (r) =>
    r.fulfill({
      json: [
        { id: 'a3', occurredAt: '2026-11-19T13:40:00.000Z', source: 'twitch', outcome: null, payload: payload('follow', 'Kai') },
        { id: 'a2', occurredAt: '2026-11-19T13:31:00.000Z', source: 'twitch', outcome: 'sound_missing', payload: payload('cheer', 'Berry', { bits: 100 }) },
        { id: 'a1', occurredAt: '2026-11-19T12:58:00.000Z', source: 'simulation', outcome: 'cleared', payload: payload('raid', 'Rain', { viewers: 12 }) },
      ],
    }),
  )
  await page.route('**/api/admin/alerts/recent/*/replay', (r) => {
    replays.push(new URL(r.request().url()).pathname)
    return r.fulfill({ status: 204 })
  })
  await page.goto('/admin/alert-log')
  const recent = page.getByRole('region', { name: zh.alerts.recent.title })
  const rows = recent.getByRole('listitem')
  await expect(rows).toHaveCount(3)
  await expect(rows.nth(0)).toContainText(zh.alerts.recent.outcome.pending)
  await expect(rows.nth(1)).toContainText(zh.alerts.recent.outcome.sound_missing)
  await expect(rows.nth(2)).toContainText(zh.alerts.recent.outcome.cleared)
  await expect(rows.nth(2)).toContainText(zh.alerts.recent.simulated)

  await rows.nth(1).getByRole('button', { name: zh.alerts.recent.replay }).click()
  await expect(page.getByText(zh.alerts.recent.replayed)).toBeVisible()
  expect(replays).toEqual(['/api/admin/alerts/recent/a2/replay'])
})
