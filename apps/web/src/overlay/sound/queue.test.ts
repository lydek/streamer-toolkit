import { describe, expect, it } from 'vitest'
import { type PlaySound, soundQueue } from './queue'

const sound = (n: number): PlaySound => ({ url: `/sounds/s${n}`, volume: 50 })

describe('soundQueue', () => {
  it('plays one at a time in order, going on after a failed one', async () => {
    const started: string[] = []
    const finish: (() => void)[] = []
    const enqueue = soundQueue((s) => {
      started.push(s.url)
      if (s.url.endsWith('2')) return Promise.reject(new Error('refused'))
      return new Promise((done) => finish.push(done))
    })
    enqueue(sound(1))
    enqueue(sound(2))
    enqueue(sound(3))
    await Promise.resolve()
    expect(started).toEqual(['/sounds/s1'])
    finish.shift()!()
    await new Promise((r) => setTimeout(r))
    expect(started).toEqual(['/sounds/s1', '/sounds/s2', '/sounds/s3'])
    finish.shift()!()
    await new Promise((r) => setTimeout(r))
    enqueue(sound(4))
    expect(started.at(-1)).toBe('/sounds/s4')
  })
})
