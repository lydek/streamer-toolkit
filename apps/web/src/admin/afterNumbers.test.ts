import { flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const get = vi.hoisted(() => vi.fn())
const put = vi.hoisted(() => vi.fn())
vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: get, PUT: put },
  api: (p: Promise<unknown>) => p,
}))

// The state is shared for the whole window, so each case loads a fresh copy.
async function loaded(saved: unknown) {
  vi.resetModules()
  get.mockResolvedValue(saved)
  const mod = await import('./afterNumbers')
  const state = mod.useAfterNumbers()
  await flushPromises()
  return { ...mod, state }
}

beforeEach(() => {
  get.mockReset()
  put.mockReset()
})

describe('after numbers', () => {
  it('shows every number until a choice is saved, then masks all but the revealed ones', async () => {
    const shown = await loaded(null)
    expect(shown.state.mode).toBe('show')
    expect(shown.isMasked('counter')).toBe(false)
    const { isMasked } = await loaded({ mode: 'mask', revealed: ['counter'] })
    expect(isMasked('counter')).toBe(false)
    expect(isMasked('checkin')).toBe(true)
  })

  it('masks every number again after showing all or folding them away', async () => {
    const { isMasked, saveAfterNumbers } = await loaded({ mode: 'mask', revealed: ['sound'] })
    put.mockResolvedValue(undefined)
    await saveAfterNumbers({ mode: 'show' })
    expect(put).toHaveBeenLastCalledWith('/api/admin/after-numbers', { body: { mode: 'show', revealed: [] } })
    await saveAfterNumbers({ mode: 'mask' })
    expect(isMasked('sound')).toBe(true)
  })

  it('reveals one number by saving it with the others, and puts the choice back when saving fails', async () => {
    const { state, reveal, saveAfterNumbers } = await loaded({ mode: 'mask', revealed: ['counter'] })
    put.mockResolvedValue(undefined)
    await reveal('stats.redemptions')
    expect(put).toHaveBeenLastCalledWith('/api/admin/after-numbers', { body: { mode: 'mask', revealed: ['counter', 'stats.redemptions'] } })
    put.mockRejectedValue(new Error('down'))
    await expect(saveAfterNumbers({ mode: 'hide' })).rejects.toThrow('down')
    expect(state).toEqual({ mode: 'mask', revealed: ['counter', 'stats.redemptions'] })
  })

  it('saves one change at a time, and an older refused change leaves the newer choice', async () => {
    const { state, saveAfterNumbers } = await loaded({ mode: 'show', revealed: [] })
    let refuse!: (e: Error) => void
    put.mockReturnValueOnce(new Promise((_, reject) => (refuse = reject))).mockResolvedValueOnce(undefined)
    const mask = saveAfterNumbers({ mode: 'mask' })
    const hide = saveAfterNumbers({ mode: 'hide' })
    await flushPromises()
    expect(put).toHaveBeenCalledTimes(1)
    refuse(new Error('down'))
    await expect(mask).rejects.toThrow('down')
    await hide
    expect(put).toHaveBeenLastCalledWith('/api/admin/after-numbers', { body: { mode: 'hide', revealed: [] } })
    expect(state.mode).toBe('hide')
  })
})
