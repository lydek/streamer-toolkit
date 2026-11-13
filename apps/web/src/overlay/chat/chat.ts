export interface ChatEvent {
  id: string
  type: 'chat.message'
  seq: number
  source: 'twitch' | 'simulation'
  payload: {
    userName: string
    color: string | null
    text: string
    fragments: ({ type: 'text'; text: string } | { type: 'emote'; text: string; emoteId: string })[]
  }
}

export type Incoming = { type: 'snapshot'; seq: number; payload: { 'chat.message'?: ChatEvent[] } } | ChatEvent

export interface ChatState {
  messages: ChatEvent[]
  lastSeq: number
}

export const MAX_MESSAGES = 50

/** A snapshot replaces everything; later events apply once, in `seq` order. Other event types are ignored. */
export function apply(state: ChatState, msg: Incoming): ChatState {
  if (msg.type === 'snapshot') {
    return { messages: (msg.payload['chat.message'] ?? []).slice(-MAX_MESSAGES), lastSeq: msg.seq }
  }
  if (msg.type !== 'chat.message' || msg.seq <= state.lastSeq) return state
  return { messages: [...state.messages, msg].slice(-MAX_MESSAGES), lastSeq: msg.seq }
}
