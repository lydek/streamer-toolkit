<template>
  <div v-if="text" class="timer" :class="{ paused: !running }" data-marathon>{{ text }}</div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { clockText } from '../../admin/home'
import type { Incoming } from '../chat/chat'
import { type Marathon, remainingMs } from './marathon'

// `/overlay/marathon/`: the sub marathon countdown, as the Host's clock says.
const RETRY_MS = 2000
const latest = shallowRef<{ payload: Marathon; occurredAt: string }>()
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 250)
const running = computed(() => latest.value?.payload.running ?? false)
const text = computed(() =>
  latest.value ? clockText(remainingMs(latest.value.payload, latest.value.occurredAt, now.value)) : '',
)
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    lastSeq = msg.seq
    latest.value = msg.payload['counter.marathon'].at(-1)
  } else if (msg.type === 'counter.marathon' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    latest.value = msg
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=marathon`)
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
.timer {
  padding: 0.5rem;
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  font-size: 3rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: #fff;
  text-shadow: 0 2px 4px rgb(0 0 0 / 80%);
}

.paused {
  opacity: 0.7;
}
</style>
