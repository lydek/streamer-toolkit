// The presentation queue on home and the emergency stop in the app bar (API and `/ws/overlay` mocked).
import { type Page, expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

/** Opens home with the queue status `queue` in the snapshot; returns the requests to clear and stop. */
async function openWithQueue(page: Page, queue: unknown) {
  await mockAdmin(page)
  const calls: string[] = []
  await page.route('**/api/admin/queue/clear', (r) => (calls.push('clear'), r.fulfill({ json: { cancelled: 2 } })))
  await page.route('**/api/admin/queue/stop', (r) => (calls.push('stop'), r.fulfill({ json: { stopped: true, cancelled: 2 } })))
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
          'stream.status': [],
        },
      }),
    ),
  )
  await page.goto('/admin/')
  return calls
}

const queue = (startedAt: string) => ({
  current: { stage: 'alerts', label: '阿凱', showMs: 6000, startedAt },
  waiting: [
    { stage: 'checkin', label: '小魚', showMs: 5000 },
    { stage: 'sound', label: '喝水', showMs: 1500 },
  ],
  gapMs: 1000,
})

test('home shows what plays, what waits and about how long, and 清空排隊 clears it', async ({ page }) => {
  const calls = await openWithQueue(page, queue(new Date().toISOString()))
  const area = page.getByRole('region', { name: zh.queue.title })
  await expect(area.getByRole('listitem')).toHaveText([
    `${zh.queue.playing}${zh.queue.item.alerts.replace('{label}', '阿凱')}`,
    `2${zh.queue.item.checkin.replace('{label}', '小魚')}`,
    `3${zh.queue.item.sound.replace('{label}', '喝水')}`,
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
