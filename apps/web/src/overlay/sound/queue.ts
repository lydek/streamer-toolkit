import type { components } from '../../api/schema'
import { trackSound } from '../sound'

export type PlaySound = components['schemas']['PlaySound']

/** shortcut: beyond this many waiting sounds the oldest is dropped, like the alert queue; raise it if streams reach it. */
const WAITING_MAX = 100

/** Plays sounds one at a time in the order they come; `play` resolves when one has finished (or failed). `clear` drops the waiting ones. */
export function soundQueue(play: (s: PlaySound) => Promise<void>) {
  const waiting: PlaySound[] = []
  let playing = false
  async function run() {
    playing = true
    for (let next = waiting.shift(); next; next = waiting.shift()) await play(next).catch(() => {})
    playing = false
  }
  const enqueue = (s: PlaySound) => {
    if (waiting.length === WAITING_MAX) waiting.shift()
    waiting.push(s)
    if (!playing) void run()
  }
  return Object.assign(enqueue, { clear: () => void waiting.splice(0) })
}

/** Plays `s` to its end; a sound that fails or is refused ends at once, so the next one is never held up. */
export function playToEnd(s: PlaySound): Promise<void> {
  return new Promise((done) => {
    const audio = new Audio(s.url)
    audio.volume = Math.min(Math.max(s.volume, 0), 100) / 100
    trackSound(audio)
    audio.onended = audio.onerror = () => done()
    audio.play()?.catch(() => done())
  })
}
