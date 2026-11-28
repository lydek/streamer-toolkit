import { type Ref, computed, ref, watch } from 'vue'
import { api, client } from './api'
import type { Incoming } from '../overlay/chat/chat'

/** The pages whose waiting items the tabs count: song requests in the queue, redemptions not yet fulfilled. */
export const COUNTED = ['/songs', '/redemptions'] as const
type Counted = (typeof COUNTED)[number]

/**
 * What the tabs show as waiting, owned by the app bar so every page shows the same numbers. A page that is off
 * (in `hidden`) counts nothing and is not asked. `onMessage` takes each live message: a change on a page reloads its
 * count, and a snapshot (on connect and after a reconnect) reloads both.
 */
export function usePendingCounts(hidden: Ref<Set<string>>) {
  const counts = ref<Record<Counted, number>>({ '/songs': 0, '/redemptions': 0 })
  const loaders: Record<Counted, () => Promise<number>> = {
    '/songs': async () => (await api(client.GET('/api/admin/song-requests'))).queued.length,
    '/redemptions': async () =>
      (await api(client.GET('/api/admin/redemptions', { params: { query: { status: 'unfulfilled', limit: 1 } } }))).total,
  }
  // Reads of one page can overlap (a snapshot, then a change): only the latest read's answer counts.
  const latest: Record<Counted, number> = { '/songs': 0, '/redemptions': 0 }
  async function load(path: Counted) {
    const mine = ++latest[path]
    if (hidden.value.has(path)) {
      counts.value = { ...counts.value, [path]: 0 }
      return
    }
    // A failed read keeps the last number: the next change or reconnect tries again. An answer that comes back
    // after the page was turned off is dropped, or it would bring back the count just cleared.
    const n = await loaders[path]().catch(() => undefined)
    if (n !== undefined && mine === latest[path] && !hidden.value.has(path)) counts.value = { ...counts.value, [path]: n }
  }
  const loadAll = () => Promise.all(COUNTED.map(load))
  // Turning a feature on or off shows or clears its count.
  watch(
    () => COUNTED.map((p) => hidden.value.has(p)).join(),
    () => void loadAll(),
  )
  function onMessage(msg: Incoming) {
    if (msg.type === 'snapshot') void loadAll()
    else if (msg.type === 'songs.changed') void load('/songs')
    else if (msg.type === 'redemption.changed') void load('/redemptions')
  }
  /** The waiting items on these pages together. */
  const countOf = (paths: string[]) => paths.reduce((sum, p) => sum + (counts.value[p as Counted] ?? 0), 0)
  return { counts: computed(() => counts.value), countOf, onMessage, loadAll }
}
