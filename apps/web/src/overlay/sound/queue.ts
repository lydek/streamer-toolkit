import type { components } from '../../api/schema'
import { playSound } from '../sound'

export type PlaySound = components['schemas']['PlaySound']

/** shortcut: beyond this many waiting sounds the oldest is dropped, like the alert queue; raise it if streams reach it. */
const WAITING_MAX = 100

/**
 * Plays sounds one at a time in the order they come; `play` resolves when one has finished (or failed). `test` marks a
 * sound from the test button. `clear` drops the waiting ones; `clearTests` drops the waiting test sounds and says
 * whether the one playing is a test sound.
 */
export function soundQueue(play: (s: PlaySound) => Promise<void>) {
  const waiting: { sound: PlaySound; test: boolean }[] = []
  let current: (typeof waiting)[number] | undefined
  async function run() {
    for (current = waiting.shift(); current; current = waiting.shift()) await play(current.sound).catch(() => {})
  }
  const enqueue = (sound: PlaySound, test = false) => {
    if (waiting.length === WAITING_MAX) waiting.shift()
    waiting.push({ sound, test })
    if (!current) void run()
  }
  const clearTests = () => {
    waiting.splice(0, waiting.length, ...waiting.filter((w) => !w.test))
    return current?.test ?? false
  }
  return Object.assign(enqueue, { clear: () => void waiting.splice(0), clearTests })
}

/** Plays `s` to its end; a sound that fails or is refused ends at once, so the next one is never held up. */
export function playToEnd(s: PlaySound): Promise<void> {
  return new Promise((done) => {
    // A refusal ends it (in `playSound`, after this handler is set), so the queue goes on.
    const audio = playSound(s.url, s.volume)
    audio.onended = audio.onerror = () => done()
  })
}
