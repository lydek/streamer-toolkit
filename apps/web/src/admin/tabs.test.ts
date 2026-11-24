// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { FEATURES, shareModules } from './features'
import { UNBUILT_TILES } from './home'
import { GROUPS, SOON_PAGES, categoryOf, categoryTarget, closedPageTarget, firstPage, fittingTabs, groupOf, groupUsable } from './tabs'
import { router } from './router'

// The alerts feature's pages: turning it off hides all of them.
const ALERT_PAGES = FEATURES.find((f) => f.id === 'alerts')!.pages

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
    // Hidden scenes and mixer, then the coming-soon modes page, are skipped.
    expect(firstPage(media, new Set(['/scenes', '/mixer']))).toBe('/obs-sources')
    // Hidden alerts, then the coming-soon polls page, are skipped.
    const interact = GROUPS.find((g) => g.id === 'interact')!
    expect(firstPage(interact, new Set(ALERT_PAGES))).toBe('/checkin')
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
      // A feature without a module (or others' modules it works with) is not built: all its pages are grey. A built
      // one has a page to open.
      if (!f.module && !f.openWith) expect(f.pages.every((p) => soon.has(p)), f.id).toBe(true)
      else expect(f.pages.some((p) => !soon.has(p)), f.id).toBe(true)
    }
    const tilePages = SOON_PAGES.filter((p) => p.soon!.tile).map((p) => p.path.slice(1))
    expect(UNBUILT_TILES.map((t) => t.id)).toEqual(tilePages)
  })

  it('sends a page not built yet home', () => {
    for (const p of SOON_PAGES) expect(router.resolve(p.path).matched[0]?.redirect, p.path).toBe('/')
  })
})

describe('categories', () => {
  it('finds the category of a page; home, songs and unknown paths have none', () => {
    expect(categoryOf('/card-style')?.id).toBe('checkin')
    expect(categoryOf('/redemptions')?.id).toBe('points')
    expect(categoryOf('/')).toBeUndefined()
    expect(categoryOf('/songs')).toBeUndefined()
    expect(categoryOf('/nope')).toBeUndefined()
  })

  it('lists a grouped page in exactly one of its categories', () => {
    for (const g of GROUPS.filter((g) => g.categories))
      expect(g.categories!.flatMap((c) => c.pages)).toEqual(g.pages)
  })

  it('goes to the page last seen in a category while it can open, else its first open page', () => {
    const checkin = categoryOf('/checkin')!
    expect(categoryTarget(checkin)).toBe('/checkin')
    expect(categoryTarget(checkin, new Set(), '/card-style')).toBe('/card-style')
    expect(categoryTarget(checkin, new Set(['/card-style']), '/card-style')).toBe('/checkin')
    // 獎勵 is not built yet, so 頻道點數 opens 兌換時播放 (the first built page).
    expect(categoryTarget(categoryOf('/redemptions')!)).toBe('/reward-playback')
  })

  it('has nowhere to go for a category with no built page', () => {
    expect(categoryTarget(categoryOf('/polls')!)).toBeUndefined()
  })
})

describe('closedPageTarget', () => {
  it('leaves an open page alone', () => {
    expect(closedPageTarget('/checkin', new Set(['/sounds']))).toBeUndefined()
  })

  it("sends a turned-off page to its group's first open page, or home when the group has none", () => {
    expect(closedPageTarget('/card-style', new Set(['/checkin', '/card-style']))).toBe('/alerts')
    expect(closedPageTarget('/alerts', new Set(ALERT_PAGES))).toBe('/checkin')
    const media = GROUPS.find((g) => g.id === 'media')!.pages.map((p) => p.path)
    expect(closedPageTarget('/sounds', new Set(media))).toBe('/')
  })
})

describe('router', () => {
  const at = async (path: string) => {
    await router.push('/')
    await router.push(path)
    return router.currentRoute.value.path
  }

  it('sends a turned-off page elsewhere, whatever its letter case or trailing slash, and opens it once on again', async () => {
    shareModules([{ id: 'checkin', enabled: false, closed: false }])
    for (const path of ['/checkin', '/CHECKIN', '/card-style/']) expect(await at(path), path).toBe('/alerts')
    shareModules([{ id: 'checkin', enabled: true, closed: false }])
    expect(await at('/card-style')).toBe('/card-style')
  })
})
