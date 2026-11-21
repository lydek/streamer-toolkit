// 模擬事件 (SimulateDialog.vue) with the admin API mocked: each row sends its test event, a feature turned off cannot
// send, a failure is reported, and Esc gives focus back to the top bar button.
import { expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'

const fill = (text: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((s, [k, v]) => s.replace(`{${k}}`, String(v)), text)

test('each row sends its test event, a feature turned off cannot, and Esc returns focus', async ({ page }) => {
  const sent: { path: string; body: unknown }[] = []
  await page.route(/\/api\/admin\/(simulate\/[\w-]+|sounds\/[^/]+\/play)$/, (r) => {
    sent.push({ path: new URL(r.request().url()).pathname, body: r.request().postDataJSON() })
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/overlays', (r) => r.fulfill({ json: { chat: 1, alerts: 0, checkin: 1, sound: 0, bgm: 0, counter: 0, marathon: 0, other: 0 } }))
  await openAdmin(page, {
    modules: ['chat', 'alerts', 'checkin', 'counter'].map((id) => ({ id, enabled: true, closed: false })),
  })
  const opener = page.getByRole('banner').getByRole('button', { name: zh.simulate.dialog.title })
  await opener.click()
  const dialog = page.getByRole('dialog', { name: zh.simulate.dialog.title })
  await expect(dialog.getByRole('button', { name: zh.app.close })).toBeFocused()

  const row = (id: string) => dialog.locator(`[data-row="${id}"]`)
  await row('chat').getByRole('button', { name: zh.simulate.dialog.sendMessage }).click()
  await expect(dialog.getByRole('status')).toHaveText(fill(zh.simulate.dialog.sent, { row: zh.simulate.dialog.rows.chat, button: zh.simulate.dialog.sendMessage }))
  for (const kind of ['follow', 'subscribe', 'resubscribe', 'gift', 'cheer', 'raid'] as const) {
    await row('alerts').getByRole('button', { name: zh.alerts.kinds[kind], exact: true }).click()
  }
  await row('checkin').getByRole('button', { name: zh.simulate.dialog.fullCard }).click()
  await row('marathon').getByRole('button', { name: zh.simulate.dialog.showTest }).click()
  await expect.poll(() => sent.map((s) => s.path)).toEqual([
    '/api/admin/simulate/chat',
    ...Array(6).fill('/api/admin/simulate/alert'),
    '/api/admin/simulate/checkin',
    '/api/admin/simulate/overlay-test',
  ])
  expect(sent.at(-2)!.body).toEqual({ stamps: 10 })
  expect(sent.at(-1)!.body).toEqual({ overlay: 'marathon' })

  // The sound feature is off in this test: its row cannot send.
  await expect(row('sound')).toContainText(zh.simulate.dialog.featureOff)
  await expect(row('sound').getByRole('button', { name: zh.simulate.dialog.play })).toBeDisabled()
  await expect(row('alerts')).toContainText(zh.simulate.dialog.notInObs)

  // A failure says why.
  await page.route('**/api/admin/simulate/chat', (r) => r.fulfill({ status: 400, json: { code: 'simulate.invalid' } }))
  await row('chat').getByRole('button', { name: zh.simulate.dialog.sendMessage }).click()
  await expect(dialog.getByRole('status')).not.toContainText(zh.simulate.dialog.rows.chat)

  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(opener).toBeFocused()
})
