<template>
  <!-- Viewer text is rendered through Vue's text interpolation only, never as HTML. -->
  <div
    v-if="card"
    :key="key"
    class="card"
    :class="[{ full: card.full }, card.style && [`preset-${card.style.preset}`, `anim-${card.style.animation}`]]"
    :style="look"
    data-checkin-card
  >
    <img v-if="avatar" class="avatar" :src="avatar" alt="" referrerpolicy="no-referrer" />
    <div class="body">
      <div class="name">{{ card.title ?? card.userName }}</div>
      <ol class="slots" :aria-label="`${card.stamps}/${card.slots}`">
        <li v-for="n in card.slots" :key="n" :class="{ stamped: n <= card.stamps, latest: n === card.stamps }" />
      </ol>
      <div class="count">
        {{ card.stamps }}/{{ card.slots }}<span class="total">{{
          card.description ?? t('overlay.checkin.total', { n: card.total })
        }}</span>
      </div>
      <div v-if="card.full" class="banner">{{ t('overlay.checkin.full') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { followAppearance } from '../appearance'
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { components } from '../../api/schema'
import { type Incoming, stopsStage } from '../chat/chat'
import { playSound, stopSounds } from '../sound'
import { twitchImage } from './checkin'

type Stamped = components['schemas']['CheckinStampedPayload']

const RETRY_MS = 2000
const { t } = useI18n()
const card = shallowRef<Stamped>()
// A new key restarts the animations when the next card replaces the current one.
const key = ref(0)
const avatar = computed(() => (card.value?.avatarUrl ? twitchImage(card.value.avatarUrl) : undefined))
// Text on a light card color (手帳's paper, or any light color picked) is dark, so it keeps WCAG AA contrast.
const LIGHT_CARD = { '--card-fg': '#1f1a17', '--card-banner': '#7a5200', '--card-shadow': 'none' }
/** Whether `#rrggbb` is lighter than the point where dark text contrasts better than white. */
function light(hex: string) {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex)
  if (!m) return false
  const [r, g, b] = m.slice(1).map((h) => {
    const c = parseInt(h, 16) / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b! > 0.179
}
/** The card's style as CSS variables; images come from the Host's own `/checkin/` route only. */
const look = computed(() => {
  const c = card.value
  if (!c) return {}
  const vars: Record<string, string> = { '--duration': `${c.durationMs}ms` }
  const s = c.style
  if (!s) return vars
  const image = (url: string | null) => (url && url.startsWith('/checkin/') ? `url("${url}")` : 'none')
  return {
    ...vars,
    '--card-bg': s.backgroundColor,
    ...(light(s.backgroundColor) ? LIGHT_CARD : {}),
    '--card-image': image(s.backgroundUrl),
    '--stamp': s.stampColor,
    '--stamp-image': image(s.stampUrl),
    '--per-row': String(s.perRow),
  }
})
let lastSeq = 0
let hide: ReturnType<typeof setTimeout> | undefined
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

/** Shows one card for its duration with its sound; the Host paces them, so a new one only replaces a stale one. */
function show(p: Stamped) {
  clearTimeout(hide)
  card.value = p
  key.value++
  hide = setTimeout(() => (card.value = undefined), p.durationMs)
  if (p.sound) playSound(p.sound.url, p.sound.volume)
}

// Theme and text size from 畫面工具 (built-in look only).
const appearance = followAppearance()

function receive(msg: Incoming) {
  appearance(msg)
  if (msg.type === 'snapshot') {
    // Cards are never in snapshots, so none replays after a reconnect.
    lastSeq = msg.seq
  } else if ((stopsStage(msg, 'checkin') || msg.type === 'checkin.hidden') && msg.seq > lastSeq) {
    // Emergency stop, skip or 隱藏畫面卡片: the card and its sound go away at once.
    lastSeq = msg.seq
    clearTimeout(hide)
    card.value = undefined
    stopSounds()
  } else if (msg.type === 'checkin.stamped' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    show(msg.payload)
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=checkin`)
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

connect()
onBeforeUnmount(() => {
  stopped = true
  clearTimeout(retry)
  clearTimeout(hide)
  ws?.close()
})

defineExpose({ receive })
</script>

<style>
html,
body {
  margin: 0;
  background: transparent;
}
</style>

<style scoped>
.card {
  display: flex;
  align-items: center;
  gap: 1rem;
  width: fit-content;
  max-width: calc(100vw - 2rem);
  margin: 1rem;
  padding: 1rem 1.25rem;
  border-radius: 12px;
  /* The card style's color and image win over the overlay theme's background. */
  background: var(--card-image, none) center / cover no-repeat, var(--card-bg, var(--ov-bg, rgb(24 22 36 / 0.92)));
  color: var(--card-fg, var(--ov-fg, #fff));
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  text-shadow: var(--card-shadow, var(--ov-shadow, 0 1px 2px rgb(0 0 0 / 0.6)));
  animation:
    enter 300ms ease-out,
    leave 300ms ease-in calc(var(--duration) - 300ms) forwards;
}

.avatar {
  width: 72px;
  height: 72px;
  border-radius: 50%;
  flex: none;
}

.body {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  min-width: 0;
}

.name {
  font-size: calc(1.5rem * var(--ov-scale, 1));
  font-weight: 700;
  overflow-wrap: anywhere;
}

.slots {
  display: grid;
  grid-template-columns: repeat(var(--per-row, 10), 1.5rem);
  gap: 0.375rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.slots li {
  width: 1.5rem;
  height: 1.5rem;
  border: 2px solid var(--stamp, var(--ov-accent, #d8b7ff));
  border-radius: 50%;
}

.slots li.stamped {
  background: var(--stamp-image, none) center / contain no-repeat, var(--stamp, var(--ov-accent, #d8b7ff));
}

.slots li.latest {
  animation: stamp 500ms cubic-bezier(0.3, 1.6, 0.6, 1) 200ms both;
}

.count {
  font-size: calc(1.125rem * var(--ov-scale, 1));
}

.total {
  margin-inline-start: 0.75rem;
  opacity: 0.85;
}

.full {
  outline: 3px solid #ffd76a;
  animation:
    enter 300ms ease-out,
    glow 900ms ease-in-out 300ms 3,
    leave 300ms ease-in calc(var(--duration) - 300ms) forwards;
}

.full .slots li {
  border-color: #ffd76a;
  background: #ffd76a;
}

.banner {
  font-size: calc(1.25rem * var(--ov-scale, 1));
  font-weight: 700;
  color: var(--card-banner, #ffd76a);
}

@keyframes enter {
  from {
    opacity: 0;
    transform: translateY(1rem);
  }
}

/* The card style's entry animation; slide is the default above, and a full card keeps its glow. */
.anim-fade:not(.full) {
  animation-name: fade-in, leave;
}

.anim-pop:not(.full) {
  animation-name: pop-in, leave;
}

.anim-none:not(.full) {
  animation-name: none, leave;
}

@keyframes fade-in {
  from {
    opacity: 0;
  }
}

@keyframes pop-in {
  from {
    opacity: 0;
    transform: scale(0.6);
  }
}

/* 手帳: a paper page with ruled lines and a dashed edge. */
.preset-journal {
  border: 2px dashed rgb(0 0 0 / 25%);
  font-family: 'Segoe Print', 'Microsoft JhengHei', serif;
  text-shadow: none;
  background-image: var(--card-image, none),
    repeating-linear-gradient(transparent 0 1.4rem, rgb(0 0 0 / 8%) 1.4rem 1.5rem);
}

@keyframes leave {
  to {
    opacity: 0;
    transform: translateY(1rem);
  }
}

@keyframes stamp {
  from {
    transform: scale(2.2);
    opacity: 0;
  }
}

@keyframes glow {
  50% {
    box-shadow: 0 0 1.5rem 0.5rem rgb(255 215 106 / 0.7);
  }
}

@media (prefers-reduced-motion: reduce) {
  .card,
  .full,
  .slots li.latest {
    animation: none;
  }
}
</style>
