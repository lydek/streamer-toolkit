// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'

const { client } = vi.hoisted(() => ({ client: { GET: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))
import { loadOutbox, useCheckinOutbox } from './checkinOutbox'

const ok = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
const reads = () => client.GET.mock.calls.filter(([path]) => path === '/api/admin/checkin/outbox').length

beforeEach(() => {
  vi.useFakeTimers()
  client.GET.mockReset()
  client.GET.mockResolvedValue(ok({ pending: 0, oldest_pending_at: null, dead_letters: [] }))
})
afterEach(() => vi.useRealTimers())

describe('useCheckinOutbox', () => {
  it('reads with one poll however many readers there are, and stops when the last one goes', async () => {
    const settings = effectScope()
    const drawer = effectScope()
    settings.run(() => useCheckinOutbox())
    drawer.run(() => useCheckinOutbox())
    await flushPromises()
    const first = reads()
    await vi.advanceTimersByTimeAsync(2000)
    expect(reads() - first).toBe(1)
    drawer.stop()
    await vi.advanceTimersByTimeAsync(2000)
    expect(reads() - first).toBe(2)
    settings.stop()
    await vi.advanceTimersByTimeAsync(4000)
    expect(reads() - first).toBe(2)
  })

  it('reads only while wanted (home, while the check-in module is on), and shares what it read', async () => {
    const on = ref(false)
    const home = effectScope()
    const { outbox } = home.run(() => useCheckinOutbox(() => on.value))!
    await vi.advanceTimersByTimeAsync(4000)
    expect(reads()).toBe(0)
    client.GET.mockResolvedValue(ok({ pending: 0, oldest_pending_at: null, dead_letters: [{ event_id: 'e1' }] }))
    on.value = true
    await nextTick()
    await flushPromises()
    expect(outbox.value?.dead_letters).toHaveLength(1)
    on.value = false
    await nextTick()
    const before = reads()
    await vi.advanceTimersByTimeAsync(4000)
    expect(reads()).toBe(before)
    home.stop()
  })

  it('a fresh read after a change wins over a read that was already on its way', async () => {
    const before = { pending: 0, oldest_pending_at: null, dead_letters: [{ event_id: 'e1' }] }
    const after = { pending: 0, oldest_pending_at: null, dead_letters: [] }
    let answerOld!: (v: unknown) => void
    client.GET.mockReturnValueOnce(new Promise((r) => (answerOld = r))).mockResolvedValueOnce(ok(after))
    const scope = effectScope()
    const { outbox } = scope.run(() => useCheckinOutbox(() => false))!
    const old = loadOutbox()
    await loadOutbox(true)
    expect(outbox.value).toEqual(after)
    answerOld(ok(before))
    await old
    expect(outbox.value).toEqual(after)
    scope.stop()
  })
})
