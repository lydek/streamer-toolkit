import { describe, expect, it } from 'vitest'
import { type ChatEvent, MAX_MESSAGES, apply } from './chat'

const chat = (seq: number): ChatEvent => ({
  id: `c${seq}`,
  type: 'chat.message',
  version: 1,
  seq,
  occurredAt: '2026-11-13T10:00:00.000Z',
  source: 'twitch',
  payload: { userId: '7', userLogin: 'viewer', userName: 'Viewer', color: null, text: `msg ${seq}`, fragments: [], badges: [], highlighted: false },
})

describe('apply', () => {
  it('starts from the snapshot and skips events it already contains', () => {
    let state = apply({ messages: [], lastSeq: 0 }, { type: 'snapshot', version: 1, seq: 2, payload: { 'chat.removed': [], 'chat.message': [chat(1), chat(2)], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'chat.text_size': [] } })
    state = apply(state, chat(2))
    state = apply(state, chat(3))
    expect(state.messages.map((m) => m.seq)).toEqual([1, 2, 3])
    expect(state.lastSeq).toBe(3)
  })

  it('replaces everything on a new snapshot after a reconnect', () => {
    const before = apply({ messages: [chat(1)], lastSeq: 9 }, { type: 'snapshot', version: 1, seq: 0, payload: { 'chat.removed': [], 'chat.message': [], 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'screen.break': [], 'chat.text_size': [] } })
    expect(before).toEqual({ messages: [], lastSeq: 0 })
  })

  it('ignores other event types such as obs.status', () => {
    const state = { messages: [chat(1)], lastSeq: 1 }
    const status = apply(state, {
      id: 's2',
      type: 'obs.status',
      version: 1,
      seq: 2,
      occurredAt: '2026-11-13T10:00:00.000Z',
      source: 'obs',
      payload: { state: 'connected', reason: null },
    })
    expect(status).toBe(state)
  })

  it('keeps only the newest messages', () => {
    let state = { messages: [] as ChatEvent[], lastSeq: 0 }
    for (let seq = 1; seq <= MAX_MESSAGES + 5; seq++) state = apply(state, chat(seq))
    expect(state.messages).toHaveLength(MAX_MESSAGES)
    expect(state.messages[0].seq).toBe(6)
  })
})

describe('chat.removed', () => {
  const removed = (seq: number, payload: { messageId?: string; userId?: string }) =>
    ({ id: `r${seq}`, type: 'chat.removed', version: 1, seq, occurredAt: '', source: 'twitch', payload }) as const
  const other = (seq: number): ChatEvent => ({ ...chat(seq), payload: { ...chat(seq).payload, userId: '8' } })

  it('limits a channel clear to its broadcaster and time window', () => {
    const message = { ...chat(1), occurredAt: '2026-11-20T12:00:00Z', payload: { ...chat(1).payload, broadcasterId: '42' } }
    const state = { messages: [message], lastSeq: 1 }
    for (const [channel, since, left] of [
      ['43', '2026-11-19T12:00:00Z', 1],
      ['42', '2026-11-20T12:01:00Z', 1],
      ['42', '2026-11-20T12:00:00Z', 0],
    ] as const) {
      expect(apply(state, { ...removed(2, {}), payload: { broadcasterId: channel, since } }).messages).toHaveLength(left)
    }
  })

  it("drops one message, one viewer's messages, or everything", () => {
    const state = { messages: [chat(1), other(2), chat(3)], lastSeq: 3 }
    expect(apply(state, removed(4, { messageId: 'c3' })).messages.map((m) => m.seq)).toEqual([1, 2])
    expect(apply(state, removed(4, { userId: '7' })).messages.map((m) => m.seq)).toEqual([2])
    expect(apply(state, removed(4, {})).messages).toEqual([])
    expect(apply(state, removed(3, {})).messages, 'already covered by what came before').toHaveLength(3)
  })
})
