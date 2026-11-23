import { reactive } from 'vue'
import { type Schemas, api, client } from './api'

type AfterNumbers = Schemas['AfterNumbers']

/**
 * How the 下播後 home shows its numbers (settings.json): all shown, each masked as 「––」 until revealed one by one
 * (`revealed` keeps those), or the tiles folded away behind one card. Shown until the saved choice loads.
 */
const state = reactive<AfterNumbers>({ mode: 'show', revealed: [] })
let loading: Promise<void> | undefined
// What the Host holds: a failed save puts this back.
let held: AfterNumbers = { mode: 'show', revealed: [] }
// Saves go one at a time, so the Host ends with the last choice; `latest` numbers the newest.
let queue: Promise<unknown> = Promise.resolve()
let latest = 0

export function useAfterNumbers() {
  loading ??= api(client.GET('/api/admin/after-numbers'))
    .then((saved) => {
      if (saved) {
        Object.assign(state, saved)
        held = { mode: saved.mode, revealed: [...saved.revealed] }
      }
    })
    .catch(() => {
      loading = undefined
    })
  return state
}

/**
 * Saves a change and shows it at once. When the Host refuses the newest change, the choice it holds comes back;
 * an older refused change leaves the newer one alone.
 */
export async function saveAfterNumbers(change: Partial<AfterNumbers>) {
  Object.assign(state, change)
  // Revealed numbers last until all show or the tiles fold away: masking again masks them all.
  if (change.mode && change.mode !== 'mask' && !change.revealed) state.revealed = []
  const body = { mode: state.mode, revealed: [...state.revealed] }
  const mine = ++latest
  const run = queue.then(() => api(client.PUT('/api/admin/after-numbers', { body })))
  queue = run.catch(() => undefined)
  try {
    await run
    held = body
  } catch (e) {
    if (mine === latest) Object.assign(state, { mode: held.mode, revealed: [...held.revealed] })
    throw e
  }
}

/** Whether the number with this ID shows as 「––」. */
export const isMasked = (id: string) => state.mode === 'mask' && !state.revealed.includes(id)

/** Shows one masked number from now on (the Host keeps at most 50). */
export const reveal = (id: string) =>
  state.revealed.includes(id) ? Promise.resolve() : saveAfterNumbers({ revealed: [...state.revealed, id].slice(-50) })
