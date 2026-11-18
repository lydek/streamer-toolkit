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
      soon('/rehearsal', 'rehearsal', 'pi pi-play-circle', true),
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
      soon('/chat-log', 'chatLog', 'pi pi-comments'),
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
      soon('/card-style', 'cardStyle', 'pi pi-palette'),
      soon('/rewards', 'rewards', 'pi pi-star', true),
      { path: '/redemptions', label: 'modules.names.redemption' },
      soon('/counters', 'counters', 'pi pi-hashtag', true),
    ],
  },
  {
    id: 'media',
    icon: 'pi pi-images',
    pages: [
      soon('/scenes', 'scenes', 'pi pi-images', true),
      soon('/music', 'music', 'pi pi-headphones', true),
      { path: '/sounds', label: 'modules.names.sound' },
      soon('/tts', 'tts', 'pi pi-comment', true),
      soon('/screen-tools', 'screenTools', 'pi pi-desktop', true),
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

/** Every coming-soon page, for the router and the home tiles. */
export const SOON_PAGES = GROUPS.flatMap((g) => g.pages).filter((p) => p.soon)

/** The group a route path belongs to, or undefined (settings, not found). */
export function groupOf(path: string): TabGroup | undefined {
  return GROUPS.find((g) => g.pages.some((p) => p.path === path))
}

/**
 * Where a group tab goes before any of its pages was visited: its first built page, else its first page; pages in
 * `hidden` (features turned off) are skipped.
 */
export function firstPage(g: TabGroup, hidden: Set<string> = new Set()): string {
  const pages = g.pages.filter((p) => !hidden.has(p.path))
  return (pages.find((p) => !p.soon) ?? pages[0] ?? g.pages[0]).path
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
