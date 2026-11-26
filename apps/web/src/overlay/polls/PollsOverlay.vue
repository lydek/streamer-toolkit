<template>
  <!-- Viewer-facing Twitch text only ever goes in as text ({{ }}), never as markup. -->
  <section v-if="shown?.kind === 'poll'" class="card" data-poll>
    <h1 class="title">{{ shown.poll.title }}</h1>
    <div v-for="c in shown.poll.choices" :key="c.id" class="row" :class="{ lead: ended && c.votes === topVotes && topVotes > 0 }">
      <span class="name">{{ c.title }}</span>
      <span class="track" aria-hidden="true"><i :style="{ width: `${share(c.votes, pollTotal)}%` }"></i></span>
      <b class="value">{{ share(c.votes, pollTotal) }}%</b>
    </div>
    <p class="foot">{{ ended ? t('overlay.polls.ended') : timeLeft(shown.poll.endsAt) }}</p>
  </section>
  <section v-else-if="shown?.kind === 'prediction'" class="card" data-prediction>
    <h1 class="title">{{ shown.prediction.title }}</h1>
    <div
      v-for="o in shown.prediction.outcomes"
      :key="o.id"
      class="row"
      :class="{ lead: o.id === shown.prediction.winningOutcomeId }"
    >
      <span class="name">{{ o.title }}</span>
      <span class="track" aria-hidden="true"><i :style="{ width: `${share(o.points, predictionTotal)}%` }"></i></span>
      <b class="value">{{ o.points.toLocaleString() }}</b>
    </div>
    <p class="foot">{{ predictionFoot }}</p>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { shortClockText } from '../../admin/home'
import type { components } from '../../api/schema'
import { followAppearance } from '../appearance'
import type { Incoming } from '../chat/chat'

type Contest = components['schemas']['ContestPayload']
type Poll = components['schemas']['PollView']
type Prediction = components['schemas']['PredictionView']

// `/overlay/polls/`: the running poll or prediction with its live result; once it ends, the result stays this long
// after Twitch's end time (a result read later, e.g. from an earlier stream, never comes back on screen).
const RESULT_MS = 10_000
const RETRY_MS = 2000
const { t } = useI18n()
// The latest `twitch.contest`.
const latest = shallowRef<Contest>()
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 250)
let lastSeq = 0
let ws: WebSocket | undefined
let retry: ReturnType<typeof setTimeout> | undefined
let stopped = false

const running = (p: Prediction) => p.status === 'active' || p.status === 'locked'
// When an ended one ended, by Twitch's clock; NaN when Twitch did not say (then it is not shown).
const endedAt = (p: Poll | Prediction) => Date.parse('endedAt' in p ? p.endedAt : p.endsAt)
// What shows: a running poll, else a running prediction, else whichever ended last, while its result is fresh.
const shown = computed<{ kind: 'poll'; poll: Poll } | { kind: 'prediction'; prediction: Prediction } | undefined>(() => {
  const c = latest.value
  if (!c) return undefined
  const { poll, prediction } = c
  if (poll?.status === 'active') return { kind: 'poll', poll }
  if (prediction && running(prediction)) return { kind: 'prediction', prediction }
  const pollEnd = poll ? endedAt(poll) : NaN
  const predictionEnd = prediction ? endedAt(prediction) : NaN
  const fresh = (end: number) => now.value - end <= RESULT_MS
  if (poll && fresh(pollEnd) && !(predictionEnd > pollEnd)) return { kind: 'poll', poll }
  if (prediction && fresh(predictionEnd)) return { kind: 'prediction', prediction }
  return undefined
})
const ended = computed(() => shown.value?.kind === 'poll' && shown.value.poll.status !== 'active')
const pollTotal = computed(() => (shown.value?.kind === 'poll' ? shown.value.poll.choices.reduce((sum, c) => sum + c.votes, 0) : 0))
const topVotes = computed(() => (shown.value?.kind === 'poll' ? Math.max(0, ...shown.value.poll.choices.map((c) => c.votes)) : 0))
const predictionTotal = computed(() =>
  shown.value?.kind === 'prediction' ? shown.value.prediction.outcomes.reduce((sum, o) => sum + o.points, 0) : 0,
)
const share = (part: number, total: number) => (total ? Math.round((part / total) * 100) : 0)
const timeLeft = (at: string) => shortClockText(Date.parse(at) - now.value)
const predictionFoot = computed(() => {
  if (shown.value?.kind !== 'prediction') return ''
  const p = shown.value.prediction
  if (p.status === 'active') return timeLeft(p.locksAt)
  if (p.status === 'resolved') {
    const name = p.outcomes.find((o) => o.id === p.winningOutcomeId)?.title ?? ''
    return t('overlay.polls.won', { name })
  }
  return t(`overlay.polls.${p.status}`)
})

// Theme and text size from 畫面工具 (built-in look only).
const appearance = followAppearance()

function receive(msg: Incoming) {
  appearance(msg)
  if (msg.type === 'snapshot') {
    lastSeq = msg.seq
    latest.value = msg.payload['twitch.contest']?.at(-1)?.payload
  } else if (msg.type === 'twitch.contest' && msg.seq > lastSeq) {
    lastSeq = msg.seq
    latest.value = msg.payload
  }
}

function connect() {
  ws = new WebSocket(`ws://${location.host}/ws/overlay?kind=polls`)
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
  gap: 0.5rem;
  min-width: 20rem;
  max-width: 32rem;
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
  font-size: calc(1.125rem * var(--ov-scale, 1));
  overflow-wrap: anywhere;
}

.row {
  display: grid;
  grid-template-columns: minmax(4rem, 10rem) minmax(4rem, 1fr) auto;
  gap: 0.75rem;
  align-items: center;
}

.name {
  overflow-wrap: anywhere;
}

.track {
  height: 0.5em;
  border-radius: 999px;
  background: rgb(255 255 255 / 20%);
  overflow: hidden;
}

.track i {
  display: block;
  height: 100%;
  background: var(--ov-accent, #ffd54f);
  transition: width 0.3s;
}

.row.lead .name,
.row.lead .value {
  color: var(--ov-accent, #ffd54f);
}

.value {
  font-variant-numeric: tabular-nums;
}

.foot {
  margin: 0;
  font-size: calc(0.875rem * var(--ov-scale, 1));
  opacity: 0.8;
}
</style>
