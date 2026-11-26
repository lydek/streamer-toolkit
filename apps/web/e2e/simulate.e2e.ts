// 模擬事件 (SimulateDialog.vue) with the admin API mocked: each row sends its test event, a feature turned off cannot
// send, a failure is reported, and Esc gives focus back to the top bar button.
import { expect, test } from '@playwright/test'
import { openAdmin, zh } from './fixtures'

const fill = (text: string, values: Record<string, string | number>) =>
  Object.entries(values).reduce((s, [k, v]) => s.replace(`{${k}}`, String(v)), text)

test('each row sends its test event, a feature turned off cannot, and Esc returns focus', async ({ page }) => {
  // Home first: the mocks' catch-all for unmocked requests is registered then, and later routes win over it.
  await openAdmin(page, {
    modules: ['chat', 'alerts', 'checkin', 'counter', 'marathon', 'sound'].map((id) => ({ id, enabled: true, closed: false })),
  })
  const sent: { path: string; body: unknown }[] = []
  await page.route(/\/api\/admin\/(simulate\/[\w-]+|sounds\/[^/]+\/play|music\/player\/test)$/, (r) => {
    sent.push({ path: new URL(r.request().url()).pathname, body: r.request().postDataJSON() })
    return r.fulfill({ status: 204 })
  })
  await page.route('**/api/admin/overlays', (r) => r.fulfill({ json: { chat: 1, alerts: 0, checkin: 1, sound: 0, bgm: 0, counter: 0, marathon: 0, break: 0, warnings: 0, leaderboard: 0, polls: 0, other: 0 } }))
  await page.route('**/api/admin/sounds', (r) =>
    r.fulfill({ json: { sounds: [{ id: 's1', label: '叮咚' }, { id: 's2', label: '鈴聲' }], slots: {}, volume: 100 } }),
  )
  await page.route('**/api/admin/checkin/settings', (r) => r.fulfill({ json: { card_slots: 10 } }))
  const opener = page.getByRole('banner').getByRole('button', { name: zh.simulate.dialog.title })
  await opener.click()
  const dialog = page.getByRole('dialog', { name: zh.simulate.dialog.title })
  await expect(dialog.getByRole('button', { name: zh.app.close })).toBeFocused()

  const row = (id: string) => dialog.locator(`[data-row="${id}"]`)
  // The dialog's own result line; the sound list adds a hidden status of its own.
  const result = dialog.locator('p[role="status"]')
  await row('chat').getByRole('button', { name: zh.simulate.dialog.sendMessage }).click()
  await expect(result).toHaveText(fill(zh.simulate.dialog.sent, { row: zh.simulate.dialog.rows.chat, button: zh.simulate.dialog.sendMessage }))
  for (const kind of ['follow', 'subscribe', 'resubscribe', 'gift', 'cheer', 'raid'] as const) {
    await row('alerts').getByRole('button', { name: zh.alerts.kinds[kind], exact: true }).click()
  }
  // The sound list opens inside the modal dialog, and the picked sound plays.
  await row('sound').getByRole('combobox').click()
  await page.getByRole('option', { name: '鈴聲' }).click()
  await row('sound').getByRole('button', { name: zh.simulate.dialog.play }).click()
  await row('checkin').getByRole('button', { name: zh.simulate.dialog.checkin }).click()
  await row('checkin').getByRole('button', { name: zh.simulate.dialog.fullCard }).click()
  await row('marathon').getByRole('button', { name: zh.simulate.dialog.showTest }).click()
  await expect.poll(() => sent.length).toBe(11)
  expect(sent).toEqual([
    { path: '/api/admin/simulate/chat', body: { text: zh.simulate.dialog.chatText } },
    { path: '/api/admin/simulate/alert', body: { kind: 'follow', amount: null } },
    { path: '/api/admin/simulate/alert', body: { kind: 'subscribe', amount: null } },
    { path: '/api/admin/simulate/alert', body: { kind: 'resubscribe', amount: 3 } },
    { path: '/api/admin/simulate/alert', body: { kind: 'gift', amount: 5 } },
    { path: '/api/admin/simulate/alert', body: { kind: 'cheer', amount: 100 } },
    { path: '/api/admin/simulate/alert', body: { kind: 'raid', amount: 20 } },
    { path: '/api/admin/sounds/s2/play', body: null },
    { path: '/api/admin/simulate/checkin', body: { stamps: 1 } },
    { path: '/api/admin/simulate/checkin', body: { stamps: 10 } },
    { path: '/api/admin/simulate/overlay-test', body: { overlay: 'marathon' } },
  ])

  // The music feature is off in this test: its row cannot send.
  await expect(row('bgm')).toContainText(zh.simulate.dialog.featureOff)
  await expect(row('bgm').getByRole('button', { name: zh.simulate.dialog.playTest })).toBeDisabled()
  await expect(row('alerts')).toContainText(zh.simulate.dialog.notInObs)

  // A failure says why.
  await page.route('**/api/admin/simulate/chat', (r) => r.fulfill({ status: 400, json: { code: 'simulate.invalid' } }))
  await row('chat').getByRole('button', { name: zh.simulate.dialog.sendMessage }).click()
  await expect(result).not.toContainText(zh.simulate.dialog.rows.chat)

  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(opener).toBeFocused()
})
