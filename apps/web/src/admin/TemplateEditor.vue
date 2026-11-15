<template>
  <div class="editor">
    <p v-if="status" :role="status.error ? 'alert' : 'status'">{{ status.text }}</p>
    <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>

    <p v-if="!source && !loadFailed">{{ t('app.loading') }}</p>
    <form v-else-if="source" class="files" @submit.prevent="save">
      <label for="template-html"><code>template.html</code></label>
      <Textarea id="template-html" v-model="html" rows="14" spellcheck="false" autocomplete="off" />
      <label for="template-css"><code>style.css</code></label>
      <Textarea id="template-css" v-model="css" rows="10" spellcheck="false" autocomplete="off" />
      <div class="buttons">
        <Button type="submit" :label="t('editor.save')" icon="pi pi-save" :disabled="busy || !dirty" />
        <Button v-if="conflict" :label="t('editor.reload')" icon="pi pi-refresh" outlined :disabled="busy" @click="load" />
      </div>
    </form>
    <Button v-else :label="t('editor.reload')" icon="pi pi-refresh" outlined @click="load" />

    <section aria-labelledby="preview-heading">
      <h2 id="preview-heading">{{ t('editor.preview') }}</h2>
      <!-- Templates never render in the admin window. The preview is the chat overlay in a sandbox without
           allow-same-origin: its origin is `null`, so it gets no IPC and no token, and data arrives by postMessage. -->
      <iframe
        ref="frame"
        class="preview"
        sandbox="allow-scripts"
        :src="PREVIEW_URL"
        :title="t('editor.preview')"
        @load="sendSnapshot"
      ></iframe>
    </section>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Textarea from 'primevue/textarea'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { onBeforeRouteLeave } from 'vue-router'
import { errorMessage } from '../i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import { useLiveEvents } from './home'

const PREVIEW_URL = '/overlay/chat/?preview'

const { t } = useI18n()
const frame = ref<HTMLIFrameElement>()
const source = ref<Schemas['TemplateSource']>()
const html = ref('')
const css = ref('')
const busy = ref(false)
const loadFailed = ref(false)
const conflict = ref(false)
const templateError = ref<string | null>(null)
const status = ref<{ text: string; error: boolean }>()
// A textarea turns CRLF into LF; files saved with CRLF get it back on save, so one edit does not rewrite every line.
const lf = (s: string) => s.replaceAll('\r\n', '\n')
const like = (text: string, original: string) => (original.includes('\r\n') ? text.replaceAll('\n', '\r\n') : text)
const dirty = computed(
  () => !!source.value && (html.value !== lf(source.value.html) || css.value !== lf(source.value.css)),
)

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
        sample(1, 'Viewer', '#1E90FF', t('editor.sample.first')),
        sample(2, 'Someone', null, t('editor.sample.second')),
        sample(3, 'Streamer', '#9146FF', t('editor.sample.third')),
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

async function refreshError() {
  try {
    templateError.value = (await api(client.GET('/api/admin/templates/chat'))).error
  } catch {
    // The next change or save tries again.
  }
}

async function run(action: () => Promise<void>) {
  busy.value = true
  status.value = undefined
  try {
    await action()
  } catch (e) {
    const code = e instanceof ApiError ? e.code : String(e)
    conflict.value = code === 'template.conflict'
    status.value = { text: errorMessage(code), error: true }
  } finally {
    busy.value = false
  }
}

function show(s: Schemas['TemplateSource']) {
  source.value = s
  html.value = lf(s.html)
  css.value = lf(s.css)
}

const load = () =>
  run(async () => {
    loadFailed.value = false
    conflict.value = false
    try {
      show(await api(client.GET('/api/admin/templates/chat/source')))
    } catch (e) {
      loadFailed.value = !source.value
      throw e
    }
    await refreshError()
  })

const save = () =>
  run(async () => {
    const base = source.value!
    // Only `source` moves on: anything typed while the save was in flight stays in the fields and stays dirty.
    source.value = await api(
      client.PUT('/api/admin/templates/chat/source', {
        body: { html: like(html.value, base.html), css: like(css.value, base.css), revision: base.revision },
      }),
    )
    status.value = { text: t('editor.saved'), error: false }
  })

onMounted(load)

// Saving is explicit (a save can conflict), so leaving with unsaved edits asks instead of losing them silently.
onBeforeRouteLeave(() => !dirty.value || window.confirm(t('editor.leave')))
</script>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.files {
  align-items: stretch;
}

.files textarea {
  font-family: Consolas, 'Cascadia Mono', monospace;
  font-size: 0.875rem;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

/* OBS browser sources are transparent; a checkerboard-free dark box stands in for the scene. */
.preview {
  width: 100%;
  height: 20rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-950);
}
</style>
