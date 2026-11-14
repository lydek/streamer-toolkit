<template>
  <div>
    <p v-if="status !== 'open'" class="status" role="status">{{ t(`overlay.status.${status}`) }}</p>
    <!-- The user template is mounted here; messages are rendered by template.ts as plain text. -->
    <div ref="host"></div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type ChatEvent, type ChatState, type Incoming, apply } from './chat'
import { loadTemplate, mount, stylesheetsLoaded } from './template'

const RETRY_MS = 2000
const TEMPLATE_BASE = `${location.origin}/templates/chat/`

const { t } = useI18n()
const status = ref<'connecting' | 'open' | 'closed'>('connecting')
const host = ref<HTMLElement>()
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
  host.value.replaceChildren(next)
  render = renderNext
  render(state.messages)
}

function connect() {
  status.value = 'connecting'
  ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onopen = () => (status.value = 'open')
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    // On every snapshot too: the template may have changed while disconnected.
    if (msg.type === 'snapshot' || msg.type === 'template.changed') void applyTemplate()
    state = apply(state, msg)
    render?.(state.messages)
  }
  ws.onclose = () => {
    status.value = 'closed'
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

connect()
onBeforeUnmount(() => {
  stopped = true
  clearTimeout(retry)
  ws?.close()
})
</script>
