<template>
  <section class="preview-section" aria-labelledby="preview-heading">
    <h2 id="preview-heading">{{ t('preview.heading') }}</h2>
    <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>
    <!-- Templates never render in the admin window. The preview is the chat overlay in a sandbox without
         allow-same-origin: its origin is `null`, so it gets no IPC and no token, and data arrives by postMessage. -->
    <iframe
      ref="frame"
      class="preview"
      sandbox="allow-scripts"
      :src="PREVIEW_URL"
      :title="t('preview.heading')"
      @load="sendSnapshot"
    ></iframe>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from '../overlay/chat/chat'
import { api, client } from './api'
import { useLiveEvents } from './home'

const PREVIEW_URL = '/overlay/chat/?preview'

const { t } = useI18n()
const frame = ref<HTMLIFrameElement>()
const templateError = ref<string | null>(null)

let chat: ChatState = { messages: [], lastSeq: 0 }

// The iframe's origin is `null`, which no targetOrigin can name; what goes in is public chat data only.
const post = (msg: Incoming) => frame.value?.contentWindow?.postMessage(msg, '*')

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

/** What the preview starts from: the recent chat, or sample messages while there is none. */
function sendSnapshot() {
  const messages = chat.messages.length
    ? chat.messages
    : [
        sample(1, 'Viewer', '#1E90FF', t('preview.sample.first')),
        sample(2, 'Someone', null, t('preview.sample.second')),
        sample(3, 'Streamer', '#9146FF', t('preview.sample.third')),
      ]
  post({ type: 'snapshot', version: 1, seq: chat.lastSeq, payload: { 'chat.message': messages, 'obs.status': [] } })
}

useLiveEvents(2000, (msg) => {
  chat = apply(chat, msg)
  // Rebuilt rather than forwarded, so an empty chat still shows the sample messages.
  if (msg.type === 'snapshot') sendSnapshot()
  else post(msg)
  if (msg.type === 'template.changed') void refreshError()
})

/** Why the user template is not in use (the overlay then shows the default one), or null when it is. */
async function refreshError() {
  try {
    templateError.value = (await api(client.GET('/api/admin/templates/chat'))).error
  } catch {
    // The next template change tries again.
  }
}

onMounted(refreshError)
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

/* OBS browser sources are transparent; a plain dark box stands in for the scene. */
.preview {
  width: 100%;
  height: 20rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-950);
}
</style>
