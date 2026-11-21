import { describe, expect, it, vi } from 'vitest'
import { type BgmPlayer, follow, positionMs, testPlayer } from './player'

const AT = '2026-11-21T10:00:00.000Z'
const track = { id: 't1', title: 'Lo-fi', url: '/music/t1', durationMs: 180_000 }
const status = (p: Partial<BgmPlayer> = {}): BgmPlayer => ({
  state: 'playing',
  playlist: 'p1',
  track,
  positionMs: 10_000,
  onAir: true,
  shuffle: false,
  volume: 40,
  ...p,
})

/** The parts of `<audio>` the overlay uses. */
function fakeAudio() {
  const attrs: Record<string, string> = {}
  return {
    volume: 1,
    currentTime: 0,
    paused: true,
    get src() {
      return attrs.src ? `http://127.0.0.1:47810${attrs.src}` : ''
    },
    set src(url: string) {
      attrs.src = url
      this.currentTime = 0
    },
    getAttribute: (name: string) => attrs[name] ?? null,
    removeAttribute: (name: string) => void delete attrs[name],
    play: vi.fn(function (this: { paused: boolean }) {
      this.paused = false
      return Promise.resolve()
    }),
    pause: vi.fn(function (this: { paused: boolean }) {
      this.paused = true
    }),
  }
}

describe('positionMs', () => {
  const now = Date.parse(AT) + 2500
  it('runs on from the Host position only while it plays on air', () => {
    expect(positionMs(status(), AT, now)).toBe(12_500)
    expect(positionMs(status({ state: 'paused' }), AT, now)).toBe(10_000)
    expect(positionMs(status({ onAir: false }), AT, now)).toBe(10_000)
  })

  it('never passes the end of the track', () => {
    expect(positionMs(status({ positionMs: 179_000 }), AT, now)).toBe(180_000)
    expect(positionMs(status({ track: null, state: 'stopped', positionMs: 0 }), AT, now)).toBe(0)
  })
})

describe('follow', () => {
  it('starts the track from where the Host is, at its volume', () => {
    vi.useFakeTimers({ now: Date.parse(AT) + 1000 })
    const audio = fakeAudio()
    follow(audio as unknown as HTMLAudioElement, status(), AT)
    expect(audio.getAttribute('src')).toBe('/music/t1')
    expect(audio.currentTime).toBe(11)
    expect(audio.volume).toBe(0.4)
    expect(audio.paused).toBe(false)
    vi.useRealTimers()
  })

  it('does not skip for a small drift, seeks for a large one, and pauses or stops', () => {
    vi.useFakeTimers({ now: Date.parse(AT) })
    const audio = fakeAudio()
    const a = audio as unknown as HTMLAudioElement
    follow(a, status(), AT)
    audio.currentTime = 10.6
    follow(a, status({ volume: 80 }), AT)
    expect(audio.currentTime).toBe(10.6)
    expect(audio.volume).toBe(0.8)
    follow(a, status({ positionMs: 60_000 }), AT)
    expect(audio.currentTime).toBe(60)

    follow(a, status({ state: 'paused', positionMs: 60_000 }), AT)
    expect(audio.paused).toBe(true)
    follow(a, status({ state: 'stopped', track: null, positionMs: 0 }), AT)
    expect(audio.getAttribute('src')).toBeNull()
    vi.useRealTimers()
  })
})

describe('testPlayer', () => {
  const test = { track, volume: 30, durationMs: 5000 }

  it('plays a test from the start on its own element for its time, then stops', () => {
    vi.useFakeTimers()
    const real = fakeAudio()
    follow(real as unknown as HTMLAudioElement, status({ state: 'paused' }), AT)
    const own = fakeAudio()
    const player = testPlayer(own as unknown as HTMLAudioElement)
    player.play(test)
    expect([own.getAttribute('src'), own.currentTime, own.volume, own.paused]).toEqual(['/music/t1', 0, 0.3, false])
    // Another test starts the time again.
    vi.advanceTimersByTime(4000)
    player.play(test)
    vi.advanceTimersByTime(4000)
    expect(own.paused).toBe(false)
    vi.advanceTimersByTime(1000)
    expect([own.paused, own.getAttribute('src')]).toEqual([true, null])
    // The player's own audio was never touched.
    expect([real.paused, real.currentTime]).toEqual([true, 10])
    vi.useRealTimers()
  })

  it('stops at once when the player starts playing, not when it pauses or stops', () => {
    const own = fakeAudio()
    const player = testPlayer(own as unknown as HTMLAudioElement)
    player.play(test)
    player.player(status({ state: 'paused' }))
    player.player(status({ state: 'stopped', track: null }))
    expect(own.paused).toBe(false)
    player.player(status())
    expect(own.paused).toBe(true)
  })
})
