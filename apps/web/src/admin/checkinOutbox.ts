import { type Ref, onScopeDispose, shallowRef, watch } from 'vue'
import { type Schemas, api, client } from './api'

/*
 * The check-in outbox (backlog while paused, redemptions that kept failing), read by one shared 2 s poll while any
 * reader wants it: 設定 › 功能 › 打卡 and the check-in drawer (both through CheckinOutbox.vue) and the home page's
 * 待處理. Opening the drawer on home therefore never starts a second poll.
 */
const POLL_MS = 2000
const outbox = shallowRef<Schemas['CheckinOutbox']>()
let readers = 0
let timer: number | undefined
let reading: Promise<Schemas['CheckinOutbox']> | undefined
let reads = 0

/**
 * Reads the outbox now. Readers share the read in flight; `fresh` starts a new one (after a retry or skip, the read in
 * flight may predate it), and only the latest read is shown.
 */
export function loadOutbox(fresh = false): Promise<Schemas['CheckinOutbox']> {
  if (reading && !fresh) return reading
  const mine = ++reads
  const read: Promise<Schemas['CheckinOutbox']> = api(client.GET('/api/admin/checkin/outbox'))
    .then((o) => {
      if (mine === reads) outbox.value = o
      return o
    })
    .finally(() => {
      if (reading === read) reading = undefined
    })
  reading = read
  return read
}

function start() {
  if (readers++ === 0) timer = window.setInterval(() => void loadOutbox().catch(() => {}), POLL_MS)
}
function stop() {
  if (--readers === 0) window.clearInterval(timer)
}

/** The shared outbox while `wanted` is true (e.g. while the check-in module is on); stops reading with its scope. */
export function useCheckinOutbox(wanted: () => boolean = () => true): { outbox: Ref<Schemas['CheckinOutbox'] | undefined> } {
  let on = false
  const set = (want: boolean) => {
    if (want === on) return
    on = want
    if (want) {
      start()
      void loadOutbox().catch(() => {})
    } else stop()
  }
  watch(wanted, set, { immediate: true })
  onScopeDispose(() => set(false))
  return { outbox }
}
