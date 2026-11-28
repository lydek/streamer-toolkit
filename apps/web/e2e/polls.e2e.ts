// 投票與預測 page layout, with the admin API mocked (polls and predictions themselves are tested in the Host).
import { expect, test } from '@playwright/test'
import { mockAdmin, scrollsSideways, zh } from './fixtures'

test('the new poll form fits a 320px window: the option boxes stay inside the card', async ({ page }) => {
  await mockAdmin(page)
  await page.route('**/api/admin/polls', (r) => r.fulfill({ json: { poll: null, prediction: null } }))
  await page.route('**/api/admin/polls/templates', (r) => r.fulfill({ json: [] }))
  await page.setViewportSize({ width: 320, height: 700 })
  await page.goto('/admin/polls')
  const options = page.getByRole('group', { name: zh.polls.options })
  await expect(options).toBeVisible()
  expect(await scrollsSideways(page)).toBe(false)
  const [group, first] = await Promise.all([options.boundingBox(), options.getByRole('textbox').first().boundingBox()])
  expect(first!.x + first!.width).toBeLessThanOrEqual(group!.x + group!.width + 1)
  // Wider windows keep the full 20rem boxes.
  for (const width of [800, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    expect((await options.getByRole('textbox').first().boundingBox())!.width, `${width}px`).toBeCloseTo(320, 0)
  }
})
