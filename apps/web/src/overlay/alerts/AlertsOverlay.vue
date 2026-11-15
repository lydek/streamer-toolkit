<template>
  <!-- The template is mounted here; alerts are rendered by template.ts as plain text. -->
  <div ref="host"></div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import type { Incoming } from '../chat/chat'
import type { AlertPayload } from './alerts'
import chimeUrl from './chime.wav'
import { stylesheetsLoaded } from '../chat/template'
import { type AlertTemplate, fillAlert, loadAlertTemplate } from './template'

const RETRY_MS = 2000
const TEMPLATE_BASE = `${location.origin}/templates/alerts/`

const host = ref<HTMLElement>()
let template: AlertTemplate | undefined
let ready = Promise.resolve()
let loads = 0
let lastSeq = 0
let hide: ReturnType<typeof setTimeout> | undefined
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

/** Swaps in the current template once its CSS is in, so it never flashes unstyled. */
async function applyTemplate() {
  const load = ++loads
  const next = await loadAlertTemplate(TEMPLATE_BASE, String(Date.now()))
  if (load !== loads || !host.value) return
  const box = document.createElement('div')
  box.hidden = true
  box.append(...next.nodes)
  host.value.append(box)
  await stylesheetsLoaded(box)
  if (load !== loads || !host.value) return box.remove()
  box.hidden = false
  while (host.value.firstChild !== box) host.value.firstChild!.remove()
  template = next
}

const reload = () => (ready = applyTemplate())

/** Shows one alert for its duration with the sound. The Host paces them, so a new one only replaces a stale one. */
async function play(p: AlertPayload) {
  await ready
  if (!template) return
  clearTimeout(hide)
  const { stage } = template
  stage.replaceChildren(fillAlert(template.item, p))
  hide = setTimeout(() => stage.replaceChildren(), p.durationMs)
  // OBS lets browser sources play sound without a click; elsewhere a refused play() only means silence.
  new Audio(chimeUrl).play()?.catch(() => {})
}

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    // The template may have changed while disconnected. Alerts are never in snapshots, so none replays.
    lastSeq = msg.seq
    reload()
  } else if (msg.type === 'alert' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    void play(msg.payload)
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

connect()
reload()
onBeforeUnmount(() => {
  stopped = true
  clearTimeout(retry)
  clearTimeout(hide)
  ws?.close()
})
</script>
