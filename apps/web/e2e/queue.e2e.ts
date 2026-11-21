// The presentation queue on home and the emergency stop in the app bar (API and `/ws/overlay` mocked).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

/** Opens home with the queue status `queue` in the snapshot; returns the requests to clear and stop. */
async function openWithQueue(page: Page, queue: unknown) {
  await mockAdmin(page)
  const calls: string[] = []
  await page.route('**/api/admin/queue/clear', (r) => (calls.push('clear'), r.fulfill({ json: { cancelled: 2 } })))
  await page.route('**/api/admin/queue/stop', (r) => (calls.push('stop'), r.fulfill({ json: { stopped: true, cancelled: 2 } })))
  await page.route('**/api/admin/queue/skip', (r) => (calls.push('skip'), r.fulfill({ json: { skipped: true } })))
  await page.routeWebSocket(/\/ws\/overlay$/, (ws) =>
    ws.send(
      JSON.stringify({
        type: 'snapshot',
        version: 1,
        seq: 1,
        payload: {
          'chat.message': [],
          'obs.status': [],
          'obs.state': [],
          'presentation.queue': [{ id: 'q', type: 'presentation.queue', version: 1, seq: 1, occurredAt: '', source: 'system', payload: queue }],
          'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'screen.warnings': [], 'screen.appearance': [], 'leaderboard.update': [], 'chat.text_size': [],
        },
      }),
    ),
  )
  await page.goto('/admin/')
  // The queue starts as a small tile on home; it opens in place into its block.
  await page.locator('#tiles [data-expand="queue"]').click()
  return calls
}

const queue = (startedAt: string, problem: string | null = null, soundOnAir = true) => ({
  current: { stage: 'alerts', subject: { kind: 'alert', alert: 'subscribe', viewer: '阿凱' }, showMs: 6000, startedAt, problem },
  waiting: [
    { stage: 'checkin', subject: { kind: 'card', viewer: '小魚', stamps: 7, slots: 10 }, showMs: 5000, onAir: true },
    { stage: 'sound', subject: { kind: 'sound', sound: '喝水', viewer: '橘子' }, showMs: 1500, onAir: soundOnAir },
  ],
  gapMs: 1000,
})

test('home shows what plays, what waits and about how long, and 清空排隊 clears it', async ({ page }) => {
  const calls = await openWithQueue(page, queue(new Date().toISOString()))
  const area = page.getByRole('region', { name: zh.queue.title })
  await expect(area.getByRole('listitem')).toHaveText([
    `${zh.queue.playing}特效：阿凱 訂閱了${zh.queue.skip}`,
    '2打卡卡片：小魚 7/10',
    '3音效：喝水（橘子兌換）',
  ])
  // About 7 s left of the alert, then 6 s and 2.5 s.
  await expect(area.getByText(/排隊 2 個 · 約 1[45] 秒/)).toBeVisible()

  await area.getByRole('button', { name: zh.queue.clear }).click()
  await expect(page.getByText(zh.queue.cleared.replace('{cancelled}', '2'))).toBeVisible()
  expect(calls).toEqual(['clear'])
})

test('an empty queue says so', async ({ page }) => {
  await openWithQueue(page, { current: null, waiting: [], gapMs: 1000 })
  const area = page.getByRole('region', { name: zh.queue.title })
  await expect(area.getByText(zh.queue.summaryIdle.replace('{waiting}', '0'))).toBeVisible()
  await expect(area.getByRole('listitem')).toHaveCount(0)
})

test('the emergency stop in the app bar stops everything and says what it stopped', async ({ page }) => {
  const calls = await openWithQueue(page, queue(new Date().toISOString()))
  await page.getByRole('banner').getByRole('button', { name: zh.queue.stop }).click()
  await expect(page.getByText(zh.queue.stopped.replace('{stopped}', '1').replace('{cancelled}', '2'))).toBeVisible()
  expect(calls).toEqual(['stop'])
})

test('跳過 ends only the one playing', async ({ page }) => {
  const calls = await openWithQueue(page, queue(new Date().toISOString()))
  const area = page.getByRole('region', { name: zh.queue.title })
  await area.getByRole('button', { name: zh.queue.skip }).click()
  await expect.poll(() => calls).toEqual(['skip'])
})

test('what the Host can see went wrong shows on the item', async ({ page }) => {
  await openWithQueue(page, queue(new Date().toISOString(), 'sound_missing', false))
  const items = page.getByRole('region', { name: zh.queue.title }).getByRole('listitem')
  await expect(items.nth(0)).toContainText(zh.queue.problem.sound_missing)
  await expect(items.nth(1)).not.toContainText(zh.queue.offAir)
  await expect(items.nth(2)).toContainText(zh.queue.offAir)
})
