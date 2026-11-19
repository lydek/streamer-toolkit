import { onScopeDispose, shallowRef, watch } from 'vue'
import type { Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'

/** The Twitch status the app bar polls every 2 s (undefined until its first answer); read it instead of polling again. */
export const twitchOverview = shallowRef<Schemas['Overview']>()

/** A status light in the status bar. `key` is the i18n key of its state text. */
export interface Light {
  state: 'connected' | 'reconnecting' | 'error' | 'loading'
  key: string
}

/** One tile status line: never blank, never a stale value while loading or failed. */
export type Line = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; text: string }

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
  if (msg.type === 'chat.message') next.lastChat = msg
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

/** Follows `source` at most once per `ms`, always ending with its latest value (trailing update). */
export function useThrottled<T>(source: () => T, ms = 3000) {
  const shown = shallowRef(source())
  let last = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  const flush = () => {
    timer = undefined
    last = Date.now()
    shown.value = source()
  }
  watch(source, () => {
    if (timer) return
    const wait = last + ms - Date.now()
    if (wait <= 0) flush()
    else timer = setTimeout(flush, wait)
  })
  onScopeDispose(() => clearTimeout(timer))
  return shown
}

export type Group = 'live' | 'features'

export interface Tile {
  id: string
  group: Group
  icon: string
}

/** Default home tiles, one per module; a disabled module shows no tile. */
export const TILES: Tile[] = [
  { id: 'chat', group: 'live', icon: 'pi pi-comments' },
  { id: 'alerts', group: 'live', icon: 'pi pi-bell' },
  { id: 'redemption', group: 'features', icon: 'pi pi-gift' },
  { id: 'checkin', group: 'features', icon: 'pi pi-check-square' },
  { id: 'sound', group: 'features', icon: 'pi pi-volume-up' },
]

/** Non-empty groups in display order. */
export function groupTiles(tiles: Tile[]) {
  return (['live', 'features'] as const)
    .map((group) => ({ group, tiles: tiles.filter((t) => t.group === group) }))
    .filter((g) => g.tiles.length > 0)
}

/** Non-empty groups in display order, each with the tiles of its enabled modules. */
export function tileGroups(modules: Schemas['ModuleState'][], tiles = TILES) {
  const enabled = new Set(modules.filter((m) => m.enabled).map((m) => m.id))
  return groupTiles(tiles.filter((t) => enabled.has(t.id)))
}

/** The home's tiles in saved order (`null`: never arranged, the defaults). Unknown and repeated IDs are skipped. */
export function arrangedTiles(saved: string[] | null, tiles = TILES): Tile[] {
  if (!saved) return tiles
  const byId = new Map(tiles.map((t) => [t.id, t]))
  return [...new Set(saved)].flatMap((id) => byId.get(id) ?? [])
}

/** Tiles not on the home: removed ones and features added since the last arrange (never inserted on their own). */
export function availableTiles(arranged: Tile[], tiles = TILES): Tile[] {
  const shown = new Set(arranged.map((t) => t.id))
  return tiles.filter((t) => !shown.has(t.id))
}

/** IDs with `id` swapped with its nearest tile of the same group (`step` -1: earlier, 1: later). */
export function moveTile(arranged: Tile[], id: string, step: -1 | 1): string[] {
  const ids = arranged.map((t) => t.id)
  const from = ids.indexOf(id)
  let to = from + step
  while (arranged[to] && arranged[to].group !== arranged[from].group) to += step
  if (from >= 0 && arranged[to]) [ids[from], ids[to]] = [ids[to], ids[from]]
  return ids
}

/** IDs with `id` dragged onto `target` (same group): it takes the target's place. */
export function dropTile(arranged: Tile[], id: string, target: string): string[] {
  const ids = arranged.map((t) => t.id)
  const from = ids.indexOf(id)
  const to = ids.indexOf(target)
  if (from < 0 || to < 0) return ids
  ids.splice(from, 1)
  ids.splice(to, 0, id)
  return ids
}

/** 1-based position of tile `id` within its group, for announcements. */
export function groupPosition(arranged: Tile[], id: string): number {
  const group = arranged.find((t) => t.id === id)?.group
  return arranged.filter((t) => t.group === group).findIndex((t) => t.id === id) + 1
}

/** Toast rules: success dismisses itself; failure stays until the user closes it (no `life`). */
export const successToast = (summary: string) => ({ severity: 'success' as const, summary, life: 3000 })
export const errorToast = (summary: string) => ({ severity: 'error' as const, summary })
