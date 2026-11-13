import { describe, expect, it } from 'vitest'
import { type ChatEvent, MAX_MESSAGES, apply } from './chat'

const chat = (seq: number): ChatEvent => ({
  id: `c${seq}`,
  type: 'chat.message',
  seq,
  source: 'twitch',
  payload: { userName: 'Viewer', color: null, text: `msg ${seq}`, fragments: [] },
})

describe('apply', () => {
  it('starts from the snapshot and skips events it already contains', () => {
    let state = apply({ messages: [], lastSeq: 0 }, { type: 'snapshot', seq: 2, payload: { 'chat.message': [chat(1), chat(2)] } })
    state = apply(state, chat(2))
    state = apply(state, chat(3))
    expect(state.messages.map((m) => m.seq)).toEqual([1, 2, 3])
    expect(state.lastSeq).toBe(3)
  })

  it('replaces everything on a new snapshot after a reconnect', () => {
    const before = apply({ messages: [chat(1)], lastSeq: 9 }, { type: 'snapshot', seq: 0, payload: {} })
    expect(before).toEqual({ messages: [], lastSeq: 0 })
  })

  it('keeps only the newest messages', () => {
    let state = { messages: [] as ChatEvent[], lastSeq: 0 }
    for (let seq = 1; seq <= MAX_MESSAGES + 5; seq++) state = apply(state, chat(seq))
    expect(state.messages).toHaveLength(MAX_MESSAGES)
    expect(state.messages[0].seq).toBe(6)
  })
})
