// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { playSound, stopSounds } from './sound'
import { playToEnd } from './sound/queue'

afterEach(() => vi.restoreAllMocks())

describe('overlay sounds', () => {
  it('forgets a sound the browser refuses to play, and still stops one that plays', async () => {
    const pause = vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {})
    const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockRejectedValue(new Error('NotAllowedError'))
    playSound('/sounds/a', 50)
    // The queue goes on at once.
    await expect(playToEnd({ url: '/sounds/b', volume: 50 })).resolves.toBeUndefined()
    await Promise.resolve()
    stopSounds()
    expect(pause).not.toHaveBeenCalled()

    play.mockResolvedValue(undefined)
    playSound('/sounds/c', 50)
    stopSounds()
    expect(pause).toHaveBeenCalledTimes(1)
  })
})
