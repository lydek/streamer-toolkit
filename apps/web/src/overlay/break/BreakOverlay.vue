<template>
  <div v-if="card?.shown" class="card" data-break>
    <b v-if="card.text" class="text">{{ card.text }}</b>
    <span v-if="left > 0" class="num" data-break-countdown>{{ countdownText(left) }}</span>
    <span v-else-if="card.endText" class="end" data-break-end>{{ card.endText }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import type { Incoming } from '../chat/chat'
import { type BreakCard, breakLeftMs, countdownText } from './breakCard'

// `/overlay/break/`: the break / starting-soon card, counting down to the end the Host stored.
const RETRY_MS = 2000
const card = shallowRef<BreakCard>()
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 250)
const left = computed(() => (card.value ? breakLeftMs(card.value, now.value) : 0))
// A tick up to 250 ms old would count a card that just arrived past its length (10:01 before 10:00).
watch(card, () => (now.value = Date.now()))
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

function receive(msg: Incoming) {
  if (msg.type === 'snapshot') {
    lastSeq = msg.seq
    card.value = msg.payload['screen.break'].at(-1)?.payload
  } else if (msg.type === 'screen.break' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    card.value = msg.payload
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=break`)
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
.card {
  display: inline-grid;
  justify-items: center;
  gap: 0.25rem;
  padding: 1rem 1.75rem;
  border-radius: 0.75rem;
  background: rgb(0 0 0 / 70%);
  color: #fff;
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  font-size: 1.5rem;
}

.num,
.end {
  font-size: 2rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
