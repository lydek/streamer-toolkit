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
})
