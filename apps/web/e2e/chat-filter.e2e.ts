// 聊天過濾 layout in a real browser. The page is unbuilt (grey in Vite mode `e2e`), so it is opened on the harness
// page (e2e/harness/), API mocked.
import { expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

test('on a phone each rule has its switch beside its name and the action under it; wider, it stays as it was', async ({ page }) => {
  await mockAdmin(page)
  await page.goto('/e2e/harness/index.html#/chat-filter')
  const rows = page.locator('.rule-row')
  await expect(rows).toHaveCount(6)
  // Per rule: where the name, the switch and the action picker sit.
  const boxes = () =>
    rows.evaluateAll((els) =>
      els.map((row) => {
        const box = (sel: string) => {
          const b = row.querySelector(sel)!.getBoundingClientRect()
          return { mid: b.top + b.height / 2, top: b.top, bottom: b.bottom, left: b.left, right: b.right }
        }
        return { name: box('.label'), toggle: box('.p-toggleswitch'), action: box('.action') }
      }),
    )
  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 })
    for (const r of await boxes()) {
      expect(Math.abs(r.toggle.mid - r.name.mid), `${width}px`).toBeLessThan(12)
      expect(r.action.top, `${width}px`).toBeGreaterThanOrEqual(r.toggle.bottom - 1)
      expect(Math.max(r.toggle.right, r.action.right), `${width}px`).toBeLessThanOrEqual(width)
    }
  }
  // Wide: the name on the left, the action and the switch on one line to its right.
  await page.setViewportSize({ width: 1280, height: 900 })
  for (const r of await boxes()) {
    expect(r.action.left).toBeGreaterThan(r.name.right)
    expect(Math.abs(r.toggle.mid - r.action.mid)).toBeLessThan(12)
  }
  await expect(page.getByRole('heading', { name: zh.chatFilter.rules })).toBeVisible()
})
