import { onScopeDispose, shallowRef, watch } from 'vue'
import type { Incoming } from '../overlay/chat/chat'
import type { Schemas } from './api'

/** A status light in the status bar. `key` is the i18n key of its state text. */
export interface Light {
  state: 'connected' | 'reconnecting' | 'error' | 'loading'
  key: string
}

/** One tile status line: never blank, never a stale value while loading or failed. */
export type Line = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; text: string }

/** The id of the element tiles and lights open (`aria-controls`). */
export const PANEL_ID = 'home-panel'

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
      return { state: 'error', key: 'home.status.twitch.failed' }
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

/** What the home page keeps from `/ws/overlay`. `loading` until the first snapshot, `failed` while disconnected. */
export interface LiveState {
  status: 'loading' | 'open' | 'failed'
  lastSeq: number
  obs?: Schemas['ObsStatusPayload']
  lastChat?: Schemas['ChatMessageEvent']
}

export function applyLive(state: LiveState, msg: Incoming): LiveState {
  if (msg.type === 'snapshot') {
    return {
      status: 'open',
      lastSeq: msg.seq,
      obs: msg.payload['obs.status'].at(-1)?.payload,
      lastChat: msg.payload['chat.message'].at(-1),
    }
  }
  // Events before the snapshot, or replays, are already covered by it.
  if (state.status !== 'open' || msg.seq <= state.lastSeq) return state
  const next = { ...state, lastSeq: msg.seq }
  if (msg.type === 'obs.status') next.obs = msg.payload
  if (msg.type === 'chat.message') next.lastChat = msg
  return next
}

/** Connects to `/ws/overlay` (read-only, no token) and reconnects until the calling scope ends. */
export function useLiveEvents(retryMs = 2000) {
  const state = shallowRef<LiveState>({ status: 'loading', lastSeq: 0 })
  let ws: WebSocket | undefined
  let retry: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  function connect() {
    ws = new WebSocket(`ws://${location.host}/ws/overlay`)
    ws.onmessage = (e) => (state.value = applyLive(state.value, JSON.parse(e.data) as Incoming))
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
export const TILES: Tile[] = [{ id: 'chat', group: 'live', icon: 'pi pi-comments' }]

/** Non-empty groups in display order, each with the tiles of its enabled modules. */
export function tileGroups(modules: Schemas['ModuleState'][], tiles = TILES) {
  const enabled = new Set(modules.filter((m) => m.enabled).map((m) => m.id))
  return (['live', 'features'] as const)
    .map((group) => ({ group, tiles: tiles.filter((t) => t.group === group && enabled.has(t.id)) }))
    .filter((g) => g.tiles.length > 0)
}

/** Toast rules: success dismisses itself; failure stays until the user closes it (no `life`). */
export const successToast = (summary: string) => ({ severity: 'success' as const, summary, life: 3000 })
export const errorToast = (summary: string) => ({ severity: 'error' as const, summary })
