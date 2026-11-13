<template>
  <div class="chat">
    <p v-if="status !== 'open'" class="status" role="status">{{ t(`overlay.status.${status}`) }}</p>
    <ul>
      <!-- Text interpolation only: viewer input is always escaped. -->
      <li v-for="m in state.messages" :key="m.id">
        <span class="name" :style="{ color: m.payload.color ?? undefined }">{{ m.payload.userName }}</span>
        <span class="text">{{ m.payload.text }}</span>
      </li>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { type ChatState, type Incoming, apply } from './chat'

const RETRY_MS = 2000

const { t } = useI18n()
const status = ref<'connecting' | 'open' | 'closed'>('connecting')
const state = ref<ChatState>({ messages: [], lastSeq: 0 })
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

function connect() {
  status.value = 'connecting'
  ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onopen = () => (status.value = 'open')
  ws.onmessage = (e) => {
    state.value = apply(state.value, JSON.parse(e.data) as Incoming)
  }
  ws.onclose = () => {
    status.value = 'closed'
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

<style scoped>
ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.name {
  font-weight: bold;
  margin-right: 0.5em;
}
</style>
