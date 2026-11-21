/*
 * The app bar's group tabs (docs/decisions.md 管理 UI 版面): top-level groups, each with one or more pages. A group
 * with several pages shows them as a second row of sub-tabs; never more than two levels. Settings is not a tab
 * (it sits behind the gear button), and neither is any route not listed here.
 *
 * Every screen of the design is listed (docs/scope.md 階段 0); a page marked `soon` is not built yet and shows the
 * coming-soon page (`pages.<id>` holds its name and one-line description). `tile` marks the page a coming-soon
 * feature's home tile opens.
 */

export interface TabPage {
  path: string
  /** i18n key of the page name. */
  label: string
  /** Not built yet: the route shows the coming-soon page. */
  soon?: { id: string; icon: string; tile?: boolean }
}

export interface TabGroup {
  id: string
  icon: string
  pages: TabPage[]
}

const soon = (path: string, id: string, icon: string, tile = false): TabPage => ({
  path,
  label: `pages.${id}.name`,
  soon: { id, icon, tile },
})

export const GROUPS: TabGroup[] = [
  {
    id: 'home',
    icon: 'pi pi-home',
    pages: [
      { path: '/', label: 'nav.groups.home' },
      soon('/stream-info', 'streamInfo', 'pi pi-pencil', true),
      { path: '/rehearsal', label: 'pages.rehearsal.name' },
    ],
  },
  {
    id: 'song',
    icon: 'pi pi-microphone',
    pages: [
      soon('/songs', 'songs', 'pi pi-microphone', true),
      soon('/song-list', 'songList', 'pi pi-book'),
      soon('/singing-booth', 'singingBooth', 'pi pi-headphones', true),
    ],
  },
  {
    id: 'chat',
    icon: 'pi pi-comments',
    pages: [
      { path: '/chat-log', label: 'pages.chatLog.name' },
      soon('/chat-filter', 'chatFilter', 'pi pi-filter', true),
      { path: '/chat', label: 'pages.chatTemplates.name' },
      soon('/chat-window', 'chatWindow', 'pi pi-window-maximize'),
      soon('/commands', 'commands', 'pi pi-reply', true),
      soon('/timers', 'timers', 'pi pi-clock'),
      soon('/greetings', 'greetings', 'pi pi-heart'),
    ],
  },
  {
    id: 'interact',
    icon: 'pi pi-bolt',
    pages: [
      { path: '/alerts', label: 'modules.names.alerts' },
      soon('/polls', 'polls', 'pi pi-chart-bar', true),
      { path: '/checkin', label: 'modules.names.checkin' },
      { path: '/card-style', label: 'pages.cardStyle.name' },
      soon('/rewards', 'rewards', 'pi pi-star', true),
      { path: '/redemptions', label: 'modules.names.redemption' },
      { path: '/counters', label: 'modules.names.counter' },
    ],
  },
  {
    id: 'media',
    icon: 'pi pi-images',
    pages: [
      { path: '/scenes', label: 'modules.names.scene' },
      { path: '/music', label: 'modules.names.bgm' },
      { path: '/sounds', label: 'modules.names.sound' },
      soon('/tts', 'tts', 'pi pi-comment', true),
      { path: '/screen-tools', label: 'modules.names.screen-tools' },
      soon('/avatar', 'avatar', 'pi pi-face-smile', true),
      soon('/shoutouts', 'shoutouts', 'pi pi-video', true),
      soon('/image-sizes', 'imageSizes', 'pi pi-image', true),
    ],
  },
  {
    id: 'viewers',
    icon: 'pi pi-users',
    pages: [
      soon('/viewers', 'viewers', 'pi pi-users', true),
      soon('/roles', 'roles', 'pi pi-tags'),
      soon('/leaderboard', 'leaderboard', 'pi pi-trophy'),
      soon('/credits', 'credits', 'pi pi-list', true),
      soon('/raids', 'raids', 'pi pi-send', true),
    ],
  },
]

/** Every page not built yet: grey in the tabs, its URL leads home, and `tile` ones are grey home tiles. */
export const SOON_PAGES = GROUPS.flatMap((g) => g.pages).filter((p) => p.soon)

/** The group a route path belongs to, or undefined (settings, not found). */
export function groupOf(path: string): TabGroup | undefined {
  return GROUPS.find((g) => g.pages.some((p) => p.path === path))
}

/** A group can be opened while it has a page that is built and not turned off; otherwise its tab is grey. */
export const groupUsable = (g: TabGroup, hidden: Set<string> = new Set()) =>
  g.pages.some((p) => !p.soon && !hidden.has(p.path))

/**
 * Where a group tab goes before any of its pages was visited: its first built page, else its first page; pages in
 * `hidden` (features turned off) are skipped.
 */
export function firstPage(g: TabGroup, hidden: Set<string> = new Set()): string {
  const pages = g.pages.filter((p) => !hidden.has(p.path))
  return (pages.find((p) => !p.soon) ?? pages[0] ?? g.pages[0]).path
}

/**
 * How many tabs fit in `available` pixels, given each tab's width and the gap between items. The last slot holds
 * 「編輯分頁」 (`editWidth`) when every tab fits, else "More" (`moreWidth`) in place of the ones that do not.
 */
export function fittingTabs(widths: number[], gap: number, available: number, moreWidth: number, editWidth: number): number {
  const total = (n: number) => widths.slice(0, n).reduce((sum, w) => sum + w, 0) + gap * Math.max(n - 1, 0)
  if (total(widths.length) + gap + editWidth <= available) return widths.length
  let n = widths.length - 1
  while (n > 0 && total(n) + gap + moreWidth > available) n--
  return n
}
