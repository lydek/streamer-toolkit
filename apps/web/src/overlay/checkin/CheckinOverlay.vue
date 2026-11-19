<template>
  <!-- Viewer text is rendered through Vue's text interpolation only, never as HTML. -->
  <div
    v-if="card"
    :key="key"
    class="card"
    :class="{ full: card.full }"
    :style="{ '--duration': `${card.durationMs}ms` }"
    data-checkin-card
  >
    <img v-if="avatar" class="avatar" :src="avatar" alt="" referrerpolicy="no-referrer" />
    <div class="body">
      <div class="name">{{ card.userName }}</div>
      <ol class="slots" :aria-label="`${card.stamps}/${card.slots}`">
        <li v-for="n in card.slots" :key="n" :class="{ stamped: n <= card.stamps, latest: n === card.stamps }" />
      </ol>
      <div class="count">
        {{ card.stamps }}/{{ card.slots }}<span class="total">{{ t('overlay.checkin.total', { n: card.total }) }}</span>
      </div>
      <div v-if="card.full" class="banner">{{ t('overlay.checkin.full') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
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

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    // Cards are never in snapshots, so none replays after a reconnect.
    lastSeq = msg.seq
  } else if (stopsStage(msg, 'checkin') && msg.seq > lastSeq) {
    // Emergency stop or skip: the card and its sound go away at once.
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
  background: rgb(24 22 36 / 0.92);
  color: #fff;
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  text-shadow: 0 1px 2px rgb(0 0 0 / 0.6);
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
  font-size: 1.5rem;
  font-weight: 700;
  overflow-wrap: anywhere;
}

.slots {
  display: flex;
  flex-wrap: wrap;
  gap: 0.375rem;
  max-width: 22rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.slots li {
  width: 1.5rem;
  height: 1.5rem;
  border: 2px solid #d8b7ff;
  border-radius: 50%;
}

.slots li.stamped {
  background: #d8b7ff;
}

.slots li.latest {
  animation: stamp 500ms cubic-bezier(0.3, 1.6, 0.6, 1) 200ms both;
}

.count {
  font-size: 1.125rem;
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
  font-size: 1.25rem;
  font-weight: 700;
  color: #ffd76a;
}

@keyframes enter {
  from {
    opacity: 0;
    transform: translateY(1rem);
  }
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
