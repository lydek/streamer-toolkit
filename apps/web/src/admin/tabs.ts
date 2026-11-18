/*
 * The app bar's group tabs (docs/decisions.md 管理 UI 版面): top-level groups, each with one or more pages. A group
 * with several pages shows them as a second row of sub-tabs; never more than two levels. Settings is not a tab
 * (it sits behind the gear button), and neither is any route not listed here.
 */

export interface TabPage {
  path: string
  /** i18n key of the page name. */
  label: string
}

export interface TabGroup {
  id: string
  icon: string
  pages: TabPage[]
}

export const GROUPS: TabGroup[] = [
  { id: 'home', icon: 'pi pi-home', pages: [{ path: '/', label: 'nav.groups.home' }] },
  { id: 'chat', icon: 'pi pi-comments', pages: [{ path: '/chat', label: 'modules.names.chat' }] },
  {
    id: 'interact',
    icon: 'pi pi-bolt',
    pages: [
      { path: '/alerts', label: 'modules.names.alerts' },
      { path: '/checkin', label: 'modules.names.checkin' },
      { path: '/redemptions', label: 'modules.names.redemption' },
    ],
  },
  { id: 'media', icon: 'pi pi-images', pages: [{ path: '/sounds', label: 'modules.names.sound' }] },
]

/** The group a route path belongs to, or undefined (settings, not found). */
export function groupOf(path: string): TabGroup | undefined {
  return GROUPS.find((g) => g.pages.some((p) => p.path === path))
}

/**
 * How many tabs fit in `available` pixels, given each tab's width, the gap between tabs and the width of the
 * "More" button that takes the place of the ones that do not fit. All of them, when they fit without "More".
 */
export function fittingTabs(widths: number[], gap: number, available: number, moreWidth: number): number {
  const total = (n: number) => widths.slice(0, n).reduce((sum, w) => sum + w, 0) + gap * Math.max(n - 1, 0)
  if (total(widths.length) <= available) return widths.length
  let n = widths.length - 1
  while (n > 0 && total(n) + gap + moreWidth > available) n--
  return n
}
