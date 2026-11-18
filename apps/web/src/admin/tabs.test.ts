// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { GROUPS, fittingTabs, groupOf } from './tabs'
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
  it('keeps every tab and no "More" when they all fit', () => {
    expect(fittingTabs([100, 100, 100], 10, 320, 80)).toBe(3)
  })

  it('leaves room for "More" once one tab does not fit', () => {
    // 100 + 10 + 100 + 10 + 80 = 300 fits; a third tab would need 410.
    expect(fittingTabs([100, 100, 100], 10, 319, 80)).toBe(2)
  })

  it('can put every tab into "More"', () => {
    expect(fittingTabs([100, 100], 10, 90, 80)).toBe(0)
  })
})
