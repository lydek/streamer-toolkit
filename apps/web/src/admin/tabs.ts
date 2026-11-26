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
  /** i18n key of the page name, also its full page's title. */
  label: string
  /** A built page's icon, beside its full page's title. */
  icon?: string
  /** The icon's color on a page without a home tile (a page with one takes the tile's). */
  tint?: string
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
      { path: '/stream-info', label: 'pages.streamInfo.name', icon: 'pi pi-pencil' },
      { path: '/rehearsal', label: 'pages.rehearsal.name', icon: 'pi pi-play-circle' },
    ],
  },
  {
    id: 'song',
    icon: 'pi pi-microphone',
    pages: [
      soon('/songs', 'songs', 'pi pi-microphone', true),
      soon('/song-list', 'songList', 'pi pi-book'),
      soon('/song-rules', 'songRules', 'pi pi-sliders-h'),
      soon('/singing-booth', 'singingBooth', 'pi pi-headphones', true),
    ],
  },
  grouped('chat', 'pi pi-comments', [
    {
      id: 'chatroom',
      pages: [
        { path: '/chat-log', label: 'pages.chatLog.name', icon: 'pi pi-list' },
        { path: '/chat', label: 'pages.chatTemplates.name', icon: 'pi pi-palette' },
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
        { path: '/alerts', label: 'modules.names.alerts', icon: 'pi pi-bell', tabLabel: 'nav.pageTabs.alertTemplates' },
        { path: '/alert-events', label: 'pages.alertEvents.name', icon: 'pi pi-list' },
        { path: '/alert-log', label: 'pages.alertLog.name', icon: 'pi pi-history' },
      ],
    },
    {
      id: 'checkin',
      pages: [
        { path: '/checkin', label: 'modules.names.checkin', icon: 'pi pi-check-square', tabLabel: 'nav.pageTabs.checkin' },
        { path: '/card-style', label: 'pages.cardStyle.name', icon: 'pi pi-palette' },
        { path: '/checkin-rules', label: 'pages.checkinRules.name', icon: 'pi pi-sliders-h' },
      ],
    },
    {
      id: 'points',
      pages: [
        { path: '/rewards', label: 'pages.rewards.name', icon: 'pi pi-star', tabLabel: 'nav.pageTabs.rewards' },
        { path: '/reward-playback', label: 'pages.rewardPlayback.name', icon: 'pi pi-play', tint: 'yellow' },
        { path: '/redemptions', label: 'modules.names.redemption', icon: 'pi pi-gift' },
      ],
    },
    {
      id: 'counters',
      pages: [
        { path: '/counters', label: 'modules.names.counter', icon: 'pi pi-hashtag' },
        { path: '/marathon', label: 'pages.marathon.name', icon: 'pi pi-stopwatch' },
      ],
    },
    { id: 'polls', pages: [soon('/polls', 'polls', 'pi pi-chart-bar', true)] },
  ]),
  grouped('media', 'pi pi-images', [
    {
      id: 'obs',
      pages: [
        { path: '/scenes', label: 'modules.names.scene', icon: 'pi pi-images' },
        soon('/modes', 'modes', 'pi pi-sliders-h'),
        { path: '/mixer', label: 'pages.mixer.name', icon: 'pi pi-sliders-v' },
        { path: '/obs-sources', label: 'pages.obsSources.name', icon: 'pi pi-clone', tint: 'blue' },
      ],
    },
    {
      id: 'music',
      pages: [
        { path: '/music', label: 'modules.names.bgm', icon: 'pi pi-headphones', tabLabel: 'nav.pageTabs.musicPlayer' },
        { path: '/music-playlists', label: 'pages.musicPlaylists.name', icon: 'pi pi-list' },
        { path: '/music-library', label: 'pages.musicLibrary.name', icon: 'pi pi-folder' },
        soon('/music-requests', 'musicRequests', 'pi pi-youtube'),
      ],
    },
    { id: 'sound', pages: [{ path: '/sounds', label: 'modules.names.sound', icon: 'pi pi-volume-up' }] },
    {
      id: 'widgets',
      pages: [
        { path: '/screen-tools', label: 'modules.names.screen-tools', icon: 'pi pi-moon', tabLabel: 'nav.pageTabs.breakCard' },
        { path: '/warnings', label: 'pages.warnings.name', icon: 'pi pi-exclamation-triangle' },
        soon('/goals', 'goals', 'pi pi-flag'),
        soon('/captions', 'captions', 'pi pi-align-left'),
        { path: '/overlay-look', label: 'pages.overlayLook.name', icon: 'pi pi-palette', tint: 'primary' },
      ],
    },
    {
      id: 'pictures',
      pages: [
        { path: '/media', label: 'pages.media.name', icon: 'pi pi-images' },
        { path: '/image-sizes', label: 'pages.imageSizes.name', icon: 'pi pi-image' },
      ],
    },
    {
      id: 'avatar',
      pages: [
        { ...soon('/avatar', 'avatar', 'pi pi-face-smile', true), tabLabel: 'nav.pageTabs.avatar3d' },
        soon('/mic-avatar', 'micAvatar', 'pi pi-microphone'),
        soon('/outfits', 'outfits', 'pi pi-user'),
      ],
    },
  ]),
  grouped('viewers', 'pi pi-users', [
    {
      id: 'viewers',
      pages: [soon('/viewers', 'viewers', 'pi pi-users', true), soon('/roles', 'roles', 'pi pi-tags')],
    },
    { id: 'leaderboard', pages: [{ path: '/leaderboard', label: 'pages.leaderboard.name', icon: 'pi pi-trophy', tint: 'yellow' }] },
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

/**
 * Dev mode (the Vite dev server: `pnpm tauri dev`, `pnpm web:dev`) opens the pages not built yet, to draw them ahead
 * of their backend. Release builds, `cargo run`, unit tests (mode `test`) and E2E (mode `e2e`) keep them grey.
 */
export const showUnbuilt = () => import.meta.env.DEV && import.meta.env.MODE === 'development'

/** Grey and closed: a page not built yet, outside dev mode. */
export const isGrey = (p: TabPage) => !!p.soon && !showUnbuilt()

/** The page tab at a route path, or undefined (settings, not found). */
export const tabPage = (path: string): TabPage | undefined => groupOf(path)?.pages.find((p) => p.path === path)

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
  const open = c.pages.filter((p) => !isGrey(p) && !hidden.has(p.path))
  return open.find((p) => p.path === last)?.path ?? open[0]?.path
}

/** A group can be opened while it has a page that is built and not turned off; otherwise its tab is grey. */
export const groupUsable = (g: TabGroup, hidden: Set<string> = new Set()) =>
  g.pages.some((p) => !isGrey(p) && !hidden.has(p.path))

/**
 * Where a group tab goes before any of its pages was visited: its first built page, else its first page; pages in
 * `hidden` (features turned off) are skipped.
 */
export function firstPage(g: TabGroup, hidden: Set<string> = new Set()): string {
  const pages = g.pages.filter((p) => !hidden.has(p.path))
  return (pages.find((p) => !isGrey(p)) ?? pages[0] ?? g.pages[0]).path
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
