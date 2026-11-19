<template>
  <TemplatePicker
    overlay="chat"
    :ids="BUILT_IN"
    :label="(id) => t(`preview.templates.${id}`)"
    :start="start"
    :on-live="track"
  />
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from '../overlay/chat/chat'
import type { Schemas } from './api'
import TemplatePicker from './TemplatePicker.vue'

type BuiltIn = Exclude<Schemas['ChatTemplate'], 'custom'>

// Keys of a full record, so a template added to the Host's list fails the typecheck until it is listed here.
const BUILT_IN = Object.keys({ default: 0, compact: 0, bubble: 0 } satisfies Record<BuiltIn, 0>) as BuiltIn[]

const { t } = useI18n()
let chat: ChatState = { messages: [], lastSeq: 0 }
const track = (msg: Incoming) => (chat = apply(chat, msg))

function sample(n: number, userName: string, color: string | null, text: string): ChatEvent {
  return {
    id: `sample-${n}`,
    type: 'chat.message',
    version: 1,
    seq: 0,
    occurredAt: new Date(0).toISOString(),
    source: 'simulation',
    payload: { userId: `sample-${n}`, userLogin: userName.toLowerCase(), userName, color, text, fragments: [], badges: [], highlighted: false },
  }
}

/** What a preview starts from: the recent chat, or sample messages while there is none. */
function start(): Incoming[] {
  const messages = chat.messages.length
    ? chat.messages
    : [
        sample(1, 'Viewer', '#1E90FF', t('preview.sample.first')),
        sample(2, 'Someone', null, t('preview.sample.second')),
        sample(3, 'Streamer', '#9146FF', t('preview.sample.third')),
      ]
  return [{ type: 'snapshot', version: 1, seq: chat.lastSeq, payload: { 'chat.message': messages, 'obs.status': [], 'obs.state': [], 'presentation.queue': [] } }]
}
</script>
