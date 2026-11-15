<template>
  <section class="preview-section" aria-labelledby="preview-heading">
    <h2 id="preview-heading">{{ t('preview.heading') }}</h2>
    <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>
    <div class="options" role="radiogroup" aria-labelledby="preview-heading">
      <!-- Picking applies at once; clicks on a preview reach the radio because the iframe ignores the pointer. -->
      <label v-for="id in BUILT_IN" :key="id" class="option">
        <span class="choice">
          <input
            type="radio"
            name="chat-template"
            :value="id"
            :checked="selected === id"
            @change="choose(id)"
          />
          {{ t(`preview.templates.${id}`) }}
        </span>
        <!-- Templates never render in the admin window. Each preview is the chat overlay in a sandbox without
             allow-same-origin: its origin is `null`, so it gets no IPC and no token, and data arrives by postMessage. -->
        <iframe
          ref="frames"
          class="preview"
          sandbox="allow-scripts"
          tabindex="-1"
          :src="`${PREVIEW_URL}&template=${id}`"
          :title="t(`preview.templates.${id}`)"
          @load="sendSnapshot($event.target as HTMLIFrameElement)"
        ></iframe>
      </label>
    </div>
  </section>
</template>

<script setup lang="ts">
import { useToast } from 'primevue/usetoast'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast, useLiveEvents } from './home'

type BuiltIn = Exclude<Schemas['ChatTemplate'], 'custom'>

const PREVIEW_URL = '/overlay/chat/?preview'
// Keys of a full record, so a template added to the Host's list fails the typecheck until it is listed here.
const BUILT_IN = Object.keys({ default: 0, compact: 0, bubble: 0 } satisfies Record<BuiltIn, 0>) as BuiltIn[]

const { t } = useI18n()
const toast = useToast()
const frames = ref<HTMLIFrameElement[]>([])
const selected = ref<Schemas['ChatTemplate']>()
const templateError = ref<string | null>(null)

let chat: ChatState = { messages: [], lastSeq: 0 }
let choosing = false
let next: BuiltIn | undefined

// The iframes' origin is `null`, which no targetOrigin can name; what goes in is public chat data only.
const post = (msg: Incoming, frame?: HTMLIFrameElement) =>
  (frame ? [frame] : frames.value).forEach((f) => f.contentWindow?.postMessage(msg, '*'))

function sample(n: number, userName: string, color: string | null, text: string): ChatEvent {
  return {
    id: `sample-${n}`,
    type: 'chat.message',
    version: 1,
    seq: 0,
    occurredAt: new Date(0).toISOString(),
    source: 'simulation',
    payload: { userId: `sample-${n}`, userLogin: userName.toLowerCase(), userName, color, text, fragments: [] },
  }
}

/** What a preview starts from: the recent chat, or sample messages while there is none. */
function sendSnapshot(frame?: HTMLIFrameElement) {
  const messages = chat.messages.length
    ? chat.messages
    : [
        sample(1, 'Viewer', '#1E90FF', t('preview.sample.first')),
        sample(2, 'Someone', null, t('preview.sample.second')),
        sample(3, 'Streamer', '#9146FF', t('preview.sample.third')),
      ]
  post({ type: 'snapshot', version: 1, seq: chat.lastSeq, payload: { 'chat.message': messages, 'obs.status': [] } }, frame)
}

useLiveEvents(2000, (msg) => {
  chat = apply(chat, msg)
  // Rebuilt rather than forwarded, so an empty chat still shows the sample messages.
  if (msg.type === 'snapshot') sendSnapshot()
  else post(msg)
  if (msg.type === 'template.changed') void refresh()
})

/** The selected template, and why the user template is not in use (the overlay then shows the default one). */
async function refresh() {
  try {
    const status = await api(client.GET('/api/admin/templates/chat'))
    // A switch still in flight wins over an older answer.
    if (!choosing) selected.value = status.selected
    templateError.value = status.error
  } catch {
    // The next template change tries again.
  }
}

/** Saves one pick at a time and then only the latest, so quick arrow-key moves cannot land out of order. */
async function choose(id: BuiltIn) {
  selected.value = id
  next = id
  if (choosing) return
  choosing = true
  try {
    while (next) {
      const body = { selected: next }
      next = undefined
      try {
        await api(client.PUT('/api/admin/templates/chat', { body }))
      } catch (e) {
        toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
      }
    }
  } finally {
    choosing = false
  }
  // Shows what the Host really has: the old template after a failure.
  await refresh()
}

onMounted(refresh)
</script>

<style scoped>
.preview-section {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.preview-section h2 {
  margin: 0;
}

.options {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(16rem, 100%), 1fr));
  gap: 1rem;
}

.option {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  cursor: pointer;
}

.choice {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.choice input {
  width: 1.25rem;
  height: 1.25rem;
  margin: 0;
  accent-color: var(--p-primary-color);
}

/* OBS browser sources are transparent; a plain dark box stands in for the scene. */
.preview {
  width: 100%;
  height: 14rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-950);
  pointer-events: none;
}

.option:has(input:checked) .preview {
  border-color: var(--p-primary-color);
  outline: 2px solid var(--p-primary-color);
}
</style>
