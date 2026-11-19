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
  | components['schemas']['StreamStatusEvent']
  | components['schemas']['TemplateChangedEvent']
  | components['schemas']['RedemptionChangedEvent']
  | components['schemas']['CheckinChangedEvent']
  | components['schemas']['CheckinStampedEvent']
  | components['schemas']['SoundPlayEvent']

/** The emergency stop (every overlay), or a skip of the item `stage`'s overlay is playing. */
export const stopsStage = (msg: Incoming, stage: components['schemas']['Stage']) =>
  msg.type === 'presentation.stopped' || (msg.type === 'presentation.skipped' && msg.payload.stage === stage)

export interface ChatState {
  messages: ChatEvent[]
  lastSeq: number
}

export const MAX_MESSAGES = 50

/** A snapshot replaces everything; later events apply once, in `seq` order. Other event types are ignored. */
export function apply(state: ChatState, msg: Incoming): ChatState {
  if (msg.type === 'snapshot') {
    return { messages: msg.payload['chat.message'].slice(-MAX_MESSAGES), lastSeq: msg.seq }
  }
  if (msg.type !== 'chat.message' || msg.seq <= state.lastSeq) return state
  return { messages: [...state.messages, msg].slice(-MAX_MESSAGES), lastSeq: msg.seq }
}
