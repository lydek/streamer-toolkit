// The stream stage card on home: before, live or after, following Twitch (API and `/ws/overlay` mocked).
import { type Page, expect, test } from '@playwright/test'
import type { components } from '../src/api/schema'
import { mockAdmin, zh } from './fixtures'

type Schemas = components['schemas']

const summary = (endedAt: string | null, endedSinceLaunch = true): Schemas['StreamSummary'] => ({
  startedAt: '2026-11-18T11:58:00.000Z',
  endedAt,
  endedSinceLaunch,
  peakViewers: 58,
  follows: 3,
  subs: 5,
  bits: 1200,
  raids: 1,
  redemptions: 42,
  chatMessages: 1204,
  firstChats: 8,
})

/** Opens home with Twitch's status `stream` in the snapshot; `push` sends a later status. */
async function open(
  page: Page,
  stream: Schemas['StreamStatusPayload'],
  streamSummary: Schemas['StreamSummary'] | null,
  more: Parameters<typeof mockAdmin>[1] & { obs?: Schemas['ObsStatusPayload']; overlays?: () => Schemas['OverlayCounts'] } = {},
) {
  const { obs, overlays, ...admin } = more
  const state = await mockAdmin(page, { streamSummary, ...admin })
  if (overlays) await page.route('**/api/admin/overlays', (r) => r.fulfill({ json: overlays() }))
  let push: (s: Schemas['StreamStatusPayload']) => void = () => {}
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) => {
    const event = (seq: number, payload: Schemas['StreamStatusPayload']) => ({
      id: `s${seq}`,
      type: 'stream.status',
      version: 1,
      seq,
      occurredAt: '',
      source: 'twitch',
      payload,
    })
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [],
          'obs.status': obs ? [{ id: 'o', type: 'obs.status', version: 1, seq: 1, occurredAt: '', source: 'system', payload: obs }] : [],
          'obs.state': [],
          'presentation.queue': [],
          'stream.status': [event(1, stream)],
        },
      }),
    )
    let seq = 1
    push = (s) => ws.send(JSON.stringify(event(++seq, s)))
  })
  await page.goto('/admin/')
  return { state, push: (s: Schemas['StreamStatusPayload']) => push(s) }
}

const stage = (page: Page) => page.getByRole('region', { name: zh.stage.title })
const pressed = (page: Page, name: string) => stage(page).getByRole('button', { name, exact: true })

test("while live it shows how long, the peak and this stream's numbers", async ({ page }) => {
  await open(page, { state: 'live', startedAt: '2026-11-18T11:58:00.000Z', viewers: 50 }, summary(null))
  await expect(pressed(page, zh.stage.live)).toHaveAttribute('aria-pressed', 'true')
  await expect(stage(page).getByText(/^\d+:\d\d:\d\d · 最高 58 位觀眾$/)).toBeVisible()
  await expect(stage(page).getByText('1,200')).toBeVisible()
  await expect(stage(page).getByText(zh.stage.stats.redemptions)).toBeVisible()
})

test('a stream that ended this run shows its summary, and the next start switches back to live', async ({ page }) => {
  const { state, push } = await open(page, { state: 'offline' }, summary('2026-11-18T14:39:00.000Z'))
  await expect(pressed(page, zh.stage.after)).toHaveAttribute('aria-pressed', 'true')
  await expect(stage(page).getByText(/–.* · 2:41 · 最高 58 位觀眾$/)).toBeVisible()

  state.streamSummary = { ...summary(null), startedAt: '2026-11-19T11:00:00.000Z', follows: 0 }
  push({ state: 'live', startedAt: '2026-11-19T11:00:00.000Z' })
  await expect(pressed(page, zh.stage.live)).toHaveAttribute('aria-pressed', 'true')
})

test('after a restart it shows before, and the last stream stays one click away', async ({ page }) => {
  await open(page, { state: 'offline' }, summary('2026-11-18T14:39:00.000Z', false))
  await expect(pressed(page, zh.stage.before)).toHaveAttribute('aria-pressed', 'true')
  await expect(stage(page).getByText(zh.stage.checks.obs.bad)).toBeVisible()
  await pressed(page, zh.stage.after).click()
  await expect(stage(page).getByText('42', { exact: true })).toBeVisible()
})

test('before the first stream there is nothing to sum up', async ({ page }) => {
  await open(page, { state: 'unknown' }, null)
  await pressed(page, zh.stage.after).click()
  await expect(stage(page).getByText(zh.stage.noStream)).toBeVisible()
})

test('before the stream it checks Twitch, EventSub, OBS and each overlay, with a way to fix what is not ready', async ({ page }) => {
  let overlays = { chat: 0, alerts: 0, checkin: 0, sound: 0, other: 1 }
  await open(page, { state: 'offline' }, null, {
    overlays: () => overlays,
    overview: { client_id: 'cid', status: { state: 'signed_in', user_id: '42', login: 'me' } },
    obs: { state: 'connected', reason: null },
  })
  await expect(stage(page).getByText('3 / 4 好了')).toBeVisible()
  await expect(stage(page).getByText(zh.stage.checks.overlay.chat.bad)).toBeVisible()
  await expect(stage(page).getByRole('link', { name: zh.stage.checks.fix })).toHaveAttribute('href', /\/chat$/)

  // The overlay is added to OBS: the next poll sees it.
  overlays = { ...overlays, chat: 1 }
  await expect(stage(page).getByText('4 / 4 好了')).toBeVisible()
  await expect(stage(page).getByRole('link', { name: zh.stage.checks.fix })).toHaveCount(0)
})

test('signed out and OBS away, the checks say so and fixing leads to the connection settings', async ({ page }) => {
  await open(page, { state: 'unknown' }, null)
  await expect(stage(page).getByText(zh.stage.checks.twitch.bad)).toBeVisible()
  await expect(stage(page).getByText(zh.stage.checks.obs.bad)).toBeVisible()
  await stage(page).getByRole('listitem').filter({ hasText: zh.stage.checks.obs.bad }).getByRole('link').click()
  await expect(page).toHaveURL(/\/settings\/connections#obs$/)
})
