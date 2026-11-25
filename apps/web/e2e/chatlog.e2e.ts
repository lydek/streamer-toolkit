// The chat log page: 即時 and past periods, filters, search, hiding bots and 在 Twitch 開啟 (API, IPC and
// `/ws/overlay` mocked).
import { type Page, type WebSocketRoute, expect, test } from '@playwright/test'
import { invokes, mockAdmin, zh } from './fixtures'

const payload = (userName: string, text: string, over: object = {}) => ({
  userId: userName.toLowerCase(),
  userLogin: userName.toLowerCase(),
  userName,
  color: null,
  text,
  fragments: [{ type: 'text', text }],
  badges: [],
  highlighted: false,
  ...over,
})
const stored = (id: string, sentAt: string, userName: string, text: string, over: object = {}) => ({
  id,
  sentAt,
  deleted: false,
  payload: payload(userName, text, over),
})

async function open(page: Page, sockets?: WebSocketRoute[], removals: object[] = [], messages: object[] = []) {
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  // The Host's list of known bots, which 隱藏機器人 hides.
  await page.route('**/api/admin/chat/bots', (r) => r.fulfill({ json: ['nightbot', 'streamelements'] }))
  await page.route('**/api/admin/chat/messages', (r) => r.fulfill({ json: [stored('l1', '2026-11-19T12:00:00.000Z', 'Now', 'live hello')] }))
  await page.route('**/api/admin/chat/periods', (r) =>
    r.fulfill({
      json: [
        { kind: 'stream', from: '2026-11-16T12:00:00.000Z', to: '2026-11-16T15:00:00.000Z', firstAt: '2026-11-16T12:01:00.000Z', messages: 3 },
        { kind: 'offline', from: null, to: '2026-11-16T12:00:00.000Z', firstAt: '2026-11-15T09:00:00.000Z', messages: 1 },
      ],
    }),
  )
  const logQueries: string[] = []
  await page.route('**/api/admin/chat/log**', (r) => {
    logQueries.push(new URL(r.request().url()).search)
    return r.fulfill({
      json: {
        messages: [
          stored('m1', '2026-11-16T12:01:00.000Z', 'Fish', '@me can you sing'),
          stored('m2', '2026-11-16T12:03:00.000Z', 'Nightbot', 'Follow the rules'),
          stored('m3', '2026-11-16T12:05:00.000Z', 'Kai', '!打卡', { badges: ['moderator'] }),
          stored('m4', '2026-11-16T12:06:00.000Z', 'Newbie', '第一次來', { firstChat: true }),
        ],
        events: [{ at: '2026-11-16T12:02:00.000Z', kind: 'redemption', userName: 'Berry', reward: '喝水提醒' }],
        truncated: false,
      },
    })
  })
  if (sockets) {
    await page.routeWebSocket('**/ws/overlay', ws => {
      sockets.push(ws)
      ws.send(JSON.stringify({ type: 'snapshot', version: 1, seq: 10, payload: {
        'chat.message': messages, 'chat.removed': removals, 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'chat.text_size': [],
      } }))
    })
  }
  await page.goto('/admin/chat-log')
  return logQueries
}

const lines = (page: Page) => page.getByRole('list', { name: zh.chatLog.lines }).getByRole('listitem')

test('即時 shows chat from before the page opened; a past stream shows its messages and events in order', async ({ page }) => {
  const queries = await open(page)
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText('live hello')

  await page.getByRole('combobox').click()
  await expect(page.getByRole('option', { name: /^11\/15 .*（沒有直播）$/ })).toBeVisible()
  await page.getByRole('option', { name: /^11\/16 .*的直播$/ }).click()
  await expect(lines(page)).toHaveCount(5)
  await expect(lines(page).nth(1)).toContainText('Berry 兌換了「喝水提醒」')
  expect(queries).toEqual(['?from=2026-11-16T12%3A00%3A00.000Z&to=2026-11-16T15%3A00%3A00.000Z'])
})

test('filters, search and hiding bots narrow the lines together', async ({ page }) => {
  await open(page)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: /^11\/16 .*的直播$/ }).click()
  await expect(lines(page)).toHaveCount(5)

  await page.getByRole('button', { name: zh.chatLog.filter.first }).click()
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText(`${zh.chat.firstChat}`)
  await page.getByRole('button', { name: zh.chatLog.filter.mentions }).click()
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText('@me can you sing')
  await page.getByRole('button', { name: zh.chatLog.filter.redemptions }).click()
  await expect(lines(page)).toContainText('喝水提醒')
  await page.getByRole('button', { name: zh.chatLog.filter.mods }).click()
  await expect(lines(page)).toContainText('!打卡')

  await page.getByRole('button', { name: zh.chatLog.filter.all }).click()
  await page.getByRole('checkbox', { name: zh.chatLog.hideBots }).check()
  await expect(lines(page)).toHaveCount(4)
  await page.getByRole('searchbox', { name: zh.chatLog.search }).fill('sing')
  await expect(lines(page)).toHaveCount(1)
})

test('在 Twitch 開啟 hands only the login to the desktop', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: zh.chatLog.openTwitch }).click()
  await expect.poll(() => invokes(page, 'open_twitch_chat')).toEqual([{ cmd: 'open_twitch_chat', args: { login: 'me' } }])
})

test('settings say where 第一次聊天 comes from', async ({ page }) => {
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
  await page.goto('/admin/settings/connections')
  await expect(page.getByText(zh.twitch.firstChat.fallback)).toBeVisible()
})

test('reconnect refreshes a selected closed stream after moderation while disconnected', async ({ page }) => {
  const sockets: WebSocketRoute[] = []
  await open(page, sockets)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: /^11\/16 / }).click()
  await expect(lines(page).first()).toContainText('@me can you sing')
  for (const ws of sockets.splice(0)) await ws.close()
  await page.route('**/api/admin/chat/log**', r => r.fulfill({ json: {
    messages: [{ ...stored('m1', '2026-11-16T12:01:00.000Z', 'Fish', ''), deleted: true }],
    events: [], truncated: false,
  } }))
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText(zh.chat.deleted)
})

test('reconnect applies snapshot removals to a past stream before they are stored', async ({ page }) => {
  const sockets: WebSocketRoute[] = []
  const removals: object[] = []
  const queries = await open(page, sockets, removals)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: /^11\/16 / }).click()
  await expect(lines(page).first()).toContainText('@me can you sing')
  removals.push({
    id: 'r1', type: 'chat.removed', version: 1, seq: 9, source: 'twitch',
    occurredAt: '2026-11-16T13:00:00Z', payload: { messageId: 'm1' },
  })
  for (const ws of sockets.splice(0)) await ws.close()
  await expect.poll(() => queries.length).toBeGreaterThan(1)
  await expect(lines(page).first()).toContainText(zh.chat.deleted)
  await expect(page.getByText('@me can you sing')).toHaveCount(0)
})

test('live chat retries a failed history read after reconnect', async ({ page }) => {
  const sockets: WebSocketRoute[] = []
  await open(page, sockets)
  await expect(lines(page)).toContainText('live hello')
  await expect.poll(() => sockets.length).toBeGreaterThan(0)
  let recovered = false
  await page.route('**/api/admin/chat/messages', (r) => !recovered
    ? r.fulfill({ status: 500, json: { code: 'chat.storage_failed' } })
    : r.fulfill({ json: [stored('l2', '2026-11-19T12:01:00.000Z', 'Now', 'recovered history')] }))
  for (const ws of sockets.splice(0)) await ws.close()
  const notice = page.getByRole('main').getByText(zh.chat.historyRetry)
  await expect(notice).toBeVisible()
  recovered = true
  await expect(lines(page)).toContainText('recovered history')
  await expect(notice).toHaveCount(0)
})

test('a past period uses first-chat marks from the reconnect snapshot', async ({ page }) => {
  const sockets: WebSocketRoute[] = []
  const messages: object[] = []
  await open(page, sockets, [], messages)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: /^11\/16 / }).click()
  await expect(lines(page).first()).toContainText('@me can you sing')
  messages.push({
    id: 'm1', type: 'chat.message', version: 1, seq: 9, source: 'twitch',
    occurredAt: '2026-11-16T12:01:00Z',
    payload: payload('Fish', '@me can you sing', { firstChat: true }),
  })
  for (const ws of sockets.splice(0)) await ws.close()
  await expect(lines(page).first()).toContainText(zh.chat.firstChat)
})

test('an offline period appears after its first message is stored', async ({ page }) => {
  const sockets: WebSocketRoute[] = []
  await open(page, sockets)
  await expect(lines(page)).toContainText('live hello')
  await expect.poll(() => sockets.length).toBeGreaterThan(0)
  await page.route('**/api/admin/chat/periods', (r) => r.fulfill({ json: [{
    kind: 'offline', from: '2026-11-19T11:00:00Z', to: null,
    firstAt: '2026-11-19T12:02:00Z', messages: 1,
  }] }))
  for (const ws of sockets) ws.send(JSON.stringify({
    id: 'offline-first', type: 'chat.message', version: 1, seq: 11, source: 'twitch',
    occurredAt: '2026-11-19T12:02:00Z', payload: payload('Fish', 'offline hello'),
  }))
  await expect(lines(page).filter({ hasText: 'offline hello' })).toBeVisible()
  await page.getByRole('combobox').click()
  await expect(page.getByRole('option', { name: /^11\/19/ })).toBeVisible()
})
