/*
 * The app bar's group tabs (docs/decisions.md 管理 UI 版面), three levels: groups in the bar; a group's categories in
 * the second row (a group without categories lists its pages there instead); a category's pages as a row under the
 * page title, shown only when it has more than one. Settings is not a tab (it sits behind the gear button), and
 * neither is any route not listed here.
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
  /** i18n key of a shorter name in the page row, where the full name would repeat the category's. */
  tabLabel?: string
}

export interface TabCategory {
  /** `nav.categories.<id>` holds its name. */
  id: string
  pages: TabPage[]
}

export interface TabGroup {
  id: string
  icon: string
  /** Every page of the group in order (its categories' pages, when it has categories). */
  pages: TabPage[]
  /** The second row; absent: the second row lists the pages. */
  categories?: TabCategory[]
}

const soon = (path: string, id: string, icon: string, tile = false): TabPage => ({
  path,
  label: `pages.${id}.name`,
  soon: { id, icon, tile },
})
const grouped = (id: string, icon: string, categories: TabCategory[]): TabGroup => ({
  id,
  icon,
  categories,
  pages: categories.flatMap((c) => c.pages),
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
  grouped('chat', 'pi pi-comments', [
    {
      id: 'chatroom',
      pages: [
        { path: '/chat-log', label: 'pages.chatLog.name' },
        { path: '/chat', label: 'pages.chatTemplates.name' },
        soon('/chat-window', 'chatWindow', 'pi pi-window-maximize'),
        soon('/chat-filter', 'chatFilter', 'pi pi-filter', true),
      ],
    },
    {
      id: 'bot',
      pages: [
        soon('/commands', 'commands', 'pi pi-reply', true),
        soon('/timers', 'timers', 'pi pi-clock'),
        soon('/greetings', 'greetings', 'pi pi-heart'),
      ],
    },
  ]),
  grouped('interact', 'pi pi-bolt', [
    {
      id: 'alerts',
      pages: [
        { path: '/alerts', label: 'modules.names.alerts', tabLabel: 'nav.pageTabs.alertTemplates' },
        { path: '/alert-events', label: 'pages.alertEvents.name' },
        { path: '/alert-log', label: 'pages.alertLog.name' },
      ],
    },
    {
      id: 'checkin',
      pages: [
        { path: '/checkin', label: 'modules.names.checkin', tabLabel: 'nav.pageTabs.checkin' },
        { path: '/card-style', label: 'pages.cardStyle.name' },
        { path: '/checkin-rules', label: 'pages.checkinRules.name' },
      ],
    },
    {
      id: 'points',
      pages: [
        { ...soon('/rewards', 'rewards', 'pi pi-star', true), tabLabel: 'nav.pageTabs.rewards' },
        { path: '/redemptions', label: 'modules.names.redemption' },
        { path: '/reward-playback', label: 'pages.rewardPlayback.name' },
      ],
    },
    {
      id: 'counters',
      pages: [
        { path: '/counters', label: 'modules.names.counter' },
        { path: '/marathon', label: 'pages.marathon.name' },
      ],
    },
    { id: 'polls', pages: [soon('/polls', 'polls', 'pi pi-chart-bar', true)] },
  ]),
  grouped('media', 'pi pi-images', [
    {
      id: 'obs',
      pages: [
        { path: '/scenes', label: 'modules.names.scene' },
        soon('/modes', 'modes', 'pi pi-sliders-h'),
        { path: '/obs-sources', label: 'pages.obsSources.name' },
      ],
    },
    {
      id: 'music',
      pages: [
        { path: '/music', label: 'modules.names.bgm', tabLabel: 'nav.pageTabs.musicPlayer' },
        { path: '/music-playlists', label: 'pages.musicPlaylists.name' },
        { path: '/music-library', label: 'pages.musicLibrary.name' },
        soon('/music-requests', 'musicRequests', 'pi pi-youtube'),
      ],
    },
    { id: 'sound', pages: [{ path: '/sounds', label: 'modules.names.sound' }] },
    {
      id: 'widgets',
      pages: [
        { path: '/screen-tools', label: 'modules.names.screen-tools', tabLabel: 'nav.pageTabs.breakCard' },
        { path: '/warnings', label: 'pages.warnings.name' },
        { path: '/overlay-look', label: 'pages.overlayLook.name' },
        soon('/goals', 'goals', 'pi pi-flag'),
        soon('/captions', 'captions', 'pi pi-align-left'),
      ],
    },
    {
      id: 'pictures',
      pages: [
        { path: '/media', label: 'pages.media.name' },
        { path: '/image-sizes', label: 'pages.imageSizes.name' },
      ],
    },
    { id: 'avatar', pages: [soon('/avatar', 'avatar', 'pi pi-face-smile', true)] },
  ]),
  grouped('viewers', 'pi pi-users', [
    {
      id: 'viewers',
      pages: [soon('/viewers', 'viewers', 'pi pi-users', true), soon('/roles', 'roles', 'pi pi-tags')],
    },
    { id: 'leaderboard', pages: [{ path: '/leaderboard', label: 'pages.leaderboard.name' }] },
    {
      id: 'raids',
      pages: [
        soon('/shoutouts', 'shoutouts', 'pi pi-video', true),
        soon('/raids', 'raids', 'pi pi-send', true),
        soon('/credits', 'credits', 'pi pi-list', true),
      ],
    },
  ]),
]

/** Every page not built yet: grey in the tabs, its URL leads home, and `tile` ones are grey home tiles. */
export const SOON_PAGES = GROUPS.flatMap((g) => g.pages).filter((p) => p.soon)

/** The group a route path belongs to, or undefined (settings, not found). */
export function groupOf(path: string): TabGroup | undefined {
  return GROUPS.find((g) => g.pages.some((p) => p.path === path))
}

/** The category a route path belongs to, or undefined (a group without categories, settings, not found). */
export function categoryOf(path: string): TabCategory | undefined {
  return groupOf(path)?.categories?.find((c) => c.pages.some((p) => p.path === path))
}

/**
 * Where a category goes: the page last seen in it while that page can open, else its first built page that is not
 * turned off; undefined when it has none (the category is grey).
 */
export function categoryTarget(c: TabCategory, hidden: Set<string> = new Set(), last?: string): string | undefined {
  const open = c.pages.filter((p) => !p.soon && !hidden.has(p.path))
  return open.find((p) => p.path === last)?.path ?? open[0]?.path
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

/** Where a turned-off page leads instead: the first open page of its group, or home when none is left. */
export function closedPageTarget(path: string, hidden: Set<string>): string | undefined {
  if (!hidden.has(path)) return undefined
  const g = groupOf(path)
  return g && groupUsable(g, hidden) ? firstPage(g, hidden) : '/'
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
