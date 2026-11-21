<template>
  <ul v-if="shown.length" class="warnings" data-warnings>
    <li v-for="id in shown" :key="id" :data-warning="id">{{ t(`overlay.warnings.${id}`) }}</li>
  </ul>
</template>

<script setup lang="ts">
import { followAppearance } from '../appearance'
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Incoming } from '../chat/chat'
import { type Warnings, activeWarnings } from './warnings'

// `/overlay/warnings/`: small on-stream warnings (viewers see them too) when something is wrong.
const RETRY_MS = 2000
const { t } = useI18n()
const latest = shallowRef<Warnings>()
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 250)
const shown = computed(() => (latest.value ? activeWarnings(latest.value, now.value) : []))
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

// Theme and text size from 畫面工具 (built-in look only).
const appearance = followAppearance()

function receive(msg: Incoming) {
  appearance(msg)
  if (msg.type === 'snapshot') {
    lastSeq = msg.seq
    latest.value = msg.payload['screen.warnings'].at(-1)?.payload
  } else if (msg.type === 'screen.warnings' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    latest.value = msg.payload
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=warnings`)
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
    if (!stopped) retry = setTimeout(connect, RETRY_MS)
  }
}

connect()
onBeforeUnmount(() => {
  stopped = true
  clearInterval(ticker)
  clearTimeout(retry)
  ws?.close()
})
</script>

<style>
html,
body {
  margin: 0;
  background: transparent;
}
</style>

<style scoped>
.warnings {
  display: grid;
  justify-items: start;
  gap: 0.25rem;
  margin: 0;
  padding: 0.5rem;
  list-style: none;
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  font-size: calc(1rem * var(--ov-scale, 1));
}

.warnings li {
  padding: 0.125rem 0.5rem;
  border-radius: 4px;
  background: var(--ov-bg, rgb(0 0 0 / 70%));
  color: var(--ov-accent, #ffd54f);
}
</style>
