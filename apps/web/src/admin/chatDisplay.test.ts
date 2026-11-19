import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { LoggedChat } from './chatLog'

const { get, put } = vi.hoisted(() => ({ get: vi.fn(), put: vi.fn() }))
vi.mock('./api', () => ({ api: (p: Promise<unknown>) => p, client: { GET: get, PUT: put } }))

beforeEach(() => {
  vi.resetModules()
  vi.resetAllMocks()
  vi.useFakeTimers()
})
afterEach(() => vi.useRealTimers())

describe('chat display requests', () => {
  it('drains a static page and retries its missing avatar without another message', async () => {
    const { wantAvatars, avatars } = await import('./chatDisplay')
    get.mockImplementation(async (_, { params }) => {
      const ids: string[] = params.query.ids.split(',')
      return Object.fromEntries(ids.filter(id => id !== '1' || get.mock.calls.length > 2).map(id => [id, id]))
    })
    wantAvatars(Array.from({ length: 201 }, (_, i) => ({ payload: { userId: String(i + 1) } }) as LoggedChat))
    await vi.runAllTimersAsync()
    expect(avatars.value.size).toBe(201)
    expect(get).toHaveBeenCalledTimes(3)
  })

  it('ignores a badge response belonging to the previous account', async () => {
    const { loadBadges, badgeImages } = await import('./chatDisplay')
    let finish!: (v: unknown) => void
    get.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
    get.mockResolvedValueOnce([{ setId: 'subscriber', id: '1', title: 'B' }])
    loadBadges('A')
    loadBadges('B')
    await Promise.resolve()
    finish([{ setId: 'subscriber', id: '1', title: 'A' }])
    await Promise.resolve()
    expect(badgeImages.value.get('subscriber/1')?.title).toBe('B')
  })

  it('merges an early edit with the stored settings', async () => {
    const { DEFAULT_DISPLAY, saveChatDisplay } = await import('./chatDisplay')
    let finish!: (value: unknown) => void
    get.mockReturnValueOnce(new Promise(resolve => { finish = resolve }))
    const save = saveChatDisplay({ time: false })
    expect(put).not.toHaveBeenCalled()
    finish({ ...DEFAULT_DISPLAY, textSize: 150 })
    await save
    expect(put.mock.calls[0][1].body).toMatchObject({ time: false, textSize: 150 })
  })

  it('retries failed and empty badge reads while the account stays selected', async () => {
    const { loadBadges, badgeImages } = await import('./chatDisplay')
    get.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce([])
      .mockResolvedValueOnce([{ setId: 'vip', id: '1', title: 'VIP' }])
    loadBadges('42')
    await vi.runAllTimersAsync()
    expect(badgeImages.value.get('vip/1')?.title).toBe('VIP')
    expect(get).toHaveBeenCalledTimes(3)
  })

  it('saves in order and a failed older write leaves the newer selection intact', async () => {
    const { DEFAULT_DISPLAY, chatDisplay, saveChatDisplay } = await import('./chatDisplay')
    let reject!: () => void
    put.mockReturnValueOnce(new Promise((_, fail) => { reject = fail }))
    put.mockResolvedValueOnce(undefined)
    get.mockResolvedValueOnce(null)
    const { loadChatDisplay } = await import('./chatDisplay')
    await loadChatDisplay()
    const first = saveChatDisplay({ ...DEFAULT_DISPLAY, time: false }).catch(() => {})
    const next = { ...DEFAULT_DISPLAY, time: false, badges: false }
    const second = saveChatDisplay(next)
    await Promise.resolve()
    await Promise.resolve()
    expect(put).toHaveBeenCalledTimes(1)
    reject()
    await Promise.all([first, second])
    expect(chatDisplay.value).toEqual(next)
    expect(put.mock.calls[1][1].body).toEqual(next)
  })
})
