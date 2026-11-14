// The chat overlay against a real Host (crates/host/examples/e2e_host.rs) and its watched template folder.
import { randomUUID } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { type APIRequestContext, type Page, expect, test } from '@playwright/test'
import { HOST_PORT } from './ports'

const HOST = `http://127.0.0.1:${HOST_PORT}`
const TEMPLATE_DIR = join(process.env.E2E_DATA_DIR!, 'templates', 'chat')
const admin = { Authorization: `Bearer ${process.env.E2E_ADMIN_TOKEN}`, Origin: HOST }

// One Host and one template folder: run in order.
test.describe.configure({ mode: 'serial' })
// The Host polls the folder once a second.
test.use({ actionTimeout: 10_000 })
expect.configure({ timeout: 10_000 })

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

/** Each test starts from its own template, so it passes alone and in any order. */
async function start(page: Page, request: APIRequestContext, itemClass: string) {
  writeTemplate(template(`<li class="${itemClass}"><span data-field="text"></span></li>`))
  await page.goto(`${HOST}/overlay/chat/`)
  const text = await say(request)
  await expect(page.locator(`li.${itemClass}`, { hasText: text })).toBeVisible()
  return text
}

test('a template edit applies live, without reloading the overlay', async ({ page, request }) => {
  const text = await start(page, request, 'first')
  await page.evaluate(() => Object.assign(window, { notReloaded: true }))

  writeTemplate(template('<li class="edited"><b data-field="userName"></b> <span data-field="text"></span></li>'))
  // Messages already shown are redrawn with the new markup.
  await expect(page.locator('li.edited', { hasText: text })).toBeVisible()
  expect(await page.evaluate(() => 'notReloaded' in window)).toBe(true)
})

test('scripts in a template never run: <script>, event attributes, javascript: URLs, .js files', async ({
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

test('a broken template falls back to the default one', async ({ page, request }) => {
  const text = await start(page, request, 'custom')
  writeTemplate('<p class="broken">no chat list here</p>')

  // The default template's markup, with the messages already shown.
  await expect(page.locator('li.message', { hasText: text })).toBeVisible()
  await expect(page.locator('.broken, li.custom')).toHaveCount(0)
  const status = await request.get(`${HOST}/api/admin/templates/chat`, { headers: admin })
  expect(await status.json()).toEqual({ error: 'template.invalid' })
})
