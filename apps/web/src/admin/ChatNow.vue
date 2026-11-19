<template>
  <div class="chat-now">
    <!-- Always in the DOM, so screen readers announce the text when it appears. -->
    <p role="status" class="connection">{{ disconnected ? t('chat.disconnected') : '' }}</p>
    <div class="bar">
      <span v-if="log.paused">{{ t('chat.paused', { n: log.unseen }) }}</span>
      <!-- WCAG 2.2.2: moving content can be paused without scrolling. -->
      <Button
        :label="log.paused ? t('chat.jump') : t('chat.pause')"
        :icon="log.paused ? 'pi pi-arrow-down' : 'pi pi-pause'"
        size="small"
        outlined
        @click="log.paused ? jump() : emit('update:log', pause(log))"
      />
    </div>
    <!-- aria-live off: a busy chat would talk over everything else. Focusable so the keyboard can scroll it. -->
    <div ref="el" class="log" role="log" aria-live="off" tabindex="0" :aria-label="t('chat.log')" @scroll="onScroll">
      <p v-if="!log.messages.length">{{ t('home.chatEmpty') }}</p>
      <!-- Viewer input only ever goes in as text ({{ }}), never as markup. -->
      <p v-for="m in log.messages" :key="m.id" class="msg">
        <span v-if="broadcasterId && m.payload.userId === broadcasterId" class="badge">{{ t('chat.broadcaster') }}</span>
        <span class="name" :style="{ color: color(m) }">{{ m.payload.userName }}</span>：<span v-if="m.deleted" class="deleted">{{
          t('chat.deleted')
        }}</span
        ><template v-else>{{ m.payload.text }}</template>
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { nextTick, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ChatEvent } from '../overlay/chat/chat'
import { type ChatLog, type Rgb, backgroundOf, nameColor, pause, resume } from './chatLog'

const props = defineProps<{ log: ChatLog; broadcasterId?: string; disconnected?: boolean }>()
const emit = defineEmits<{ 'update:log': [ChatLog] }>()
const { t } = useI18n()

const el = ref<HTMLElement>()
// Theme surface.900 until the real background is read on mount.
const bg = ref<Rgb>([0x15, 0x17, 0x25])
const colors = new Map<string, string>()
let lastTop = 0

function color(m: ChatEvent) {
  const key = `${m.payload.color}|${m.payload.userId}`
  let c = colors.get(key)
  if (!c) colors.set(key, (c = nameColor(m.payload.color, m.payload.userId, bg.value)))
  return c
}

const atBottom = (e: HTMLElement) => e.scrollHeight - e.scrollTop - e.clientHeight <= 2

function toBottom() {
  if (el.value) lastTop = el.value.scrollTop = el.value.scrollHeight
}

// Only the reader's own scrolling counts: scrolling up pauses, scrolling down to the end resumes. Scroll events
// with no movement (a drawer moving between aside and dialog) change nothing.
function onScroll() {
  const e = el.value!
  const up = e.scrollTop < lastTop
  const down = e.scrollTop > lastTop
  lastTop = e.scrollTop
  if (atBottom(e)) {
    if (down && props.log.paused) emit('update:log', resume(props.log))
  } else if (up && !props.log.paused) {
    emit('update:log', pause(props.log))
  }
}

async function jump() {
  emit('update:log', resume(props.log))
  await nextTick()
  toBottom()
}

watch(
  () => props.log.messages,
  () => {
    if (!props.log.paused) toBottom()
  },
  { flush: 'post' },
)

onMounted(() => {
  bg.value = backgroundOf(el.value!, bg.value)
  colors.clear()
  // A reopened drawer starts at the latest message.
  void jump()
})
</script>

<style scoped>
.deleted {
  color: var(--p-text-muted-color);
  font-style: italic;
}

.chat-now {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

/* Empty, it cancels its flex gap instead of display:none, which would drop it from the accessibility tree. */
.connection {
  margin: 0;
}

.connection:empty {
  margin-block-end: -0.5rem;
}

.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}

.bar .p-button {
  margin-inline-start: auto;
}

.log {
  height: min(24rem, 50vh);
  overflow-y: auto;
  padding: 0.5rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
}

.log:focus-visible {
  outline: 2px solid var(--p-primary-color);
  outline-offset: 2px;
}

.log p {
  margin: 0 0 0.25rem;
  overflow-wrap: anywhere;
}

.name {
  font-weight: 600;
}

/* surface.950 on primary.400: 11:1. */
.badge {
  margin-inline-end: 0.25rem;
  padding: 0 0.375rem;
  border-radius: 6px;
  background: var(--p-primary-400);
  color: var(--p-surface-950);
  font-size: 0.875rem;
}
</style>
