// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import type { ChatEvent, Incoming } from '../overlay/chat/chat'
import {
  EMPTY_LOG,
  LOG_LIMIT,
  type Rgb,
  applyChat,
  backgroundOf,
  contrast,
  nameColor,
  parseCssColor,
  parseHex,
  pause,
  readable,
  receive,
  resume,
} from './chatLog'

// The drawer background (theme surface.900) and the page background (surface.950).
const DRAWER: Rgb = [0x15, 0x17, 0x25]
const PAGE: Rgb = [0x07, 0x08, 0x15]
const WHITE: Rgb = [255, 255, 255]

function hue(rgb: Rgb) {
  const [r, g, b] = rgb.map((c) => c / 255)
  const max = Math.max(r, g, b)
  const d = max - Math.min(r, g, b)
  const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4
  return (h * 60 + 360) % 360
}

describe('name colors', () => {
  it('computes WCAG contrast', () => {
    expect(contrast(WHITE, [0, 0, 0])).toBeCloseTo(21)
    expect(contrast([0x76, 0x76, 0x76], WHITE)).toBeCloseTo(4.54, 2)
  })

  it('makes #0000FF readable on the drawer and keeps it blue', () => {
    const fixed = parseHex(nameColor('#0000FF', '1', DRAWER))!
    expect(contrast(fixed, DRAWER)).toBeGreaterThanOrEqual(4.5)
    expect(contrast(parseHex('#0000FF')!, DRAWER)).toBeLessThan(4.5)
    // OKLCH hue 264° lands near 230-250° in HSL for lighter blues; it must stay blue, not drift to purple or cyan.
    expect(hue(fixed)).toBeGreaterThan(215)
    expect(hue(fixed)).toBeLessThan(255)
  })

  it('leaves colors that already pass unchanged', () => {
    expect(nameColor('#FF7F50', '1', DRAWER)).toBe('#ff7f50')
  })

  it('darkens on a light background', () => {
    const fixed = parseHex(nameColor('#FFFF00', '1', WHITE))!
    expect(contrast(fixed, WHITE)).toBeGreaterThanOrEqual(4.5)
    expect(fixed[2]).toBeLessThan(fixed[0])
  })

  it('gives viewers without a color a fixed color that passes', () => {
    for (const id of ['1', '42', '987654', 'abc', '']) {
      const color = nameColor(null, id, DRAWER)
      expect(color).toBe(nameColor(null, id, DRAWER))
      expect(contrast(parseHex(color)!, DRAWER)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(parseHex(nameColor(null, id, WHITE))!, WHITE)).toBeGreaterThanOrEqual(4.5)
    }
    expect(nameColor(null, '1', DRAWER)).not.toBe(nameColor(null, '2', DRAWER))
  })

  it('reaches 4.5:1 for every hue and lightness on dark and mid backgrounds', () => {
    for (const bg of [DRAWER, PAGE, [0x76, 0x76, 0x76] as Rgb]) {
      for (let n = 0; n < 0x1000000; n += 0x0f0f0f + 0x10101) {
        const c: Rgb = [(n >> 16) & 255, (n >> 8) & 255, n & 255]
        expect(contrast(readable(c, bg), bg)).toBeGreaterThanOrEqual(4.5)
      }
    }
  })

  it('reads the painted background', () => {
    expect(parseCssColor('rgb(21, 23, 37)')).toEqual([21, 23, 37])
    expect(parseCssColor('rgba(0, 0, 0, 0)')).toBeNull()
    expect(parseCssColor('rgb(1 2 3 / 50%)')).toEqual([1, 2, 3])
    const outer = document.createElement('div')
    outer.style.backgroundColor = 'rgb(21, 23, 37)'
    const inner = document.createElement('div')
    outer.append(inner)
    document.body.append(outer)
    expect(backgroundOf(inner, WHITE)).toEqual([21, 23, 37])
    expect(backgroundOf(null, WHITE)).toEqual(WHITE)
    outer.remove()
  })
})

const chat = (n: number): ChatEvent => ({
  id: `c${n}`,
  type: 'chat.message',
  version: 1,
  seq: n,
  occurredAt: '2026-11-15T10:00:00.000Z',
  source: 'twitch',
  payload: { userId: '7', userLogin: 'v', userName: 'V', color: null, text: `m${n}`, fragments: [], badges: [], highlighted: false },
})
const snapshot = (...ns: number[]): Incoming => ({
  type: 'snapshot',
  version: 1,
  seq: 0,
  payload: { 'chat.message': ns.map(chat), 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [] },
})
const ids = (log: { messages: ChatEvent[] }) => log.messages.map((m) => m.seq)

describe('chat log', () => {
  it('merges a reconnect snapshot by id without duplicates', () => {
    let log = applyChat(EMPTY_LOG, snapshot(1, 2))
    log = applyChat(log, chat(3))
    // Reconnect: the new snapshot overlaps what is shown and adds what was missed while disconnected.
    log = applyChat(log, snapshot(2, 3, 4, 5))
    expect(ids(log)).toEqual([1, 2, 3, 4, 5])
    expect(applyChat(log, snapshot(4, 5))).toBe(log)
  })

  it('counts new messages while paused and keeps the paused state across a reconnect', () => {
    let log = pause(applyChat(EMPTY_LOG, snapshot(1)))
    log = applyChat(log, chat(2))
    log = applyChat(log, snapshot(1, 2, 3, 4))
    expect(log).toMatchObject({ paused: true, unseen: 3 })
    log = resume(log)
    expect(log).toMatchObject({ paused: false, unseen: 0 })
    expect(ids(applyChat(log, chat(5)))).toEqual([1, 2, 3, 4, 5])
    expect(applyChat(log, chat(5)).unseen).toBe(0)
  })

  it('trims the top only while following', () => {
    const many = Array.from({ length: LOG_LIMIT + 10 }, (_, i) => chat(i))
    expect(receive(EMPTY_LOG, many).messages).toHaveLength(LOG_LIMIT)
    const paused = receive(pause(EMPTY_LOG), many)
    expect(paused.messages).toHaveLength(LOG_LIMIT + 10)
    expect(resume(paused).messages).toHaveLength(LOG_LIMIT)
  })

  it('ignores other events', () => {
    const log = applyChat(EMPTY_LOG, snapshot(1))
    expect(applyChat(log, { type: 'template.changed', id: 't', version: 1, seq: 9, occurredAt: '', source: 'system', payload: { overlay: 'chat' } })).toBe(log)
  })
})
