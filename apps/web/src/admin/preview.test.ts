import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { previewUrl, stopPreview, togglePreview } from './preview'

class FakeAudio {
  static made: FakeAudio[] = []
  volume = 1
  paused = true
  onended: (() => void) | null = null
  onerror: (() => void) | null = null
  constructor(public src: string) {
    FakeAudio.made.push(this)
  }
  play() {
    this.paused = false
    return Promise.resolve()
  }
  pause() {
    this.paused = true
  }
}

beforeEach(() => {
  FakeAudio.made = []
  vi.stubGlobal('Audio', FakeAudio)
})
afterEach(() => {
  stopPreview()
  vi.unstubAllGlobals()
})

describe('preview', () => {
  it('plays one at a time, and a second press stops it', () => {
    togglePreview('/sounds/a', 40)
    expect(previewUrl.value).toBe('/sounds/a')
    expect(FakeAudio.made[0].volume).toBe(0.4)

    togglePreview('/music/b')
    expect(FakeAudio.made[0].paused).toBe(true)
    expect(previewUrl.value).toBe('/music/b')

    togglePreview('/music/b')
    expect(FakeAudio.made[1].paused).toBe(true)
    expect(previewUrl.value).toBeUndefined()
  })

  it('ends when the file ends, and a row only stops its own preview', () => {
    togglePreview('/sounds/a')
    stopPreview('/sounds/other')
    expect(previewUrl.value).toBe('/sounds/a')
    FakeAudio.made[0].onended!()
    expect(previewUrl.value).toBeUndefined()
  })
})
