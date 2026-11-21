<template>
  <div>
    <!-- Styled like the template's chat text, so it shows on the scenes the template is made for. -->
    <p v-if="status !== 'open'" class="status" role="status" :style="statusStyle">
      {{ t(`overlay.status.${status}`) }}
    </p>
    <!-- The user template is mounted here; messages are rendered by template.ts as plain text. -->
    <div ref="host"></div>
  </div>
</template>

<script setup lang="ts">
import { followAppearance } from '../appearance'
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type ChatEvent, type ChatState, type Incoming, apply, applyTextSize } from './chat'
import { loadTemplate, mount, stylesheetsLoaded, textStyle } from './template'

const RETRY_MS = 2000
const params = new URLSearchParams(location.search)
// `?preview`: the admin UI's template preview (see onPreviewMessage); `&template=<id>` previews that built-in
// template instead of the selected one.
const preview = params.has('preview')
const builtIn = preview ? params.get('template') : null
const TEMPLATE_BASE =
  builtIn && /^[a-z0-9-]{1,32}$/.test(builtIn)
    ? `${location.origin}/templates/built-in/chat/${builtIn}/`
    : `${location.origin}/templates/chat/`

const { t } = useI18n()
const status = ref<'connecting' | 'open' | 'closed'>('connecting')
const host = ref<HTMLElement>()
const statusStyle = ref<ReturnType<typeof textStyle>>()
let state: ChatState = { messages: [], lastSeq: 0 }
let render: ((messages: ChatEvent[]) => void) | undefined
let loads = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

/** Swaps in the current template and redraws the messages already shown. */
async function applyTemplate() {
  const load = ++loads
  const template = await loadTemplate(TEMPLATE_BASE, String(Date.now()))
  // A newer load started meanwhile, or the overlay is gone.
  if (load !== loads || !host.value) return
  // Mount hidden next to the current template and show it once its CSS is in, so edits do not flash unstyled.
  const next = document.createElement('div')
  next.hidden = true
  host.value.append(next)
  const renderNext = mount(next, template)
  renderNext(state.messages)
  await stylesheetsLoaded(next)
  if (load !== loads || !host.value) return next.remove()
  next.hidden = false
  // Not replaceChildren(next): re-inserting `next` would load its stylesheets again.
  while (host.value.firstChild !== next) host.value.firstChild!.remove()
  render = renderNext
  render(state.messages)
  // Only now: the previous template's stylesheets are gone and no longer apply.
  statusStyle.value = textStyle(template)
}

// Theme and text size from 畫面工具 (built-in look only).
const appearance = followAppearance()

function receive(msg: Incoming) {
  appearance(msg)
  // On every snapshot too: the template may have changed while disconnected.
  if (msg.type === 'snapshot' || (msg.type === 'template.changed' && msg.payload.overlay === 'chat')) {
    void applyTemplate()
  }
  if (msg.type === 'snapshot') applyTextSize(document.documentElement, msg.payload['chat.text_size']?.at(-1)?.payload.textSize)
  if (msg.type === 'chat.text_size') applyTextSize(document.documentElement, msg.payload.textSize)
  state = apply(state, msg)
  render?.(state.messages)
}

function connect() {
  status.value = 'connecting'
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=chat`)
  ws.onopen = () => (status.value = 'open')
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
    status.value = 'closed'
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

// `?preview`: the admin UI's template preview, in a sandbox="allow-scripts" iframe. Its origin is `null`, so it
// cannot reach the Host's WebSocket; the admin window posts the same messages instead (docs/architecture.md §5).
// location.origin is still the page URL's origin, which is the admin window's.
function onPreviewMessage(e: MessageEvent) {
  if (e.source === window.parent && e.origin === location.origin) receive(e.data as Incoming)
}

if (preview) {
  status.value = 'open'
  window.addEventListener('message', onPreviewMessage)
} else {
  connect()
  // Not only on the first snapshot: the status shows (styled by the template) until a connection opens.
  void applyTemplate()
}
onBeforeUnmount(() => {
  stopped = true
  clearTimeout(retry)
  ws?.close()
  window.removeEventListener('message', onPreviewMessage)
})
</script>
