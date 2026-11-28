// 頻道點數 › 獎勵 page layout, with the admin API mocked (the rewards themselves are tested in the Host).
import { expect, test } from '@playwright/test'
import { mockAdmin, zh } from './fixtures'

/** Where an element's text starts on screen, padding left out. */
const textLeft = (el: Element) => {
  const range = document.createRange()
  range.selectNodeContents(el)
  return range.getBoundingClientRect().left
}

test('the rewards count under the heading lines up with it, not with the card edge', async ({ page }) => {
  await mockAdmin(page)
  await page.route('**/api/admin/rewards', (r) => r.fulfill({ json: [] }))
  await page.goto('/admin/rewards')
  const card = page.getByRole('region', { name: zh.rewards.heading })
  const sub = card.locator('.tbl-top .sub').first()
  await expect(sub).toBeVisible()
  for (const [width, height] of [
    [1440, 900],
    [800, 900],
    [390, 844],
  ] as const) {
    await page.setViewportSize({ width, height })
    const [head, count] = await Promise.all([card.getByRole('heading', { name: zh.rewards.heading }).evaluate(textLeft), sub.evaluate(textLeft)])
    expect(Math.abs(count - head), `${width}px`).toBeLessThan(1)
  }
})
