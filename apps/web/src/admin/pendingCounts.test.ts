import { flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import type { Incoming } from '../overlay/chat/chat'

const { client } = vi.hoisted(() => ({ client: { GET: vi.fn() } }))
vi.mock('./api', async (original) => ({ ...(await original<typeof import('./api')>()), client }))
import { usePendingCounts } from './pendingCounts'

const ok = (data: unknown) => ({ data, response: new Response(null, { status: 200 }) })
const board = (n: number) => ({ singing: null, queued: Array.from({ length: n }, (_, i) => ({ id: `r${i}` })), sung: [] })
const msg = (type: string) => ({ type }) as unknown as Incoming

let queued = 2
let unfulfilled = 7
beforeEach(() => {
  queued = 2
  unfulfilled = 7
  client.GET.mockReset()
  client.GET.mockImplementation((path: string) =>
    Promise.resolve(ok(path === '/api/admin/song-requests' ? board(queued) : { total: unfulfilled, failed_writes: 0, items: [] })),
  )
})

function counts(hidden = new Set<string>()) {
  const off = ref(hidden)
  const scope = effectScope()
  const pending = scope.run(() => usePendingCounts(off))!
  return { pending, off }
}

describe('usePendingCounts', () => {
  it('an older read that answers last does not undo a newer count', async () => {
    const { pending } = counts()
    let answerOld!: (v: unknown) => void
    client.GET.mockImplementationOnce(() => new Promise((r) => (answerOld = r)))
    pending.onMessage(msg('songs.changed'))
    pending.onMessage(msg('songs.changed'))
    await flushPromises()
    expect(pending.countOf(['/songs'])).toBe(2)
    answerOld(ok(board(5)))
    await flushPromises()
    expect(pending.countOf(['/songs'])).toBe(2)
  })

  it('counts the whole queue and every unfulfilled redemption, not just a first page', async () => {
    const { pending } = counts()
    pending.onMessage(msg('snapshot'))
    await flushPromises()
    expect(pending.countOf(['/songs'])).toBe(2)
    expect(pending.countOf(['/redemptions', '/rewards'])).toBe(7)
    expect(pending.countOf(['/songs', '/redemptions'])).toBe(9)
    expect(client.GET).toHaveBeenCalledWith('/api/admin/redemptions', { params: { query: { status: 'unfulfilled', limit: 1 } } })
  })

  it('reloads only what a live change touches', async () => {
    const { pending } = counts()
    pending.onMessage(msg('snapshot'))
    await flushPromises()
    client.GET.mockClear()
    queued = 3
    pending.onMessage(msg('songs.changed'))
    await flushPromises()
    expect(client.GET).toHaveBeenCalledTimes(1)
    expect(pending.countOf(['/songs'])).toBe(3)
    unfulfilled = 1
    pending.onMessage(msg('redemption.changed'))
    await flushPromises()
    expect(pending.countOf(['/redemptions'])).toBe(1)
  })

  it('counts nothing for a page that is off, and starts counting when it is turned on', async () => {
    const { pending, off } = counts(new Set(['/redemptions']))
    pending.onMessage(msg('snapshot'))
    await flushPromises()
    expect(pending.countOf(['/redemptions'])).toBe(0)
    expect(client.GET).not.toHaveBeenCalledWith('/api/admin/redemptions', expect.anything())
    off.value = new Set()
    await nextTick()
    await flushPromises()
    expect(pending.countOf(['/redemptions'])).toBe(7)
  })

  it('drops a count that comes back after its page was turned off', async () => {
    let answer: (v: unknown) => void = () => {}
    client.GET.mockImplementation((path: string) =>
      path === '/api/admin/song-requests' ? Promise.resolve(ok(board(0))) : new Promise((resolve) => (answer = resolve)),
    )
    const { pending, off } = counts()
    pending.onMessage(msg('snapshot'))
    off.value = new Set(['/redemptions'])
    await nextTick()
    answer(ok({ total: 4, failed_writes: 0, items: [] }))
    await flushPromises()
    expect(pending.countOf(['/redemptions'])).toBe(0)
  })

  it('keeps the last number when a read fails', async () => {
    const { pending } = counts()
    pending.onMessage(msg('snapshot'))
    await flushPromises()
    client.GET.mockRejectedValue(new Error('offline'))
    pending.onMessage(msg('songs.changed'))
    await flushPromises()
    expect(pending.countOf(['/songs'])).toBe(2)
  })
})
