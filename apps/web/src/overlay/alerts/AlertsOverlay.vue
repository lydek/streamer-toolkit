<template>
  <!-- The template is mounted here; alerts are rendered by template.ts as plain text. -->
  <div ref="host"></div>
  <!-- A reward's video or GIF (screen tools), paced by the presentation queue like the alerts. -->
  <div v-if="media" class="media" :class="`at-${media.position}`" data-media>
    <img v-if="media.format === 'gif'" :src="media.url" alt="" />
    <video v-else :src="media.url" autoplay playsinline :volume="media.volume / 100" :muted="media.volume === 0"></video>
  </div>
</template>

<script setup lang="ts">
import { followAppearance } from '../appearance'
import { onBeforeUnmount, ref, shallowRef } from 'vue'
import { type Incoming, stopsStage } from '../chat/chat'
import type { AlertPayload } from './alerts'
import type { components } from '../../api/schema'

type MediaPlay = components['schemas']['MediaPlayPayload']
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
const media = shallowRef<MediaPlay>()
let hideMedia: ReturnType<typeof setTimeout> | undefined
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
  if (sound.kind === 'built_in') playSound(chimeUrl, sound.volume)
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

// Theme and text size from 畫面工具 (built-in look only).
const appearance = followAppearance()

function receive(msg: Incoming) {
  appearance(msg)
  if (msg.type === 'snapshot') {
    // The template may have changed while disconnected. Alerts are never in snapshots, so none replays.
    lastSeq = msg.seq
    reload()
  } else if (msg.type === 'template.changed' && msg.payload.overlay === 'alerts') {
    reload()
  } else if (stopsStage(msg, 'alerts') && (preview || msg.seq > lastSeq)) {
    // Emergency stop or skip: the alert and its sound go away at once.
    lastSeq = Math.max(lastSeq, msg.seq)
    generation++
    clearTimeout(hide)
    template?.stage.replaceChildren()
    current = undefined
    stopSounds()
    clearTimeout(hideMedia)
    media.value = undefined
  } else if (msg.type === 'alert' && (preview || msg.seq > lastSeq)) {
    // The admin window posts a preview's sample and live alerts in order, without a fitting `seq`.
    lastSeq = Math.max(lastSeq, msg.seq)
    void play(msg.payload)
  } else if (msg.type === 'screen.media' && !preview && msg.seq > lastSeq) {
    lastSeq = msg.seq
    showMedia(msg.payload)
  }
}

/** Shows a reward's video or GIF for its time; the Host paces them, so a new one only replaces a stale one. */
function showMedia(p: MediaPlay) {
  clearTimeout(hideMedia)
  media.value = p
  hideMedia = setTimeout(() => (media.value = undefined), p.durationMs)
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
  clearTimeout(hideMedia)
  ws?.close()
  window.removeEventListener('message', onPreviewMessage)
})
</script>

<style scoped>
.media {
  position: fixed;
  display: grid;
  place-items: center;
}

.media img,
.media video {
  max-width: 40vw;
  max-height: 40vh;
}

.at-top_left {
  top: 2vh;
  left: 2vw;
}

.at-top_right {
  top: 2vh;
  right: 2vw;
}

.at-bottom_left {
  bottom: 2vh;
  left: 2vw;
}

.at-bottom_right {
  right: 2vw;
  bottom: 2vh;
}

.at-center {
  inset: 0;
}

.at-full {
  inset: 0;
}

.at-full img,
.at-full video {
  width: 100vw;
  height: 100vh;
  max-width: none;
  max-height: none;
  object-fit: contain;
}
</style>
