<template>
  <div class="chat">
    <p v-if="status !== 'open'" class="status">{{ status }}</p>
    <ul>
      <!-- Text interpolation only: viewer input is always escaped. -->
      <li v-for="m in messages" :key="m.id">
        <span class="name" :style="{ color: m.payload.color ?? undefined }">{{ m.payload.userName }}</span>
        <span class="text">{{ m.payload.text }}</span>
      </li>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'

interface ChatEvent {
  id: string
  type: 'chat.message'
  seq: number
  payload: {
    userName: string
    color: string | null
    text: string
    fragments: ({ type: 'text'; text: string } | { type: 'emote'; text: string; emoteId: string })[]
  }
}

type Incoming = { type: 'snapshot'; seq: number; payload: { chat: ChatEvent[] } } | ChatEvent

const MAX_MESSAGES = 50
const RETRY_MS = 2000

const status = ref<'connecting' | 'open' | 'closed'>('connecting')
const messages = ref<ChatEvent[]>([])
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

function connect() {
  status.value = 'connecting'
  ws = new WebSocket(`ws://${location.host}/ws/overlay`)
  ws.onopen = () => (status.value = 'open')
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data) as Incoming
    if (msg.type === 'snapshot') {
      // Each (re)connect starts from a fresh snapshot.
      messages.value = msg.payload.chat
      lastSeq = msg.seq
    } else if (msg.type === 'chat.message' && msg.seq > lastSeq) {
      lastSeq = msg.seq
      messages.value = [...messages.value, msg].slice(-MAX_MESSAGES)
    }
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
