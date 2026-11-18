import { afterEach, describe, expect, it, vi } from 'vitest'

const { client } = vi.hoisted(() => ({ client: { GET: vi.fn(), PUT: vi.fn() } }))
vi.mock('./api', () => ({ api: (p: Promise<unknown>) => p, client, ApiError: Error }))

import { measureSound, measureUnmeasured } from './sounds'

/** An `Audio` whose metadata says each file plays for `lengths[src]` seconds; a missing entry fails to load. */
function fakeAudio(lengths: Record<string, number>) {
  return class {
    preload = ''
    duration = NaN
    onloadedmetadata: (() => void) | null = null
    onerror: (() => void) | null = null
    set src(url: string) {
      queueMicrotask(() => {
        if (url in lengths) {
          this.duration = lengths[url]
          this.onloadedmetadata?.()
        } else this.onerror?.()
      })
    }
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('measureSound', () => {
  it('reads the length in ms, and nothing for a file it cannot read or an endless stream', async () => {
    vi.stubGlobal('Audio', fakeAudio({ '/sounds/a': 1.2345, '/sounds/live': Infinity }))
    expect(await measureSound('/sounds/a')).toBe(1235)
    expect(await measureSound('/sounds/live')).toBeUndefined()
    expect(await measureSound('/sounds/broken')).toBeUndefined()
  })
})

describe('measureUnmeasured', () => {
  it('saves the length of each sound that has none, skipping measured and unreadable ones', async () => {
    vi.stubGlobal('Audio', fakeAudio({ '/sounds/new': 2, '/sounds/old': 3 }))
    client.GET.mockResolvedValue({
      sounds: [
        { id: 'new', duration_ms: null },
        { id: 'old', duration_ms: 3000 },
        { id: 'broken', duration_ms: null },
      ],
      slots: {},
    })
    client.PUT.mockResolvedValue(undefined)
    await measureUnmeasured()
    expect(client.PUT).toHaveBeenCalledTimes(1)
    expect(client.PUT).toHaveBeenCalledWith('/api/admin/sounds/{id}/duration', {
      params: { path: { id: 'new' } },
      body: { duration_ms: 2000 },
    })
  })

  it('goes on after a sound whose length the Host refuses', async () => {
    vi.stubGlobal('Audio', fakeAudio({ '/sounds/first': 2, '/sounds/second': 3 }))
    client.GET.mockResolvedValue({
      sounds: [
        { id: 'first', duration_ms: null },
        { id: 'second', duration_ms: null },
      ],
      slots: {},
    })
    client.PUT.mockRejectedValueOnce(new Error('sound.invalid')).mockResolvedValue(undefined)
    await measureUnmeasured()
    expect(client.PUT).toHaveBeenCalledTimes(2)
    expect(client.PUT).toHaveBeenLastCalledWith('/api/admin/sounds/{id}/duration', {
      params: { path: { id: 'second' } },
      body: { duration_ms: 3000 },
    })
  })
})
