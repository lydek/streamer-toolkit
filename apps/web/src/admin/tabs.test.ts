// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { FEATURES } from './features'
import { UNBUILT_TILES } from './home'
import { GROUPS, SOON_PAGES, firstPage, fittingTabs, groupOf, groupUsable } from './tabs'
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

describe('groupUsable', () => {
  const interact = GROUPS.find((g) => g.id === 'interact')!
  const song = GROUPS.find((g) => g.id === 'song')!

  it('greys a group with no built page, and one whose built pages are all turned off', () => {
    expect(groupUsable(song)).toBe(false)
    expect(groupUsable(interact)).toBe(true)
    const built = interact.pages.filter((p) => !p.soon).map((p) => p.path)
    expect(groupUsable(interact, new Set(built))).toBe(false)
  })
})

describe('what counts as built', () => {
  it('agrees between the tabs, the features and the grey home tiles', () => {
    const soon = new Set(SOON_PAGES.map((p) => p.path))
    for (const f of FEATURES) {
      if (f.fixed) continue
      // A feature without a module is not built: all its pages are grey. A built one has a page to open.
      if (!f.module) expect(f.pages.every((p) => soon.has(p)), f.id).toBe(true)
      else expect(f.pages.some((p) => !soon.has(p)), f.id).toBe(true)
    }
    const tilePages = SOON_PAGES.filter((p) => p.soon!.tile).map((p) => p.path.slice(1))
    expect(UNBUILT_TILES.map((t) => t.id)).toEqual(tilePages)
  })

  it('sends a page not built yet home', () => {
    for (const p of SOON_PAGES) expect(router.resolve(p.path).matched[0]?.redirect, p.path).toBe('/')
  })
})
