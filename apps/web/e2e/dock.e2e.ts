// The docked chat panel: docked and saved on wide windows, a slide-in panel below 900px (API and `/ws/overlay` mocked).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

const chat = (seq: number, userName: string, text: string) => ({
  id: `c${seq}`,
  type: 'chat.message',
  version: 1,
  seq,
  occurredAt: '2026-11-18T10:00:00.000Z',
  source: 'twitch',
  payload: { color: null, fragments: [{ type: 'text', text }], text, userId: `u${seq}`, userLogin: userName.toLowerCase(), userName },
})

/** Opens home with the panel's saved state `saved`; returns the bodies of every save. */
async function open(page: Page, saved: { open: boolean; width: number } | null = null) {
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  const puts: unknown[] = []
  await page.route('**/api/admin/chat-dock', (r) => {
    if (r.request().method() === 'GET') return r.fulfill({ json: saved })
    saved = r.request().postDataJSON()
    puts.push(saved)
    return r.fulfill({ status: 204 })
  })
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 2,
        payload: { 'chat.message': [chat(1, 'Fish', 'hello everyone'), chat(2, 'Kai', '@me can you sing')], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
      }),
    ),
  )
  await page.goto('/admin/')
  await page.locator('[data-drawer-tile], [data-drawer-arrange]').first().waitFor()
  return puts
}

const chatButton = (page: Page) => page.getByRole('banner').getByRole('button', { name: zh.dock.title, exact: true })
const panel = (page: Page) => page.getByRole('complementary', { name: zh.dock.title })

test('closed until opened; opening docks it and saves, and the pages still fit', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const puts = await open(page)
  await expect(panel(page)).toHaveCount(0)
  await expect(chatButton(page)).toHaveAttribute('aria-expanded', 'false')

  await chatButton(page).click()
  await expect(panel(page)).toBeVisible()
  await expect(chatButton(page)).toHaveAttribute('aria-expanded', 'true')
  await expect.poll(() => puts).toEqual([{ open: true, width: 340 }])
  await expect(panel(page).getByText('hello everyone')).toBeVisible()
  expect(await scrollsSideways(page)).toBe(false)

  await panel(page).getByRole('button', { name: zh.dock.close }).click()
  await expect(panel(page)).toHaveCount(0)
  await expect(chatButton(page)).toBeFocused()
  await expect.poll(() => puts.at(-1)).toEqual({ open: false, width: 340 })
})

test('the saved state comes back after a restart, and the edge resizes from the keyboard', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  const puts = await open(page, { open: true, width: 400 })
  await expect(panel(page)).toBeVisible()
  expect(Math.round((await panel(page).boundingBox())!.width)).toBe(400)

  const edge = panel(page).getByRole('separator', { name: zh.dock.resize })
  await edge.focus()
  await page.keyboard.press('ArrowLeft')
  await expect(edge).toHaveAttribute('aria-valuenow', '416')
  await expect.poll(() => puts.at(-1)).toEqual({ open: true, width: 416 })
})

test('filters and search narrow the messages; sending waits for a later phase', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await open(page, { open: true, width: 340 })
  await panel(page).getByRole('button', { name: zh.dock.filter.mentions }).click()
  await expect(panel(page).getByText('@me can you sing')).toBeVisible()
  await expect(panel(page).getByText('hello everyone')).toHaveCount(0)

  await panel(page).getByRole('button', { name: zh.dock.filter.all }).click()
  await panel(page).getByRole('searchbox', { name: zh.dock.search }).fill('hello')
  await expect(panel(page).getByText('hello everyone')).toBeVisible()
  await expect(panel(page).getByText('@me can you sing')).toHaveCount(0)

  await expect(panel(page).getByRole('button', { name: zh.dock.filter.mods })).toBeDisabled()
  await expect(panel(page).getByRole('textbox', { name: zh.dock.send })).toBeDisabled()
})

test('"mentions me" follows signing in to another account without a reload', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await open(page, { open: true, width: 340 })
  await panel(page).getByRole('button', { name: zh.dock.filter.mentions }).click()
  await expect(panel(page).getByText('@me can you sing')).toBeVisible()

  // Routes added later win: the next status poll answers with another account.
  await page.route('**/api/admin/twitch', (r) =>
    r.fulfill({ json: { client_id: 'cid', status: { state: 'signed_in', user_id: '7', login: 'other' } } }),
  )
  await expect(panel(page).getByText('@me can you sing')).toHaveCount(0)
})

test('below 900px it slides in from the chat button, takes focus, and Esc closes it without saving', async ({ page }) => {
  await page.setViewportSize({ width: 820, height: 1000 })
  const puts = await open(page, { open: true, width: 340 })
  await expect(panel(page)).toHaveCount(0)
  await chatButton(page).click()
  await expect(panel(page)).toBeVisible()
  await expect(panel(page)).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(panel(page)).toHaveCount(0)
  await expect(chatButton(page)).toBeFocused()
  expect(puts).toEqual([])
})

test('events lists every alert that arrived, even one no OBS overlay plays', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  await page.route('**/api/admin/chat-dock', (r) => r.fulfill({ json: { open: true, width: 340 } }))
  const received = {
    id: 'a1',
    type: 'alert.received',
    version: 1,
    seq: 2,
    occurredAt: '2026-11-19T10:00:00.000Z',
    source: 'twitch',
    payload: { kind: 'follow', userId: '7', userLogin: 'fan', userName: 'Fan', durationMs: 6000, sound: { kind: 'built_in', volume: 100 } },
  }
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) => {
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: { 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
      }),
    )
    ws.send(JSON.stringify(received))
  })
  await page.goto('/admin/')
  await panel(page).getByRole('button', { name: zh.dock.filter.events }).click()
  await expect(panel(page).getByText(/Fan/)).toBeVisible()
})

test('chat from before a restart shows first, and a message a moderator deletes keeps a marker', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  await page.route('**/api/admin/chat-dock', (r) => r.fulfill({ json: { open: true, width: 340 } }))
  await page.route('**/api/admin/chat/messages', (r) =>
    r.fulfill({
      json: [
        { id: 'old1', sentAt: '2026-11-18T09:00:00.000Z', deleted: false, payload: { ...chat(9, 'Ann', 'from yesterday').payload } },
        { id: 'old2', sentAt: '2026-11-18T09:01:00.000Z', deleted: true, payload: { ...chat(9, 'Spam', '').payload } },
      ],
    }),
  )
  // Each part of the page opens its own socket; the removal goes to all of them.
  const sockets: { send: (m: string) => void }[] = []
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) => {
    sockets.push(ws)
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: { 'chat.message': [chat(1, 'Fish', 'hello everyone')], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
      }),
    )
  })
  await page.goto('/admin/')
  const log = panel(page).getByRole('log')
  await expect(log.getByText('from yesterday')).toBeVisible()
  await expect(log.getByText(zh.chat.deleted)).toHaveCount(1)
  const order = await log.locator('.msg').allTextContents()
  expect(order.findIndex((t) => t.includes('from yesterday'))).toBeLessThan(order.findIndex((t) => t.includes('hello everyone')))

  for (const ws of sockets) ws.send(JSON.stringify({ id: 'r', type: 'chat.removed', version: 1, seq: 2, occurredAt: '', source: 'twitch', payload: { messageId: 'c1' } }))
  await expect(log.getByText('hello everyone')).toHaveCount(0)
  await expect(log.getByText(zh.chat.deleted)).toHaveCount(2)
})

test('badge images, shared chat with 只看本頻道, and display settings that are saved', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  await page.route('**/api/admin/chat-dock', (r) => r.fulfill({ json: { open: true, width: 400 } }))
  const saved: unknown[] = []
  await page.route('**/api/admin/chat-display', (r) => {
    if (r.request().method() === 'GET') return r.fulfill({ json: null })
    saved.push(r.request().postDataJSON())
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/chat/badges', (r) =>
    r.fulfill({ json: [{ setId: 'moderator', id: '1', title: 'Moderator', url: 'https://static-cdn.jtvnw.net/badges/mod' }] }),
  )
  // A 1x1 transparent PNG stands in for Twitch's CDN.
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=', 'base64')
  await page.route('https://static-cdn.jtvnw.net/**', (r) => r.fulfill({ body: png, contentType: 'image/png' }))
  const mod = { ...chat(1, 'Kai', 'hello from here'), payload: { ...chat(1, 'Kai', 'hello from here').payload, twitchBadges: [{ setId: 'moderator', id: '1', info: '' }] } }
  const shared = {
    ...chat(2, 'Neko', 'hi from the other channel'),
    payload: { ...chat(2, 'Neko', 'hi from the other channel').payload, sharedFrom: { userId: '99', userLogin: 'pianocat', userName: 'PianoCat' } },
  }
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 2,
        payload: { 'chat.message': [mod, shared], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': [] },
      }),
    ),
  )
  await page.goto('/admin/')
  const log = panel(page).getByRole('log')
  await expect(log.getByRole('img', { name: 'Moderator' })).toBeVisible()
  await expect(panel(page).getByText(zh.chat.shared.replace('{channels}', 'PianoCat'))).toBeVisible()
  await expect(log.getByText(zh.chat.sharedFrom.replace('{channel}', 'PianoCat'))).toBeVisible()

  await panel(page).getByRole('button', { name: zh.chat.ownOnly }).click()
  await expect(log.getByText('hi from the other channel')).toHaveCount(0)
  await expect(log.getByText('hello from here')).toBeVisible()

  await panel(page).getByRole('button', { name: zh.chat.display.title }).click()
  await panel(page).getByRole('checkbox', { name: zh.chat.display.badges }).uncheck()
  await expect(log.getByRole('img', { name: 'Moderator' })).toHaveCount(0)
  await expect.poll(() => saved.at(-1)).toEqual({ style: 'standard', avatars: true, badges: false, time: true, textSize: 100 })
})
