<template>
  <section v-if="board" class="board" data-leaderboard>
    <h1 class="title">{{ t(`overlay.leaderboard.${board.board}`) }}</h1>
    <ol class="entries">
      <li v-for="(e, i) in board.entries" :key="i">
        <span class="name">{{ e.name }}</span>
        <span class="value">{{ e.value.toLocaleString() }}</span>
      </li>
    </ol>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { components } from '../../api/schema'
import { followAppearance } from '../appearance'
import type { Incoming } from '../chat/chat'

type Boards = components['schemas']['LeaderboardPayload']

// `/overlay/leaderboard/?board=bits|gifts|chat|checkins&period=stream|month|all`: one board, as the Host ranks it.
const RETRY_MS = 2000
const params = new URLSearchParams(location.search)
const wanted = { board: params.get('board') ?? 'chat', period: params.get('period') ?? 'stream' }
const { t } = useI18n()
const latest = shallowRef<Boards>()
const board = computed(() => latest.value?.boards.find((b) => b.board === wanted.board && b.period === wanted.period))
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
    latest.value = msg.payload['leaderboard.update']?.at(-1)?.payload
  } else if (msg.type === 'leaderboard.update' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    latest.value = msg.payload
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=leaderboard`)
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
.board {
  display: inline-grid;
  gap: 0.5rem;
  min-width: 16rem;
  padding: 0.75rem 1rem;
  border-radius: 0.75rem;
  background: var(--ov-bg, rgb(0 0 0 / 70%));
  color: var(--ov-fg, #fff);
  font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
  font-size: calc(1.25rem * var(--ov-scale, 1));
  text-shadow: var(--ov-shadow, 0 1px 2px rgb(0 0 0 / 60%));
}

.title {
  margin: 0;
  font-size: calc(1rem * var(--ov-scale, 1));
  color: var(--ov-accent, #ffd54f);
}

.entries {
  display: grid;
  gap: 0.25rem;
  margin: 0;
  padding-left: 1.5em;
}

.entries li {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
}

.name {
  overflow-wrap: anywhere;
}

.value {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
