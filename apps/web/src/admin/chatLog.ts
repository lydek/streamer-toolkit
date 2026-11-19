import { onScopeDispose, shallowRef } from 'vue'
import { api, client } from './api'
import { type ChatEvent, type Incoming, removes } from '../overlay/chat/chat'
import type { Schemas } from './api'

// ---- Name colors (admin UI only; overlays follow their template) ----

export type Rgb = [number, number, number]

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055)

export function parseHex(hex: string): Rgb | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex)
  if (!m) return null
  const n = parseInt(m[1], 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

const toHex = (rgb: Rgb) => `#${rgb.map((c) => c.toString(16).padStart(2, '0')).join('')}`

/** WCAG 2 contrast ratio of two 8-bit sRGB colors. */
export function contrast(a: Rgb, b: Rgb): number {
  const lum = ([r, g, b]: Rgb) =>
    0.2126 * toLinear(r / 255) + 0.7152 * toLinear(g / 255) + 0.0722 * toLinear(b / 255)
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

type Oklch = [l: number, c: number, h: number]

// OKLab matrices from https://bottosson.github.io/posts/oklab/ (public domain).
function toOklch(rgb: Rgb): Oklch {
  const [r, g, b] = rgb.map((c) => toLinear(c / 255))
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  return [L, Math.hypot(A, B), Math.atan2(B, A)]
}

/** Linear sRGB; components may fall outside 0..1 when the color is out of gamut. */
function fromOklch([L, C, h]: Oklch): [number, number, number] {
  const A = C * Math.cos(h)
  const B = C * Math.sin(h)
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const inGamut = (lin: number[]) => lin.every((c) => c >= -1e-4 && c <= 1 + 1e-4)

/** The color at lightness `L` and hue `h`, with chroma lowered only as far as sRGB needs. */
function fit(L: number, C: number, h: number): Rgb {
  let lo = 0
  let hi = C
  if (!inGamut(fromOklch([L, hi, h]))) {
    for (let i = 0; i < 20; i++) {
      const mid = (lo + hi) / 2
      if (inGamut(fromOklch([L, mid, h]))) lo = mid
      else hi = mid
    }
    hi = lo
  }
  return fromOklch([L, hi, h]).map((c) => Math.round(Math.min(1, Math.max(0, toGamma(c))) * 255)) as Rgb
}

const MIN_CONTRAST = 4.5

/** Keeps the hue and moves OKLCH lightness away from `bg` until the color reaches 4.5:1 against it. */
export function readable(color: Rgb, bg: Rgb): Rgb {
  if (contrast(color, bg) >= MIN_CONTRAST) return color
  const [L, C, h] = toOklch(color)
  // White or black always reaches 4.5:1 against one side (max of the two is at least √21 ≈ 4.58).
  const up = contrast([255, 255, 255], bg) >= contrast([0, 0, 0], bg)
  for (let i = 1; i <= 100; i++) {
    const next = fit(up ? L + (1 - L) * (i / 100) : L * (1 - i / 100), C, h)
    if (contrast(next, bg) >= MIN_CONTRAST) return next
  }
  return up ? [255, 255, 255] : [0, 0, 0]
}

/** FNV-1a: a fixed hue per viewer, the same on every launch. */
function hash(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193)
  return h >>> 0
}

/** The viewer's Twitch color made readable on `bg`; viewers without one get a fixed color from their user ID. */
export function nameColor(color: string | null, userId: string, bg: Rgb): string {
  const base = (color && parseHex(color)) || fit(0.75, 0.12, ((hash(userId) % 360) * Math.PI) / 180)
  return toHex(readable(base, bg))
}

/** `rgb()`/`rgba()` as getComputedStyle reports it; `null` when transparent or in another format. */
export function parseCssColor(css: string): Rgb | null {
  const m = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/.exec(css)
  if (!m) return null
  const alpha = m[4] === undefined ? 1 : parseFloat(m[4]) / (m[4].endsWith('%') ? 100 : 1)
  return alpha > 0 ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}

/** The first painted background at or above `el`. */
export function backgroundOf(el: Element | null, fallback: Rgb): Rgb {
  for (; el; el = el.parentElement) {
    const rgb = parseCssColor(getComputedStyle(el).backgroundColor)
    if (rgb) return rgb
  }
  return fallback
}

// ---- The drawer's live log ----

/** A message in the admin log; a deleted one keeps its place with its text gone. */
export type LoggedChat = ChatEvent & { deleted?: boolean }

export interface ChatLog {
  messages: LoggedChat[]
  paused: boolean
  /** Messages added since the log was paused. */
  unseen: number
}

export const LOG_LIMIT = 200
// shortcut: a paused log stops trimming the top (that would move what the reader is looking at) up to this cap.
const PAUSED_LIMIT = 1000

export const EMPTY_LOG: ChatLog = { messages: [], paused: false, unseen: 0 }

/** Appends messages not already shown, by event `id`; a reconnect snapshot only adds what was missed. */
export function receive(log: ChatLog, incoming: LoggedChat[]): ChatLog {
  const known = new Set(log.messages.map((m) => m.id))
  const added = incoming.filter((m) => !known.has(m.id) && known.add(m.id))
  if (!added.length) return log
  const messages = [...log.messages, ...added].slice(-(log.paused ? PAUSED_LIMIT : LOG_LIMIT))
  return { messages, paused: log.paused, unseen: log.paused ? log.unseen + added.length : 0 }
}

/** Feeds one `/ws/overlay` message into the log; other event types leave it as it is. */
export function applyChat(log: ChatLog, msg: Incoming): ChatLog {
  if (msg.type === 'snapshot') {
    const snapshot = msg.payload['chat.message']
    const first = new Set(snapshot.filter((m) => m.payload.firstChat).map((m) => m.id))
    const next = receive(log, snapshot)
    const marked = first.size ? { ...next, messages: next.messages.map((m) =>
      first.has(m.id) ? { ...m, payload: { ...m.payload, firstChat: true } } : m) } : next
    return (msg.payload['chat.removed'] ?? []).reduce(applyChat, marked)
  }
  if (msg.type === 'chat.message') return receive(log, [msg])
  if (msg.type === 'chat.first') {
    const messages = log.messages.map((m) =>
      m.id === msg.payload.messageId && !m.payload.firstChat ? { ...m, payload: { ...m.payload, firstChat: true } } : m,
    )
    return { ...log, messages }
  }
  if (msg.type === 'chat.removed') {
    const messages = log.messages.map((m) =>
      !m.deleted && m.seq < msg.seq && (msg.payload.messageId || m.seq > 0 || !(Date.parse(m.occurredAt) > Date.parse(msg.occurredAt))) && removes(msg.payload, m) ? { ...m, deleted: true, payload: { ...m.payload, text: '', fragments: [] } } : m,
    )
    return { ...log, messages }
  }
  return log
}

/** A stored message as a live `chat.message` event, so both render the same. */
export const fromStored = (s: Schemas['StoredChatMessage']): LoggedChat => ({
  id: s.id,
  type: 'chat.message',
  version: 1,
  seq: 0,
  occurredAt: s.sentAt,
  source: 'twitch',
  payload: s.payload,
  deleted: s.deleted,
})

/** Puts stored chat from before this page opened (oldest first) ahead of what the live log already has. */
export function withHistory(log: ChatLog, stored: Schemas['StoredChatMessage'][]): ChatLog {
  const known = new Set(log.messages.map((m) => m.id))
  const older = stored.filter((s) => !known.has(s.id)).map(fromStored)
  const updates = new Map(stored.map((m) => [m.id, m]))
  const current = log.messages.map((m) => {
    const row = updates.get(m.id)
    if (row?.deleted && !m.deleted) return fromStored(row)
    if (row?.payload.firstChat && !m.payload.firstChat) return { ...m, payload: { ...m.payload, firstChat: true } }
    return m
  })
  if (!older.length && current.every((m, i) => m === log.messages[i])) return log
  const messages = [...older, ...current]
    .sort((a, b) => Date.parse(a.occurredAt) - Date.parse(b.occurredAt))
    .slice(-(log.paused ? PAUSED_LIMIT : LOG_LIMIT))
  return { ...log, messages }
}

export const pause = (log: ChatLog): ChatLog => (log.paused ? log : { ...log, paused: true })
export const resume = (log: ChatLog): ChatLog => ({ messages: log.messages.slice(-LOG_LIMIT), paused: false, unseen: 0 })

/** Refresh persisted history on each connection. Replay moderation received while the read was pending. */
export function useChatHistory() {
  const log = shallowRef(EMPTY_LOG)
  const historyFailed = shallowRef(false)
  let pending: ChatUpdate[] | undefined
  let retry: ReturnType<typeof setTimeout> | undefined
  async function read(changes: ChatUpdate[], delay: number) {
    try {
      const rows = await api(client.GET('/api/admin/chat/messages'))
      if (pending !== changes) return
      log.value = changes.reduce(applyChat, withHistory(log.value, rows))
      pending = undefined
      historyFailed.value = false
    } catch {
      if (pending !== changes) return
      historyFailed.value = true
      retry = setTimeout(() => void read(changes, Math.min(delay * 2, 30_000)), delay)
    }
  }
  function refresh() {
    clearTimeout(retry)
    pending = []
    void read(pending, 1000)
  }
  onScopeDispose(() => {
    clearTimeout(retry)
    pending = undefined
  })
  refresh()
  let connected = false
  function accept(msg: Incoming) {
    if (msg.type === 'snapshot') {
      if (connected) {
        log.value = { ...log.value, messages: [], unseen: 0 }
        refresh()
      }
      connected = true
      pending?.push(...(msg.payload['chat.removed'] ?? []))
    }
    if (msg.type === 'chat.removed' || msg.type === 'chat.first') pending?.push(msg)
    log.value = applyChat(log.value, msg)
  }
  return { log, accept, historyFailed }
}

export type ChatUpdate = Extract<Incoming, { type: 'chat.removed' | 'chat.first' }>
