// The chat overlay against a real Host (crates/host/examples/e2e_host.rs): built-in templates picked through the
// admin API, and the watched user template folder (`custom`).
import { randomUUID } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { type APIRequestContext, type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import { zh } from './fixtures'
import { HOST_PORT } from './ports'

const HOST = `http://127.0.0.1:${HOST_PORT}`
const TEMPLATE_DIR = join(process.env.E2E_DATA_DIR!, 'templates', 'chat')
// The Host never creates the user folder.
mkdirSync(TEMPLATE_DIR, { recursive: true })
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }

// The tests share one Host and template folder; tagged @host, they run one at a time (playwright.config.ts).

const template = (item: string, extra = '') =>
  `${extra}<ul data-chat-list><template data-chat-message>${item}</template></ul>`

const writeTemplate = (html: string) => writeFileSync(join(TEMPLATE_DIR, 'template.html'), html)

/** Sends a simulated chat message with a unique text (the Host's snapshot keeps earlier ones) and returns it. */
async function say(request: APIRequestContext) {
  const text = randomUUID()
  const res = await request.post(`${HOST}/api/admin/simulate/chat`, { headers: admin, data: { userName: 'e2e', text } })
  expect(res.status()).toBe(204)
  return text
}

/** Picks the overlay's template, as the admin UI's picker does (`custom` has no UI entry). */
async function select(request: APIRequestContext, selected: string) {
  const res = await request.put(`${HOST}/api/admin/templates/chat`, { headers: admin, data: { selected } })
  expect(res.status()).toBe(204)
}

/** Each test starts from its own user template, so it passes alone and in any order. */
async function start(page: Page, request: APIRequestContext, itemClass: string) {
  writeTemplate(template(`<li class="${itemClass}"><span data-field="text"></span></li>`))
  await select(request, 'custom')
  await page.goto(`${HOST}/overlay/chat/`)
  const text = await say(request)
  await expect(page.locator(`li.${itemClass}`, { hasText: text })).toBeVisible()
  return text
}

test('picking a built-in template applies live, without reloading the overlay', { tag: '@host' }, async ({ page, request }) => {
  await select(request, 'default')
  await page.goto(`${HOST}/overlay/chat/`)
  const text = await say(request)
  await expect(page.locator('ul.chat:not(.bubble):not(.compact) li.message', { hasText: text })).toBeVisible()
  await page.evaluate(() => Object.assign(window, { notReloaded: true }))

  for (const id of ['bubble', 'compact']) {
    await select(request, id)
    await expect(page.locator(`ul.${id} li.message`, { hasText: text })).toBeVisible()
  }
  expect(await page.evaluate(() => 'notReloaded' in window)).toBe(true)
  const status = await request.get(`${HOST}/api/admin/templates/chat`, { headers: admin })
  expect(await status.json()).toEqual({ selected: 'compact', error: null })
})

test('the connection status takes the look of each built-in template\'s chat text', { tag: '@host' }, async ({ page, request }) => {
  // Proxies the overlay's WebSocket to the Host, so the test can drop it and refuse reconnects.
  let down = false
  const sockets: WebSocketRoute[] = []
  await page.routeWebSocket(/\/ws\/overlay\?kind=chat$/, (ws) => {
    if (down) return ws.close()
    ws.connectToServer()
    sockets.push(ws)
  })
  await select(request, 'default')
  await page.goto(`${HOST}/overlay/chat/`)
  const text = await say(request)
  const status = page.getByRole('status')
  const look = (el: Element) =>
    (({ color, fontFamily, fontSize, textShadow }) => ({ color, fontFamily, fontSize, textShadow }))(getComputedStyle(el))

  const looks: string[] = []
  for (const [id, list] of [
    ['default', 'ul.chat:not(.bubble):not(.compact)'],
    ['bubble', 'ul.bubble'],
    ['compact', 'ul.compact'],
  ]) {
    await select(request, id)
    // A reconnect's snapshot mounts the template again, hidden until its CSS is in.
    const message = page.locator(`${list} li.message`, { hasText: text }).filter({ visible: true })
    await expect(message).toBeVisible()
    down = true
    sockets.splice(0).forEach((ws) => ws.close())
    await expect(status).toHaveText(zh.overlay.status.closed)
    const expected = await message.locator('[data-field="text"]').evaluate(look)
    // White on the black scene, as each built-in template's chat text is.
    expect(expected.color).toBe('rgb(255, 255, 255)')
    expect(await status.evaluate(look)).toEqual(expected)
    looks.push(JSON.stringify(expected))
    down = false
    await expect(status).toHaveCount(0)
  }
  // The status changed with the template: the three looks differ (shadow, font size).
  expect(new Set(looks).size).toBe(3)
})

test('a template edit applies live, without reloading the overlay', { tag: '@host' }, async ({ page, request }) => {
  const text = await start(page, request, 'first')
  await page.evaluate(() => Object.assign(window, { notReloaded: true }))

  writeTemplate(template('<li class="edited"><b data-field="userName"></b> <span data-field="text"></span></li>'))
  // Messages already shown are redrawn with the new markup.
  await expect(page.locator('li.edited', { hasText: text })).toBeVisible()
  expect(await page.evaluate(() => 'notReloaded' in window)).toBe(true)
})

test('scripts in a template never run: <script>, event attributes, javascript: URLs, .js files', { tag: '@host' }, async ({
  page,
  request,
}) => {
  writeFileSync(join(TEMPLATE_DIR, 'evil.js'), "window.ran = 'file'")
  expect((await request.get(`${HOST}/templates/chat/evil.js`)).status()).toBe(404)

  await start(page, request, 'plain')
  const imageFailed = page.waitForResponse((r) => r.url().includes('/templates/chat/missing.png'))
  writeTemplate(
    template(
      `<li class="safe" onclick="window.ran = 'click'"><span data-field="text"></span></li>`,
      `<script>window.ran = 'inline'</script><script src="evil.js"></script>
       <img src="missing.png" onerror="window.ran = 'onerror'"><a href="javascript:window.ran = 'href'">link</a>`,
    ),
  )
  const item = page.locator('li.safe', { hasText: await say(request) })
  await expect(item).toBeVisible()
  await imageFailed
  await item.click()
  await page.getByText('link', { exact: true }).click()

  expect(await page.evaluate(() => (window as { ran?: string }).ran)).toBeUndefined()
  await expect(page.locator('script[src*="evil"], [onerror], [onclick], [href^="javascript"]')).toHaveCount(0)
})

test('a broken template falls back to the default one', { tag: '@host' }, async ({ page, request }) => {
  const text = await start(page, request, 'custom')
  writeTemplate('<p class="broken">no chat list here</p>')

  // The default template's markup, with the messages already shown.
  await expect(page.locator('li.message', { hasText: text })).toBeVisible()
  await expect(page.locator('.broken, li.custom')).toHaveCount(0)
  const status = await request.get(`${HOST}/api/admin/templates/chat`, { headers: admin })
  expect(await status.json()).toEqual({ selected: 'custom', error: 'template.invalid' })
})

test('the Host counts each overlay kind it serves, ignores what overlays send, and counts old URLs as other', { tag: '@host' }, async ({ page, request }) => {
  const counts = async () => (await (await request.get(`${HOST}/api/admin/overlays`, { headers: admin })).json()) as Record<string, number>
  const before = await counts()

  await page.goto(`${HOST}/overlay/chat/`)
  await expect.poll(async () => (await counts()).chat).toBe(before.chat + 1)

  // A socket that sends something stays open and changes nothing; one without a kind counts as other.
  await page.evaluate(async () => {
    const open = (url: string) =>
      new Promise<WebSocket>((resolve) => {
        const ws = new WebSocket(url)
        ws.onopen = () => resolve(ws)
      })
    const w = window as unknown as { sockets: WebSocket[] }
    w.sockets = [await open(`ws://${location.host}/ws/overlay?kind=alerts`), await open(`ws://${location.host}/ws/overlay`)]
    w.sockets[0].send('{"type":"alert"}')
  })
  await expect.poll(async () => (await counts()).alerts).toBe(before.alerts + 1)
  expect((await counts()).other).toBe(before.other + 1)

  await page.goto('about:blank')
  await expect.poll(counts).toEqual(before)
})
