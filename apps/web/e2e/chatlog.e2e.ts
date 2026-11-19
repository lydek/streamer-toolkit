// The chat log page: 即時 and past periods, filters, search, hiding bots and 在 Twitch 開啟 (API, IPC and
// `/ws/overlay` mocked).
import { type Page, expect, test } from '@playwright/test'
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

async function open(page: Page) {
  await mockAdmin(page, { overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } } })
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
        ],
        events: [{ at: '2026-11-16T12:02:00.000Z', kind: 'redemption', userName: 'Berry', reward: '喝水提醒' }],
        truncated: false,
      },
    })
  })
  await page.goto('/admin/chat-log')
  return logQueries
}

const lines = (page: Page) => page.getByRole('list', { name: zh.chatLog.lines }).getByRole('listitem')

test('即時 shows chat from before the page opened; a past stream shows its messages and events in order', async ({ page }) => {
  const queries = await open(page)
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText('live hello')

  await page.getByRole('combobox').click()
  await expect(page.getByRole('option', { name: '11/15（沒有直播）' })).toBeVisible()
  await page.getByRole('option', { name: '11/16 的直播' }).click()
  await expect(lines(page)).toHaveCount(4)
  await expect(lines(page).nth(1)).toContainText('Berry 兌換了「喝水提醒」')
  expect(queries).toEqual(['?from=2026-11-16T12%3A00%3A00.000Z&to=2026-11-16T15%3A00%3A00.000Z'])
})

test('filters, search and hiding bots narrow the lines together', async ({ page }) => {
  await open(page)
  await page.getByRole('combobox').click()
  await page.getByRole('option', { name: '11/16 的直播' }).click()
  await expect(lines(page)).toHaveCount(4)

  await page.getByRole('button', { name: zh.chatLog.filter.mentions }).click()
  await expect(lines(page)).toHaveCount(1)
  await expect(lines(page)).toContainText('@me can you sing')
  await page.getByRole('button', { name: zh.chatLog.filter.redemptions }).click()
  await expect(lines(page)).toContainText('喝水提醒')
  await page.getByRole('button', { name: zh.chatLog.filter.mods }).click()
  await expect(lines(page)).toContainText('!打卡')

  await page.getByRole('button', { name: zh.chatLog.filter.all }).click()
  await page.getByRole('checkbox', { name: zh.chatLog.hideBots }).check()
  await expect(lines(page)).toHaveCount(3)
  await page.getByRole('searchbox', { name: zh.chatLog.search }).fill('sing')
  await expect(lines(page)).toHaveCount(1)
})

test('在 Twitch 開啟 hands only the login to the desktop', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: zh.chatLog.openTwitch }).click()
  await expect.poll(() => invokes(page, 'open_twitch_chat')).toEqual([{ cmd: 'open_twitch_chat', args: { login: 'me' } }])
})
