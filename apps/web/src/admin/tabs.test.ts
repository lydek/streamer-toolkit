// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { GROUPS, firstPage, fittingTabs, groupOf } from './tabs'
import { router } from './router'

describe('groupOf', () => {
  it('finds the group of a page and of a sub-page', () => {
    expect(groupOf('/')?.id).toBe('home')
    expect(groupOf('/checkin')?.id).toBe('interact')
  })

  it('has no group for settings or unknown paths', () => {
    expect(groupOf('/settings')).toBeUndefined()
    expect(groupOf('/nope')).toBeUndefined()
  })

  it('lists only routes that exist, each in one group', () => {
    const paths = GROUPS.flatMap((g) => g.pages.map((p) => p.path))
    expect(new Set(paths).size).toBe(paths.length)
    for (const path of paths) expect(router.resolve(path).matched[0]?.path).toBe(path)
  })
})

describe('fittingTabs', () => {
  it('keeps every tab and no "More" when they all fit with 編輯分頁', () => {
    // 100 + 10 + 100 + 10 + 100 + 10 + 90 = 420.
    expect(fittingTabs([100, 100, 100], 10, 420, 80, 90)).toBe(3)
  })

  it('leaves room for "More" once the tabs and 編輯分頁 do not fit, even if "More" is narrower', () => {
    // All tabs with "More" would need 410, but "More" cannot be empty; 100 + 10 + 100 + 10 + 80 = 300 fits.
    expect(fittingTabs([100, 100, 100], 10, 419, 80, 90)).toBe(2)
  })

  it('can put every tab into "More"', () => {
    expect(fittingTabs([100, 100], 10, 90, 80, 90)).toBe(0)
  })
})

describe('firstPage', () => {
  const media = GROUPS.find((g) => g.id === 'media')!

  it('skips coming-soon pages for the first built one, and hidden ones too', () => {
    expect(firstPage(media)).toBe('/scenes')
    expect(firstPage(media, new Set(['/scenes']))).toBe('/music')
    // Hidden alerts, then the coming-soon polls page, are skipped.
    const interact = GROUPS.find((g) => g.id === 'interact')!
    expect(firstPage(interact, new Set(['/alerts']))).toBe('/checkin')
  })
})
