<template>
  <!-- Counter names are broadcaster text, rendered through Vue's text interpolation only. -->
  <ul class="counters">
    <li v-for="c in counters" :key="c.id" data-counter>
      <span class="name">{{ c.name }}</span>
      <span class="value">{{ c.value }}</span>
    </li>
  </ul>
</template>

<script setup lang="ts">
import { onBeforeUnmount, shallowRef } from 'vue'
import type { components } from '../../api/schema'
import type { Incoming } from '../chat/chat'

// `/overlay/counter/`: every counter's name and value, kept current by `counter.values`.
const RETRY_MS = 2000
const counters = shallowRef<components['schemas']['CounterValue'][]>([])
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    lastSeq = msg.seq
    counters.value = msg.payload['counter.values'].at(-1)?.payload.counters ?? []
  } else if (msg.type === 'counter.values' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    counters.value = msg.payload.counters
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=counter`)
  ws.onmessage = (e) => receive(JSON.parse(e.data) as Incoming)
  ws.onclose = () => {
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

<style>
html,
body {
  margin: 0;
  background: transparent;
}
</style>

<style scoped>
.counters {
  list-style: none;
  margin: 0;
  padding: 0.5rem;
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  color: #fff;
  text-shadow: 0 2px 4px rgb(0 0 0 / 80%);
}

li {
  display: flex;
  gap: 0.75rem;
  align-items: baseline;
  font-size: 2rem;
}

.value {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
