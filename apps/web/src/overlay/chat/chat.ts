import type { components } from '../../api/schema'

export type ChatEvent = components['schemas']['ChatMessageEvent']
export type Incoming =
  | components['schemas']['Snapshot']
  | ChatEvent
  | components['schemas']['AlertEvent']
  | components['schemas']['AlertReceivedEvent']
  | components['schemas']['ObsStatusEvent']
  | components['schemas']['ObsStateEvent']
  | components['schemas']['PresentationQueueEvent']
  | components['schemas']['PresentationStoppedEvent']
  | components['schemas']['PresentationSkippedEvent']
  | components['schemas']['ChatRemovedEvent']
  | components['schemas']['ChatFirstEvent']
  | components['schemas']['StreamStatusEvent']
  | components['schemas']['TemplateChangedEvent']
  | components['schemas']['RedemptionChangedEvent']
  | components['schemas']['CheckinChangedEvent']
  | components['schemas']['CheckinStampedEvent']
  | components['schemas']['CheckinHiddenEvent']
  | components['schemas']['SoundPlayEvent']
  | components['schemas']['SoundStoppedEvent']
  | components['schemas']['BgmPlayerEvent']
  | components['schemas']['CounterValuesEvent']
  | components['schemas']['MarathonEvent']
  | components['schemas']['BreakEvent']
  | components['schemas']['WarningsEvent']
  | components['schemas']['AppearanceEvent']
  | components['schemas']['MediaPlayEvent']
  | components['schemas']['LeaderboardEvent']
  | components['schemas']['CounterTestEvent']
  | components['schemas']['MarathonTestEvent']
  | components['schemas']['BgmTestEvent']
  | components['schemas']['ChatTextSizeEvent']

/**
 * Sets the chat overlay's text size, which the templates read as the CSS variable `--chat-text-size`; `null` keeps
 * each template's own size.
 */
export function applyTextSize(root: HTMLElement, px: number | null | undefined) {
  if (px) root.style.setProperty('--chat-text-size', `${px}px`)
  else root.style.removeProperty('--chat-text-size')
}

/** The emergency stop (every overlay), or a skip of the item `stage`'s overlay is playing. */
export const stopsStage = (msg: Incoming, stage: components['schemas']['Stage']) =>
  msg.type === 'presentation.stopped' || (msg.type === 'presentation.skipped' && msg.payload.stage === stage)

export interface ChatState {
  messages: ChatEvent[]
  lastSeq: number
}

export const MAX_MESSAGES = 50

/** Whether a moderator's `chat.removed` takes `m` down: one message, every message of one viewer, or all. */
export function removes(removal: components['schemas']['ChatRemovedPayload'], m: ChatEvent): boolean {
  if (removal.messageId !== undefined) return m.id === removal.messageId
  if (removal.broadcasterId && removal.broadcasterId !== m.payload.broadcasterId) return false
  if (removal.since && Date.parse(m.occurredAt) < Date.parse(removal.since)) return false
  if (removal.userId !== undefined) return m.payload.userId === removal.userId
  return true
}

/** A snapshot replaces everything; later events apply once, in `seq` order. Other event types are ignored. */
export function apply(state: ChatState, msg: Incoming): ChatState {
  if (msg.type === 'snapshot') {
    return { messages: msg.payload['chat.message'].slice(-MAX_MESSAGES), lastSeq: msg.seq }
  }
  if (msg.seq <= state.lastSeq) return state
  if (msg.type === 'chat.removed') {
    return { messages: state.messages.filter((m) => !removes(msg.payload, m)), lastSeq: msg.seq }
  }
  if (msg.type !== 'chat.message') return state
  return { messages: [...state.messages, msg].slice(-MAX_MESSAGES), lastSeq: msg.seq }
}
