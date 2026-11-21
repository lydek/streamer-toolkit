import { reactive } from 'vue'
import { api, client } from './api'

/** The docked chat panel's width range in CSS pixels (the Host checks the same range). */
export const DOCK_MIN = 280
export const DOCK_MAX = 560
export const DOCK_DEFAULT = 340

/**
 * The docked chat panel, shared by the app bar's chat button and the panel. `open` and `width` are the docked
 * state on wide windows, kept in settings.json; `sheet` is the slide-in panel on narrow windows, never saved.
 * Never saved: open (docs/decisions.md). Closed until the saved state has loaded, so a saved "closed" never flashes
 * open. `docked`: the panel is beside the pages right now (ChatDock.vue sets it), so home hides its chat tile.
 */
const state = reactive({ open: false, width: DOCK_DEFAULT, sheet: false, docked: false })
let loading: Promise<void> | undefined

export function useDock() {
  loading ??= api(client.GET('/api/admin/chat-dock'))
    .then((saved) => {
      Object.assign(state, saved ? { open: saved.open, width: clampWidth(saved.width) } : { open: true })
    })
    .catch(() => {
      loading = undefined
    })
  return state
}

export const clampWidth = (w: number) => Math.round(Math.min(DOCK_MAX, Math.max(DOCK_MIN, w)))

/** Saves the docked state; the caller reports a failure. */
export const saveDock = () =>
  api(client.PUT('/api/admin/chat-dock', { body: { open: state.open, width: state.width } }))
