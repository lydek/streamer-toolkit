import { onScopeDispose, reactive, shallowRef, watch } from 'vue'
import { removes, type Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'
import { SOON_PAGES } from './tabs'

/** The Twitch status the app bar polls every 2 s (undefined until its first answer); read it instead of polling again. */
export const twitchOverview = shallowRef<Schemas['Overview']>()

/**
 * Whether Twitch or OBS is down right now, set by the app bar from its lights. Unknown (still loading) is not down,
 * and losing the Host's own connection is not a service going down.
 */
export const servicesDown = reactive({ twitch: false, obs: false })
export const isDown = (l: Light) => (l.state === 'error' || l.state === 'reconnecting') && l.key !== 'home.status.failed'

/** A status light in the status bar. `key` is the i18n key of its state text. */
export interface Light {
  state: 'connected' | 'reconnecting' | 'error' | 'loading'
  key: string
}

/** One tile status line: never blank, never a stale value while loading or failed. */
/** What a tile shows; `num` is its big value (a count or a short word) with `unit` after it, `text` the line under it. */
export type Line = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; text: string; num?: string; unit?: string }

/** The Twitch light, from `GET /api/admin/twitch` (`failed`: the last request failed). */
export function twitchLight(overview: Schemas['Overview'] | undefined, failed: boolean): Light {
  if (failed) return { state: 'error', key: 'home.status.failed' }
  if (!overview) return { state: 'loading', key: 'app.loading' }
  if (!overview.client_id) return { state: 'error', key: 'home.status.twitch.setup' }
  switch (overview.status.state) {
    case 'signed_in':
      return { state: 'connected', key: 'home.status.connected' }
    case 'pending':
      return { state: 'reconnecting', key: 'home.status.twitch.pending' }
    case 'failed':
      return overview.status.code === 'twitch.scopes_missing'
        ? { state: 'error', key: 'home.status.twitch.relogin' }
        : { state: 'error', key: 'home.status.twitch.failed' }
    case 'signed_out':
      return { state: 'error', key: 'home.status.twitch.signedOut' }
  }
}

/** The OBS light, from the `obs.status` event kept in the live snapshot. */
export function obsLight(live: LiveState): Light {
  if (live.status === 'failed') return { state: 'error', key: 'home.status.failed' }
  if (!live.obs) return { state: 'loading', key: 'app.loading' }
  return {
    connected: { state: 'connected', key: 'home.status.connected' },
    reconnecting: { state: 'reconnecting', key: 'home.status.obs.reconnecting' },
    error: { state: 'error', key: 'home.status.obs.error' },
  }[live.obs.state] as Light
}

/** `H:MM:SS` for a length in milliseconds (negative counts as 0). */
export function clockText(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  return `${Math.floor(total / 3600)}:${String(Math.floor(total / 60) % 60).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`
}

export type Stage = 'before' | 'live' | 'after'

/**
 * Which stage home shows by itself: live while Twitch says so; after a stream that ended while the Host was running
 * (the summary stays reachable until the next stream starts); before otherwise, signed out included.
 */
export function streamStage(stream: Schemas['StreamStatusPayload'] | undefined, summary: Schemas['StreamSummary'] | null | undefined): Stage {
  if (stream?.state === 'live') return 'live'
  if (stream?.state === 'offline' && summary?.endedAt && summary.endedSinceLaunch) return 'after'
  return 'before'
}

const twoDigits = (n: number) => String(n).padStart(2, '0')
const dayTime = (d: Date) => `${d.getMonth() + 1}/${d.getDate()} ${twoDigits(d.getHours())}:${twoDigits(d.getMinutes())}`

/** `11/18 19:58–22:39 · 2:41` in local time; the end's day shows when it differs. */
export function sessionRange(startedAt: string, endedAt: string): string {
  const start = new Date(startedAt)
  const end = new Date(endedAt)
  const sameDay = start.toDateString() === end.toDateString()
  const endText = sameDay ? `${twoDigits(end.getHours())}:${twoDigits(end.getMinutes())}` : dayTime(end)
  const minutes = Math.max(0, Math.floor((end.getTime() - start.getTime()) / 60000))
  return `${dayTime(start)}–${endText} · ${Math.floor(minutes / 60)}:${twoDigits(minutes % 60)}`
}

/** One pre-stream check: whether it is fine, and where to fix it (a page or a settings section). */
export interface Check {
  id: 'twitch' | 'eventsub' | 'obs' | `overlay.${OverlayKind}`
  ok: boolean
  fix: string
}

type OverlayKind = 'chat' | 'alerts' | 'checkin' | 'sound' | 'bgm'

/**
 * Each overlay kind, the module that needs it in OBS, and the page with its OBS address. The counter and marathon
 * overlays are left out: they only show numbers on screen, which is optional; counting works without them.
 */
const OVERLAYS: { kind: OverlayKind; module: string; page: string }[] = [
  { kind: 'chat', module: 'chat', page: '/chat' },
  { kind: 'alerts', module: 'alerts', page: '/alerts' },
  { kind: 'checkin', module: 'checkin', page: '/checkin' },
  { kind: 'sound', module: 'sound', page: '/sounds' },
  { kind: 'bgm', module: 'bgm', page: '/music' },
]

/**
 * The pre-stream checklist: Twitch signed in, EventSub connected, OBS connected, and an OBS source open for each
 * enabled feature's overlay. Later features add their own checks here.
 */
export function preStreamChecks(
  overview: Schemas['Overview'] | undefined,
  live: LiveState,
  modules: Schemas['ModuleState'][] | undefined,
  overlays: Schemas['OverlayCounts'] | undefined,
  unused: Schemas['ExternalService'][] = [],
): Check[] {
  const twitch = '/settings/connections#twitch'
  const enabled = new Set((modules ?? []).filter((m) => m.enabled).map((m) => m.id))
  // A service not used is not checked, nor are overlays in OBS while OBS is not used.
  const usesTwitch = !unused.includes('twitch')
  const usesObs = !unused.includes('obs')
  return [
    ...(usesTwitch
      ? [
          { id: 'twitch', ok: overview?.status.state === 'signed_in', fix: twitch },
          { id: 'eventsub', ok: overview?.eventsub_connected === true, fix: twitch },
        ] satisfies Check[]
      : []),
    ...(usesObs ? [{ id: 'obs', ok: live.obs?.state === 'connected', fix: '/settings/connections#obs' } satisfies Check] : []),
    ...OVERLAYS.filter((o) => usesObs && enabled.has(o.module)).map(
      (o): Check => ({ id: `overlay.${o.kind}`, ok: (overlays?.[o.kind] ?? 0) > 0, fix: o.page }),
    ),
  ]
}

/** What the home page keeps from `/ws/overlay`. `loading` until the first snapshot, `failed` while disconnected. */
export interface LiveState {
  status: 'loading' | 'open' | 'failed'
  lastSeq: number
  obs?: Schemas['ObsStatusPayload']
  /** Scenes, inputs and outputs; empty while OBS is not connected. */
  obsState?: Schemas['StreamingState']
  /** What the presentation queue plays and holds. */
  queue?: Schemas['QueueStatus']
  /** Whether the broadcaster is live on Twitch, since when and for how many viewers. */
  stream?: Schemas['StreamStatusPayload']
  /** The background music player, with the time its position was taken. */
  bgm?: Schemas['BgmPlayerEvent']
  /** Every counter's latest value. */
  counters?: Schemas['CounterValue'][]
  /** The sub marathon timer, with the time its remaining time was taken. */
  marathon?: Schemas['MarathonEvent']
  /** The break card. */
  breakCard?: Schemas['BreakPayload']
  /** The on-stream warnings' settings and state. */
  warnings?: Schemas['WarningsPayload']
  /** The built-in overlays' theme and text size. */
  appearance?: Schemas['AppearancePayload']
  lastChat?: Schemas['ChatMessageEvent']
  /** Since this connection opened: alerts are not in snapshots. */
  lastAlert?: Schemas['AlertEvent']
}

export function applyLive(state: LiveState, msg: Incoming): LiveState {
  if (msg.type === 'snapshot') {
    return {
      status: 'open',
      lastSeq: msg.seq,
      obs: msg.payload['obs.status'].at(-1)?.payload,
      obsState: msg.payload['obs.state'].at(-1)?.payload,
      queue: msg.payload['presentation.queue'].at(-1)?.payload,
      stream: msg.payload['stream.status'].at(-1)?.payload,
      bgm: msg.payload['bgm.player'].at(-1),
      counters: msg.payload['counter.values'].at(-1)?.payload.counters,
      marathon: msg.payload['counter.marathon'].at(-1),
      breakCard: msg.payload['screen.break'].at(-1)?.payload,
      warnings: msg.payload['screen.warnings'].at(-1)?.payload,
      appearance: msg.payload['screen.appearance'].at(-1)?.payload,
      lastChat: msg.payload['chat.message'].at(-1),
    }
  }
  // Events before the snapshot, or replays, are already covered by it.
  if (state.status !== 'open' || msg.seq <= state.lastSeq) return state
  const next = { ...state, lastSeq: msg.seq }
  if (msg.type === 'obs.status') next.obs = msg.payload
  if (msg.type === 'obs.state') next.obsState = msg.payload
  if (msg.type === 'presentation.queue') next.queue = msg.payload
  if (msg.type === 'stream.status') next.stream = msg.payload
  if (msg.type === 'bgm.player') next.bgm = msg
  if (msg.type === 'counter.values') next.counters = msg.payload.counters
  if (msg.type === 'counter.marathon') next.marathon = msg
  if (msg.type === 'screen.break') next.breakCard = msg.payload
  if (msg.type === 'screen.warnings') next.warnings = msg.payload
  if (msg.type === 'screen.appearance') next.appearance = msg.payload
  if (msg.type === 'chat.message') next.lastChat = msg
  if (msg.type === 'chat.removed' && next.lastChat && removes(msg.payload, next.lastChat)) next.lastChat = undefined
  if (msg.type === 'alert') next.lastAlert = msg
  return next
}

/**
 * Connects to `/ws/overlay` (read-only, no token) and reconnects until the calling scope ends.
 * `onMessage` also gets every message, snapshots after a reconnect included.
 */
export function useLiveEvents(retryMs = 2000, onMessage?: (msg: Incoming) => void) {
  const state = shallowRef<LiveState>({ status: 'loading', lastSeq: 0 })
  let ws: WebSocket | undefined
  let retry: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  function connect() {
    ws = new WebSocket(`ws://${location.host}/ws/overlay`)
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data) as Incoming
      state.value = applyLive(state.value, msg)
      onMessage?.(msg)
    }
    ws.onclose = () => {
      // Drop what was known: the reconnect snapshot brings the current values.
      state.value = { status: 'failed', lastSeq: 0 }
      if (!stopped) retry = setTimeout(connect, retryMs)
    }
  }
  connect()
  onScopeDispose(() => {
    stopped = true
    clearTimeout(retry)
    ws?.close()
  })
  return state
}

/** A count (12, 1,234, +3) or a time (1:23, 2.5); anything else is a word (a scene, a state) and reads smaller. */
export const COUNT = /^[\d,+:.]+$/

/** Follows `source` at most once per `ms`, always ending with its latest value (trailing update). */
export function useThrottled<T>(source: () => T, ms: number | (() => number) = 3000) {
  const shown = shallowRef(source())
  let last = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  const flush = () => {
    timer = undefined
    last = Date.now()
    shown.value = source()
  }
  const gap = () => (typeof ms === 'function' ? ms() : ms)
  watch(source, () => {
    // Waiting already: a gap that dropped to none (masked numbers) flushes now, not when the wait ends.
    if (timer && gap() > 0) return
    clearTimeout(timer)
    const wait = last + gap() - Date.now()
    if (wait <= 0) flush()
    else timer = setTimeout(flush, wait)
  })
  onScopeDispose(() => clearTimeout(timer))
  return shown
}

export interface Tile {
  id: string
  icon: string
  /** The icon's color (prototype tints); `muted` for a feature not built yet. */
  tint: 'primary' | 'green' | 'yellow' | 'red' | 'blue' | 'muted'
  /** Not a module's: shown whatever modules are on. */
  always?: boolean
  /** Wider than a tile in the 12-column grid: `m` a third of a row, `l` two thirds, `w` a whole row. */
  size?: 'm' | 'l' | 'w'
  /** A feature not built yet: a grey tile that does nothing; `label` is its page's name key. */
  unbuilt?: { label: string }
}

/** The small tiles that open in place into a block, and how wide the block is (prototype `NATURAL`). */
export const EXPANDS: Record<string, 'm' | 'l' | 'w'> = { stats: 'w', feed: 'l' }

/** Default home order: the stage blocks, then one tile per module (and the event feed); a disabled module shows no tile. */
const BUILT_TILES: Tile[] = [
  // The stage blocks, shown on home itself (no drawer); the Host puts them first in lists saved before they joined.
  { id: 'pending', icon: 'pi pi-exclamation-circle', tint: 'primary', always: true, size: 'w' },
  { id: 'checklist', icon: 'pi pi-verified', tint: 'green', always: true, size: 'w' },
  { id: 'stats', icon: 'pi pi-chart-bar', tint: 'blue', always: true },
  { id: 'summary', icon: 'pi pi-flag', tint: 'yellow', always: true, size: 'w' },
  { id: 'queue', icon: 'pi pi-list', tint: 'red', always: true },
  { id: 'chat', icon: 'pi pi-comments', tint: 'blue' },
  { id: 'alerts', icon: 'pi pi-bell', tint: 'yellow' },
  { id: 'feed', icon: 'pi pi-bolt', tint: 'yellow', always: true },
  { id: 'redemption', icon: 'pi pi-gift', tint: 'red' },
  { id: 'checkin', icon: 'pi pi-check-square', tint: 'green' },
  { id: 'sound', icon: 'pi pi-volume-up', tint: 'primary' },
  { id: 'scene', icon: 'pi pi-images', tint: 'blue' },
  { id: 'mixer', icon: 'pi pi-sliders-v', tint: 'blue' },
  { id: 'bgm', icon: 'pi pi-headphones', tint: 'yellow' },
  { id: 'counter', icon: 'pi pi-hashtag', tint: 'red' },
  { id: 'screen-tools', icon: 'pi pi-desktop', tint: 'yellow' },
  { id: 'rehearsal', icon: 'pi pi-play-circle', tint: 'blue', always: true },
  { id: 'image-sizes', icon: 'pi pi-image', tint: 'green', always: true },
  { id: 'stream-info', icon: 'pi pi-pencil', tint: 'red' },
]

/**
 * Features not built yet, from the pages tabs.ts marks as home tiles. The ID is the page path without its `/`
 * (kebab-case, which the Host's `home-tiles` accepts), so a feature keeps its tile ID once it is built.
 */
export const UNBUILT_TILES: Tile[] = SOON_PAGES.filter((p) => p.soon!.tile).map((p) => ({
  id: p.path.slice(1),
  icon: p.soon!.icon,
  tint: 'muted',
  unbuilt: { label: p.label },
}))

/** Every tile: the built ones in default order, then the unbuilt ones (so hidden grey tiles list last). */
export const TILES: Tile[] = [...BUILT_TILES, ...UNBUILT_TILES]

/** The tiles home shows, in order: those of enabled modules, and the ones shown whatever modules are on (incl. grey ones). */
export function homeTiles(modules: Schemas['ModuleState'][], tiles = TILES) {
  const enabled = new Set(modules.filter((m) => m.enabled).map((m) => m.id))
  return tiles.filter((t) => t.always || t.unbuilt || enabled.has(t.id))
}

/** Each stage's home before it is arranged (prototype `DEFAULTS`, only built features). A saved arrangement always
 * wins, so changing these never rewrites a home the user arranged. */
export const STAGE_DEFAULTS: Record<Stage, string[]> = {
  before: ['checklist', 'scene', 'mixer', 'screen-tools', 'bgm', 'sound', 'alerts', 'chat'],
  live: ['pending', 'chat', 'alerts', 'scene', 'mixer', 'bgm', 'counter', 'sound', 'screen-tools', 'queue', 'redemption', 'checkin'],
  after: ['summary', 'counter', 'redemption', 'checkin', 'chat'],
}
export const stageDefaults = (stage: Stage, tiles = TILES) =>
  STAGE_DEFAULTS[stage].flatMap((id) => tiles.find((t) => t.id === id) ?? [])

/**
 * The home's tiles in saved order (`null`: never arranged, `defaults`). Unknown and repeated IDs are skipped.
 */
export function arrangedTiles(saved: string[] | null, tiles = TILES, defaults = tiles): Tile[] {
  if (!saved) return defaults
  const byId = new Map(tiles.map((t) => [t.id, t]))
  return [...new Set(saved)].flatMap((id) => byId.get(id) ?? [])
}

/** Tiles not on the home: removed ones and features added since the last arrange (never inserted on their own). */
export function availableTiles(arranged: Tile[], tiles = TILES): Tile[] {
  const shown = new Set(arranged.map((t) => t.id))
  return tiles.filter((t) => !shown.has(t.id))
}

/** IDs with `id` swapped with its neighbour (`step` -1: earlier, 1: later). */
export function moveTile(arranged: Tile[], id: string, step: -1 | 1): string[] {
  const ids = arranged.map((t) => t.id)
  const from = ids.indexOf(id)
  const to = from + step
  if (from >= 0 && arranged[to]) [ids[from], ids[to]] = [ids[to], ids[from]]
  return ids
}

/** IDs with `id` dragged onto `target`: it takes the target's place. */
export function dropTile(arranged: Tile[], id: string, target: string): string[] {
  const ids = arranged.map((t) => t.id)
  const from = ids.indexOf(id)
  const to = ids.indexOf(target)
  if (from < 0 || to < 0) return ids
  ids.splice(from, 1)
  ids.splice(to, 0, id)
  return ids
}

/** 1-based position of tile `id`, for announcements. */
export const tilePosition = (arranged: Tile[], id: string) => arranged.findIndex((t) => t.id === id) + 1

/** Toast rules: success dismisses itself; failure stays until the user closes it (no `life`). */
export const successToast = (summary: string) => ({ severity: 'success' as const, summary, life: 3000 })
export const errorToast = (summary: string) => ({ severity: 'error' as const, summary })
