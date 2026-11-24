<template>
  <div class="chat-now" :class="{ 'in-dock': inDock }">
    <!-- Always in the DOM, so screen readers announce the text when it appears. -->
    <p role="status" class="connection">{{ disconnected ? t('chat.disconnected') : '' }}</p>
    <p v-if="partners.length" class="shared">
      {{ t('chat.shared', { channels: partners.join('、') }) }}
      <Button :label="t('chat.ownOnly')" size="small" text :aria-pressed="ownOnly" @click="ownOnly = !ownOnly" />
    </p>
    <div v-if="!inDock" class="bar">
      <span v-if="log.paused">{{ t('chat.paused', { n: log.unseen }) }}</span>
      <!-- WCAG 2.2.2: moving content can be paused without scrolling. -->
      <Button
        :label="log.paused ? t('chat.jump') : t('chat.pause')"
        :icon="log.paused ? 'pi pi-arrow-down' : 'pi pi-pause'"
        size="small"
        outlined
        @click="log.paused ? jump() : emit('update:log', pause(log))"
      />
      <ChatDisplayMenu class="display" :dock-side="dockSide" />
    </div>
    <!-- aria-live off: a busy chat would talk over everything else. Focusable so the keyboard can scroll it. -->
    <div
      ref="el"
      class="log"
      role="log"
      aria-live="off"
      tabindex="0"
      :aria-label="t('chat.log')"
      :style="{ fontSize: `${chatDisplay.textSize}%` }"
      @scroll="onScroll"
    >
      <p v-if="!log.messages.length">{{ t('home.chatEmpty') }}</p>
      <!-- Viewer input only ever goes in as text ({{ }}), never as markup. -->
      <ChatLine v-for="m in shownMessages" :key="m.id" class="msg" :message="m" :name-color="color(m)" :broadcaster-id="broadcasterId" />
    </div>
    <button v-if="inDock && log.paused" type="button" class="dock-new" @click="jump">
      {{ log.unseen ? t('dock.newMessages', { n: log.unseen }) : t('dock.jumpLatest') }}
    </button>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ChatEvent } from '../overlay/chat/chat'
import { chatDisplay, loadBadges, loadChatDisplay, sharedChannels, wantAvatars } from './chatDisplay'
import ChatDisplayMenu from './ChatDisplayMenu.vue'
import ChatLine from './ChatLine.vue'
import { type ChatLog, type Rgb, backgroundOf, nameColor, pause, resume } from './chatLog'

/** `dockSide`: shown in the docked panel, whose display menu also picks the panel's side.
 * `inDock`: header owns pause/display controls; list fills the panel unboxed with a floating new-messages pill. */
const props = defineProps<{ log: ChatLog; broadcasterId?: string; disconnected?: boolean; dockSide?: boolean; inDock?: boolean }>()
const emit = defineEmits<{ 'update:log': [ChatLog] }>()
const { t } = useI18n()

const el = ref<HTMLElement>()
void loadChatDisplay().catch(() => {})
watch(() => props.broadcasterId, loadBadges, { immediate: true })

// A shared chat: the other channels taking part, and 只看本頻道 to hide their messages.
const partners = computed(() => sharedChannels(props.log.messages))
const ownOnly = ref(false)
const shownMessages = computed(() =>
  ownOnly.value && partners.value.length ? props.log.messages.filter((m) => !m.payload.sharedFrom) : props.log.messages,
)
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
  (messages) => {
    wantAvatars(messages)
    if (!props.log.paused) toBottom()
  },
  { flush: 'post', immediate: true },
)

onMounted(() => {
  bg.value = backgroundOf(el.value!, bg.value)
  colors.clear()
  // A reopened drawer starts at the latest message; a paused dock stays where the reader left it.
  if (!props.inDock || !props.log.paused) void jump()
})

defineExpose({ jump })
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

.bar > .p-button {
  margin-inline-start: auto;
}

.display {
  flex-basis: 100%;
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
  margin: 0;
}

.log .msg {
  margin-bottom: 0.125rem;
}

.chat-now.in-dock {
  position: relative;
  flex: 1;
  min-height: 0;
  gap: 0;
}

.chat-now.in-dock .connection:not(:empty),
.chat-now.in-dock .shared {
  padding: 0.375rem 0.75rem;
}

.chat-now.in-dock .connection:empty {
  margin-block-end: 0;
}

.chat-now.in-dock .log {
  flex: 1;
  min-height: 0;
  height: auto;
  padding: 0.5rem 0.75rem;
  border: 0;
  border-radius: 0;
}

.dock-new {
  position: absolute;
  left: 50%;
  bottom: 0.75rem;
  transform: translateX(-50%);
  padding: 0.375rem 0.875rem;
  border: 0;
  border-radius: 999px;
  background: var(--p-primary-color);
  color: var(--p-primary-contrast-color);
  font: inherit;
  font-size: 0.8125rem;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
}

.shared {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
