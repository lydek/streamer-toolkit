<template>
  <!-- Counter names are broadcaster text, rendered through Vue's text interpolation only. -->
  <ul class="counters">
    <li v-for="c in counters" :key="c.id" data-counter>
      <span class="name">{{ c.name }}</span>
      <span class="value">{{ c.value }}</span>
    </li>
    <!-- A test value from 模擬事件, kept apart from the real counters (docs/architecture.md). -->
    <li v-if="test !== undefined" class="test" data-counter-test>
      <span class="name">{{ t('overlay.counter.test') }}</span>
      <span class="value">{{ test }}</span>
    </li>
  </ul>
</template>

<script setup lang="ts">
import { followAppearance } from '../appearance'
import { onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { components } from '../../api/schema'
import type { Incoming } from '../chat/chat'

// `/overlay/counter/`: every counter's name and value, kept current by `counter.values`.
const RETRY_MS = 2000
// How long a `counter.test` value stays; another one starts the time again.
const TEST_MS = 3000
const { t } = useI18n()
const counters = shallowRef<components['schemas']['CounterValue'][]>([])
const test = ref<number>()
let hideTest: ReturnType<typeof setTimeout> | undefined
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
    counters.value = msg.payload['counter.values'].at(-1)?.payload.counters ?? []
  } else if (msg.type === 'counter.values' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    counters.value = msg.payload.counters
  } else if (msg.type === 'counter.test') {
    test.value = msg.payload.value
    clearTimeout(hideTest)
    hideTest = setTimeout(() => (test.value = undefined), TEST_MS)
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
  clearTimeout(hideTest)
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
  color: var(--ov-fg, #fff);
  text-shadow: var(--ov-shadow, 0 2px 4px rgb(0 0 0 / 80%));
}

li {
  display: flex;
  gap: 0.75rem;
  align-items: baseline;
  font-size: calc(2rem * var(--ov-scale, 1));
}

.value {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.test {
  color: #ffd54f;
  font-style: italic;
}
</style>
