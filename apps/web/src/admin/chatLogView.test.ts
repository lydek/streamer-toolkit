import { describe, expect, it } from 'vitest'
import type { LoggedChat } from './chatLog'
import { type Entry, type LogEvent, filterEntries, merge } from './chatLogView'

// The Host's list (crates/host/src/leaderboard.rs), in part.
const BOTS: ReadonlySet<string> = new Set(['nightbot', 'streamelements'])
const NONE: ReadonlySet<string> = new Set()

const msg = (id: string, at: string, over: Partial<LoggedChat['payload']> = {}): LoggedChat => ({
  id,
  type: 'chat.message',
  version: 1,
  seq: 0,
  occurredAt: at,
  source: 'twitch',
  payload: { userId: '7', userLogin: 'fan', userName: 'Fan', color: null, text: 'hi', fragments: [], badges: [], highlighted: false, ...over },
})
const event = (kind: LogEvent['kind'], at: string, userName = 'Kai'): LogEvent => ({ at, kind, userName })
const ids = (entries: Entry[]) => entries.map((e) => (e.kind === 'message' ? e.message.id : e.event.kind))

describe('chat log view', () => {
  const entries = merge(
    [
      msg('plain', '2026-11-19T10:00:00Z'),
      msg('mention', '2026-11-19T10:01:00Z', { text: 'hey @Me sing' }),
      msg('command', '2026-11-19T10:02:00Z', { text: '!打卡' }),
      msg('first', '2026-11-19T10:02:10Z', { firstChat: true }),
      msg('mod', '2026-11-19T10:03:00Z', { badges: ['moderator'] }),
      msg('sub', '2026-11-19T10:04:00Z', { badges: ['subscriber'] }),
      msg('cheer', '2026-11-19T10:05:00Z', { bits: 100 }),
      msg('bot', '2026-11-19T10:06:00Z', { userLogin: 'Nightbot', userName: 'Nightbot' }),
    ],
    [event('redemption', '2026-11-19T10:00:30Z'), event('raid', '2026-11-19T10:02:30Z'), event('cheer', '2026-11-19T10:05:30Z')],
  )

  it('places events among the messages by time', () => {
    expect(ids(entries).slice(0, 4)).toEqual(['plain', 'redemption', 'mention', 'command'])
  })

  it('each filter keeps its own kind of line', () => {
    const only = (f: Parameters<typeof filterEntries>[1]) => ids(filterEntries(entries, f, '', NONE, 'me'))
    expect(only('mentions')).toEqual(['mention'])
    expect(only('commands')).toEqual(['command'])
    expect(only('first')).toEqual(['first'])
    expect(only('mods')).toEqual(['mod'])
    expect(only('subscribers')).toEqual(['sub'])
    expect(only('highlights')).toEqual(['cheer', 'cheer'])
    expect(only('redemptions')).toEqual(['redemption'])
    expect(only('alerts')).toEqual(['raid'])
    expect(only('all')).toHaveLength(11)
  })

  it('search and hiding bots narrow any filter', () => {
    expect(ids(filterEntries(entries, 'all', 'SING', NONE))).toEqual(['mention'])
    expect(ids(filterEntries(entries, 'all', '', BOTS))).not.toContain('bot')
    expect(ids(filterEntries(entries, 'all', 'kai', BOTS))).toEqual(['redemption', 'raid', 'cheer'])
    // No login, no mentions.
    expect(ids(filterEntries(entries, 'mentions', '', NONE))).toEqual([])
  })
})
