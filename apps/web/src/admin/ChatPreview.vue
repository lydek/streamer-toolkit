<template>
  <div class="text-size">
    <label for="chat-text-size">{{ t('preview.textSize') }}</label>
    <Select
      :model-value="textSize"
      input-id="chat-text-size"
      :options="sizeOptions"
      option-label="label"
      option-value="value"
      @update:model-value="setTextSize"
    />
  </div>
  <TemplatePicker
    overlay="chat"
    :ids="BUILT_IN"
    :label="(id) => t(`preview.templates.${id}`)"
    :start="start"
    :on-live="track"
  />
</template>

<script setup lang="ts">
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast } from './home'
import TemplatePicker from './TemplatePicker.vue'

type BuiltIn = Exclude<Schemas['ChatTemplate'], 'custom'>

// Keys of a full record, so a template added to the Host's list fails the typecheck until it is listed here.
const BUILT_IN = Object.keys({ default: 0, compact: 0, bubble: 0 } satisfies Record<BuiltIn, 0>) as BuiltIn[]

const { t } = useI18n()
const toast = useToast()
let chat: ChatState = { messages: [], lastSeq: 0 }
// The overlay text size as the Host announces it; previews start from it too.
let sizeEvent: Schemas['ChatTextSizeEvent'] | undefined
const textSize = ref<number | null>(null)
function track(msg: Incoming) {
  chat = apply(chat, msg)
  if (msg.type === 'snapshot') sizeEvent = msg.payload['chat.text_size'].at(-1)
  if (msg.type === 'chat.text_size') sizeEvent = msg
  textSize.value = sizeEvent?.payload.textSize ?? null
}

const SIZES = [16, 18, 20, 24, 28, 32, 36, 40, 44, 48]
const sizeOptions = computed(() => [
  { value: null, label: t('preview.templateSize') },
  ...SIZES.map((n) => ({ value: n, label: `${n}px` })),
])
/** Saves the size; the Host announces it, which updates the overlays, the previews and this field. */
async function setTextSize(size: number | null) {
  try {
    await api(client.PUT('/api/admin/templates/chat/text-size', { body: { text_size: size } }))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
  }
}

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
  return [{ type: 'snapshot', version: 1, seq: chat.lastSeq, payload: { 'chat.removed': [], 'chat.message': messages, 'obs.status': [], 'obs.state': [], 'presentation.queue': [], 'stream.status': [], 'bgm.player': [], 'counter.values': [], 'counter.marathon': [], 'chat.text_size': sizeEvent ? [sizeEvent] : [] } }]
}
</script>

<style scoped>
.text-size {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
  margin-bottom: 1rem;
}
</style>
