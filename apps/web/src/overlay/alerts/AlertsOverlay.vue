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
import { playSound, stopSounds } from '../sound'
import { type AlertTemplate, fillAlert, loadAlertTemplate } from './template'

const RETRY_MS = 2000
// A preview replays its alert after this pause.
const REPLAY_MS = 600
const params = new URLSearchParams(location.search)
// `?preview`: the admin UI's template preview (see onPreviewMessage); `&template=<id>` previews that built-in
// template instead of the selected one.
const preview = params.has('preview')
const builtIn = preview ? params.get('template') : null
const TEMPLATE_BASE =
  builtIn && /^[a-z0-9-]{1,32}$/.test(builtIn)
    ? `${location.origin}/templates/built-in/alerts/${builtIn}/`
    : `${location.origin}/templates/alerts/`

const host = ref<HTMLElement>()
let template: AlertTemplate | undefined
let ready = Promise.resolve()
// Bumped by an emergency stop: an alert still waiting for its template must not show after it.
let generation = 0
let loads = 0
let lastSeq = 0
let hide: ReturnType<typeof setTimeout> | undefined
/** The alert on screen, kept across a template change. */
let current: { p: AlertPayload; until: number } | undefined
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
  // An alert playing when the template changed carries on in the new one for the rest of its time.
  if (current && current.until > Date.now()) show(current.p, current.until - Date.now())
}

const reload = () => (ready = applyTemplate())

/**
 * Shows one alert for its duration with the sound. The Host paces them, so a new one only replaces a stale one.
 * A preview plays it silently, over and over.
 */
async function play(p: AlertPayload) {
  const mine = generation
  // The newest load: a template change during a load supersedes it.
  for (let r: Promise<void> | undefined; r !== ready; ) await (r = ready)
  if (!template || mine !== generation) return
  show(p, p.durationMs)
  if (preview) return
  const { sound } = p
  if (sound.kind === 'built_in') playSound(chimeUrl, 100)
  else if (sound.kind === 'file') playSound(sound.url, sound.volume)
}

/** Draws `p` in the current template for `ms`. */
function show(p: AlertPayload, ms: number) {
  if (!template) return
  clearTimeout(hide)
  const { stage } = template
  stage.replaceChildren(fillAlert(template.item, { ...p, durationMs: ms }))
  current = { p, until: Date.now() + ms }
  hide = setTimeout(() => {
    stage.replaceChildren()
    current = undefined
    if (preview) hide = setTimeout(() => void play(p), REPLAY_MS)
  }, ms)
}

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    // The template may have changed while disconnected. Alerts are never in snapshots, so none replays.
    lastSeq = msg.seq
    reload()
  } else if (msg.type === 'template.changed' && msg.payload.overlay === 'alerts') {
    reload()
  } else if (msg.type === 'presentation.stopped' && (preview || msg.seq > lastSeq)) {
    // Emergency stop: the alert and its sound go away at once.
    lastSeq = Math.max(lastSeq, msg.seq)
    generation++
    clearTimeout(hide)
    template?.stage.replaceChildren()
    current = undefined
    stopSounds()
  } else if (msg.type === 'alert' && (preview || msg.seq > lastSeq)) {
    // The admin window posts a preview's sample and live alerts in order, without a fitting `seq`.
    lastSeq = Math.max(lastSeq, msg.seq)
    void play(msg.payload)
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=alerts`)
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

// `?preview`: in a sandbox="allow-scripts" iframe its origin is `null`, so it cannot reach the Host's WebSocket;
// the admin window posts the same messages instead (docs/architecture.md §5). location.origin is still the page
// URL's origin, which is the admin window's.
function onPreviewMessage(e: MessageEvent) {
  if (e.source === window.parent && e.origin === location.origin) receive(e.data as Incoming)
}

if (preview) window.addEventListener('message', onPreviewMessage)
else connect()
reload()
onBeforeUnmount(() => {
  stopped = true
  clearTimeout(retry)
  clearTimeout(hide)
  ws?.close()
  window.removeEventListener('message', onPreviewMessage)
})
</script>
