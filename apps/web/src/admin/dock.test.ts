import { flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const get = vi.hoisted(() => vi.fn())
vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  client: { GET: get, PUT: vi.fn() },
  api: (p: Promise<unknown>) => p,
}))

// The dock's state is shared for the whole window, so each case loads a fresh copy.
async function dockAfterLoad(saved: unknown) {
  vi.resetModules()
  get.mockResolvedValue(saved)
  const { useDock } = await import('./dock')
  const dock = useDock()
  const before = dock.open
  await flushPromises()
  return { before, dock }
}

beforeEach(() => get.mockReset())

describe('useDock', () => {
  it('opens a dock that was never saved, but only once the saved state has loaded', async () => {
    const { before, dock } = await dockAfterLoad(null)
    expect(before).toBe(false)
    expect(dock.open).toBe(true)
  })

  it('keeps a saved "closed" closed, and the saved width', async () => {
    const { dock } = await dockAfterLoad({ open: false, width: 300 })
    expect(dock.open).toBe(false)
    expect(dock.width).toBe(300)
  })

  it('docks on the saved side, and on the right when none was saved', async () => {
    expect((await dockAfterLoad({ open: true, width: 300, side: 'left' })).dock.side).toBe('left')
    expect((await dockAfterLoad({ open: true, width: 300 })).dock.side).toBe('right')
  })

  it('saves the side with the rest of the docked state', async () => {
    const { dock } = await dockAfterLoad({ open: true, width: 300 })
    const { client } = await import('./api')
    const { saveDock } = await import('./dock')
    dock.side = 'left'
    await saveDock()
    expect(client.PUT).toHaveBeenCalledWith('/api/admin/chat-dock', { body: { open: true, width: 300, side: 'left' } })
  })
})

describe('shownWidth', () => {
  it('shows the saved width while the pages keep 720px, else narrows it, never below the minimum', async () => {
    const { shownWidth } = await import('./dock')
    expect(shownWidth(560, 1700)).toBe(560)
    expect(shownWidth(560, 1100)).toBe(380)
    expect(shownWidth(560, 900)).toBe(280)
    expect(shownWidth(300, 1100)).toBe(300)
  })
})
