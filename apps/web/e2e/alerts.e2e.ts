// Alerts: the simulator in the admin UI (API mocked) and the alert events a real Host sends for it.
import { type Page, expect, test } from '@playwright/test'
import { openAdmin, scrollsSideways, zh } from './fixtures'
import { HOST_PORT } from './ports'

const HOST = `http://127.0.0.1:${HOST_PORT}`
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }
const modules = [
  { id: 'chat', enabled: true },
  { id: 'alerts', enabled: true },
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

test('a refused simulation shows why inside the drawer', async ({ page }) => {
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

test('the Host sends every simulated kind as an alert marked source=simulation', async ({ page, request }) => {
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
  await expect
    .poll(async () => [...new Set((await alerts()).map((a) => a.payload.kind))])
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
