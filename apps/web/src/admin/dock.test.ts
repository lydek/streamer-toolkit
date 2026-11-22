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

describe('shownWidth', () => {
  it('shows the saved width while the pages keep 720px, else narrows it, never below the minimum', async () => {
    const { shownWidth } = await import('./dock')
    expect(shownWidth(560, 1700)).toBe(560)
    expect(shownWidth(560, 1100)).toBe(380)
    expect(shownWidth(560, 900)).toBe(280)
    expect(shownWidth(300, 1100)).toBe(300)
  })
})
