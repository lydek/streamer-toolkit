import type { Schemas } from './api'
import type { LoggedChat } from './chatLog'

/** What the chat log page filters by; message filters show messages only, event filters events only. */
export const LOG_FILTERS = ['all', 'mentions', 'commands', 'mods', 'subscribers', 'highlights', 'redemptions', 'alerts'] as const
export type LogFilter = (typeof LOG_FILTERS)[number]

// shortcut: a fixed list of common chat bots; a broadcaster's own list waits for viewer roles (phase 7).
export const BOTS = new Set(['nightbot', 'streamelements', 'streamlabs', 'moobot', 'fossabot', 'sery_bot'])

export type LogEvent = Schemas['LogEvent']

/** One line of the chat log, placed by its time. */
export type Entry = { kind: 'message'; at: string; message: LoggedChat } | { kind: 'event'; at: string; event: LogEvent }

/** Messages and events in time order. */
export function merge(messages: LoggedChat[], events: LogEvent[]): Entry[] {
  const entries: Entry[] = [
    ...messages.map((m): Entry => ({ kind: 'message', at: m.occurredAt, message: m })),
    ...events.map((e): Entry => ({ kind: 'event', at: e.at, event: e })),
  ]
  return entries.sort((a, b) => Date.parse(a.at) - Date.parse(b.at))
}

const ALERTS = new Set<LogEvent['kind']>(['follow', 'subscribe', 'resubscribe', 'gift', 'raid'])

function messageMatches(m: LoggedChat, filter: LogFilter, mention: RegExp | undefined): boolean {
  const p = m.payload
  switch (filter) {
    case 'all':
      return true
    case 'mentions':
      return !!mention && mention.test(p.text)
    case 'commands':
      return p.text.trimStart().startsWith('!')
    case 'mods':
      return p.badges.some((b) => b === 'broadcaster' || b === 'moderator' || b === 'vip')
    case 'subscribers':
      return p.badges.includes('subscriber')
    case 'highlights':
      return p.highlighted || (p.bits ?? 0) > 0
    default:
      return false
  }
}

function eventMatches(e: LogEvent, filter: LogFilter): boolean {
  switch (filter) {
    case 'all':
      return true
    case 'redemptions':
      return e.kind === 'redemption'
    case 'alerts':
      return ALERTS.has(e.kind)
    case 'highlights':
      return e.kind === 'cheer'
    default:
      return false
  }
}

/** The entries `filter`, the search text and 隱藏機器人 leave; `login` is the broadcaster's, for 提到我. */
export function filterEntries(entries: Entry[], filter: LogFilter, query: string, hideBots: boolean, login?: string): Entry[] {
  const q = query.trim().toLowerCase()
  const mention = login ? new RegExp(`@${login}\\b`, 'i') : undefined
  return entries.filter((e) => {
    if (e.kind === 'message') {
      const p = e.message.payload
      if (hideBots && BOTS.has(p.userLogin.toLowerCase())) return false
      if (q && !`${p.userName} ${p.text}`.toLowerCase().includes(q)) return false
      return messageMatches(e.message, filter, mention)
    }
    if (q && !`${e.event.userName ?? ''} ${e.event.reward ?? ''} ${e.event.message ?? ''}`.toLowerCase().includes(q)) return false
    return eventMatches(e.event, filter)
  })
}

const two = (n: number) => String(n).padStart(2, '0')
/** `11/16` in local time. */
export const dayText = (at: string) => {
  const d = new Date(at)
  return `${d.getMonth() + 1}/${d.getDate()}`
}
/** `21:30` in local time. */
export const clockText = (at: string) => {
  const d = new Date(at)
  return `${two(d.getHours())}:${two(d.getMinutes())}`
}

/** An alert the live view heard (`alert.received`) as a log event. */
export const fromAlert = (msg: Schemas['AlertReceivedEvent']): LogEvent => ({
  at: msg.occurredAt,
  kind: msg.payload.kind,
  userName: msg.payload.userName,
  months: msg.payload.months,
  count: msg.payload.count,
  bits: msg.payload.bits,
  viewers: msg.payload.viewers,
  message: msg.payload.message,
})
