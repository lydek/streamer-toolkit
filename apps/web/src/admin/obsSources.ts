import { shallowRef } from 'vue'
import { type Schemas, api, client } from './api'

/*
 * The built-in overlays for the OBS sources page (docs/architecture.md OBS 連線): what each shows, its group (as the
 * tabs), its address and the browser source size it suits. One without `path` is not built yet and lists grey.
 * The Host tells which browser sources show an overlay; this side groups them per overlay.
 */

export interface OverlayEntry {
  id: string
  group: string
  /** Its address on the Host; absent while it is not built. */
  path?: string
  /** The module that has to be on for it; absent when it is always there. */
  module?: string
  /** Width × height for its browser source; `audio` for one that only plays sound. */
  size: string
}

export const OVERLAYS: OverlayEntry[] = [
  { id: 'songs', group: 'song', size: '500 × 300' },
  { id: 'chat', group: 'chat', path: '/overlay/chat/', module: 'chat', size: '400 × 600' },
  { id: 'alerts', group: 'interact', path: '/overlay/alerts/', module: 'alerts', size: '1920 × 1080' },
  { id: 'checkin', group: 'interact', path: '/overlay/checkin/', module: 'checkin', size: '800 × 400' },
  { id: 'counter', group: 'interact', path: '/overlay/counter/', module: 'counter', size: '600 × 200' },
  { id: 'marathon', group: 'interact', path: '/overlay/marathon/', module: 'counter', size: '600 × 160' },
  { id: 'polls', group: 'interact', size: '600 × 300' },
  { id: 'sound', group: 'media', path: '/overlay/sound/', module: 'sound', size: 'audio' },
  { id: 'bgm', group: 'media', path: '/overlay/bgm/', module: 'bgm', size: '600 × 120' },
  { id: 'break', group: 'media', path: '/overlay/break/', module: 'screen-tools', size: '1920 × 1080' },
  { id: 'warnings', group: 'media', path: '/overlay/warnings/', module: 'screen-tools', size: '1920 × 1080' },
  { id: 'goals', group: 'media', size: '800 × 100' },
  { id: 'captions', group: 'media', size: '1600 × 200' },
  { id: 'avatar', group: 'media', size: '400 × 400' },
  { id: 'requests', group: 'media', size: '1280 × 720' },
  { id: 'leaderboard', group: 'viewers', path: '/overlay/leaderboard/', size: '400 × 500' },
  { id: 'shoutouts', group: 'viewers', size: '1280 × 720' },
  { id: 'credits', group: 'viewers', size: '1920 × 1080' },
]

export interface OverlayRow {
  /** Unique per row: the overlay's ID, or the leaderboard's address for one board and period. */
  key: string
  entry: OverlayEntry
  /** The address to copy; absent for a grey row and for 「排行榜：其他」. */
  url?: string
  /** Board and period of a leaderboard row. */
  board?: { board: string; period: string }
  /** The browser sources showing it. */
  sources: Schemas['OverlaySource'][]
  /** Every scene any of those sources is in. */
  scenes: string[]
}

const pathOf = (overlay: string) => overlay.split('?')[0]

type T = (key: string, values?: Record<string, unknown>) => string

/** A row's name: the overlay's, or 「排行榜：小奇點・本場」 for one board and period. */
export function overlayName(r: OverlayRow, t: T): string {
  if (r.board) {
    const board = t(`overlay.leaderboard.${r.board.board}`)
    return t('obsSources.leaderboardRow', { board, period: t(`leaderboard.periods.${r.board.period}`) })
  }
  return t(r.key === 'leaderboard-other' ? 'obsSources.names.leaderboardOther' : `obsSources.names.${r.entry.id}`)
}

/** A browser source's width and height for an overlay's suggested size; a sound-only one gets a small box. */
export function sourceSize(size: string): [number, number] {
  const [width, height] = size === 'audio' ? [100, 100] : size.split(' × ').map(Number)
  return [width, height]
}

/** Puts a row's overlay in `scene` (POST /api/admin/obs/sources), naming a new source after it. */
export function addOverlay(scene: string, r: OverlayRow, t: T) {
  const [width, height] = sourceSize(r.entry.size)
  const name = t('obsSources.sourceName', { name: overlayName(r, t), app: t('app.name') })
  return api(client.POST('/api/admin/obs/sources', { body: { scene, overlay: r.url!, name, width, height } }))
}

export interface ObsProblem {
  id: string
  text: string
  /** The page that fixes it. */
  to: string
}

/**
 * OBS problems for home's to-do list: an overlay with two or more sources, and background music missing from the
 * scene on air while a playlist rule names that scene. Each goes away once its cause does.
 */
export function obsProblems(
  data: Schemas['ObsSources'] | undefined,
  rules: Schemas['SceneRuleBody'][],
  off: Set<string>,
  t: T,
): ObsProblem[] {
  if (!data?.connected) return []
  const rows = overlayRows(data.sources, off)
  const problems: ObsProblem[] = rows
    .filter((r) => r.sources.length > 1)
    .map((r) => ({
      id: `duplicate:${r.key}`,
      text: t('obsSources.problems.duplicate', { name: overlayName(r, t), n: r.sources.length }),
      to: '/obs-sources',
    }))
  const live = data.live_scene?.toLowerCase()
  const bgm = rows.find((r) => r.key === 'bgm')
  const ruled = rules.some((r) => r.enabled && r.playlist && r.scenes.some((s) => s.toLowerCase() === live))
  if (live && bgm && ruled && !bgm.scenes.some((s) => s.toLowerCase() === live)) {
    problems.push({ id: 'bgm:scene', text: t('obsSources.problems.bgmNotInScene', { scene: data.live_scene }), to: '/obs-sources' })
  }
  return problems
}

function row(key: string, entry: OverlayEntry, sources: Schemas['OverlaySource'][], url?: string): OverlayRow {
  return { key, entry, url, sources, scenes: [...new Set(sources.flatMap((s) => s.scenes))] }
}

/**
 * One row per overlay, in the tabs' group order; a leaderboard gets one row per board and period OBS shows, then
 * 「排行榜：其他」. Overlays of modules turned off are left out.
 */
export function overlayRows(sources: Schemas['OverlaySource'][], off: Set<string> = new Set()): OverlayRow[] {
  return OVERLAYS.filter((o) => !o.module || !off.has(o.module)).flatMap((o) => {
    if (!o.path) return [row(o.id, o, [])]
    const mine = sources.filter((s) => pathOf(s.overlay) === o.path)
    if (o.id !== 'leaderboard') return [row(o.id, o, mine, o.path)]
    const boards = [...new Set(mine.map((s) => s.overlay))].flatMap((overlay) => {
      const query = new URLSearchParams(overlay.split('?')[1] ?? '')
      const [board, period] = [query.get('board'), query.get('period')]
      if (!board || !period) return []
      const r = row(overlay, o, mine.filter((s) => s.overlay === overlay), overlay)
      return [{ ...r, board: { board, period } }]
    })
    return [...boards, row('leaderboard-other', o, [])]
  })
}

/** What OBS has of one overlay, for the status link on its pages. */
export type ObsPresence =
  | { kind: 'offline' }
  | { kind: 'missing' }
  | { kind: 'duplicate'; sources: number }
  | { kind: 'in'; scenes: number; live: boolean }

/** Where `overlay` (a path, with the query a leaderboard needs) is in OBS, from `GET /api/admin/obs/sources`. */
export function obsPresence(overlay: string, data: Schemas['ObsSources'] | undefined): ObsPresence {
  if (!data?.connected) return { kind: 'offline' }
  const exact = overlay.includes('?')
  const mine = data.sources.filter((s) => (exact ? s.overlay === overlay : pathOf(s.overlay) === overlay))
  if (mine.length > 1) return { kind: 'duplicate', sources: mine.length }
  const scenes = mine[0]?.scenes ?? []
  if (!scenes.length) return { kind: 'missing' }
  return { kind: 'in', scenes: scenes.length, live: !!data.live_scene && scenes.includes(data.live_scene) }
}

// One shared copy for every status link: read when a page with one opens, at most once per couple of seconds.
const shared = shallowRef<Schemas['ObsSources']>()
let readAt = 0
let reading: Promise<void> | undefined
// Asked to read right now while a read was under way: that read may have started before the change, so one more.
let again = false

/** The overlays' places in OBS, read again when a page that shows them opens (`force`: right now). */
export function useObsSources(force = false) {
  if (reading) again ||= force
  else if (force || Date.now() - readAt > 2000) {
    reading = api(client.GET('/api/admin/obs/sources'))
      .then((data) => {
        shared.value = data
        readAt = Date.now()
      })
      .catch(() => {})
      .finally(() => {
        reading = undefined
        if (again) {
          again = false
          useObsSources(true)
        }
      })
  }
  return shared
}
