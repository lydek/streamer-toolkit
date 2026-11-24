<template>
  <!-- The docked chat panel: beside the pages on wide windows (resizable, saved), a slide-in panel below 1000px. -->
  <aside
    v-if="shown"
    id="chat-dock"
    ref="panel"
    class="dock"
    :class="{ sheet: !wide, left: wide && dock.side === 'left' }"
    :style="wide ? { width: `${shownPx}px` } : undefined"
    :aria-label="t('dock.title')"
    tabindex="-1"
  >
    <div
      v-if="wide"
      class="resize"
      role="separator"
      aria-orientation="vertical"
      tabindex="0"
      :aria-label="t('dock.resize')"
      :aria-valuenow="shownPx"
      :aria-valuemin="DOCK_MIN"
      :aria-valuemax="DOCK_MAX"
      @pointerdown="startResize"
      @keydown="resizeByKey"
    ></div>
    <div class="head">
      <h2><i class="pi pi-comments" aria-hidden="true"></i>{{ t('dock.title') }}</h2>
      <ChatDisplayMenu dock-side icon-only />
      <Button
        icon="pi pi-search"
        text
        rounded
        :aria-label="t('dock.search')"
        :aria-expanded="searchOpen"
        aria-controls="dock-search"
        @click="toggleSearch"
      />
      <Button
        :icon="log.paused ? 'pi pi-play' : 'pi pi-pause'"
        text
        rounded
        :aria-label="t(log.paused ? 'dock.resumeScroll' : 'dock.pauseScroll')"
        :aria-pressed="log.paused"
        @click="togglePause"
      />
      <Button icon="pi pi-window-maximize" text rounded :aria-label="t('dock.openFullPage')" @click="openFullPage" />
      <Button :icon="`pi ${wide && dock.side === 'left' ? 'pi-angle-double-left' : 'pi-angle-double-right'}`" text rounded :aria-label="t('dock.close')" @click="close" />
    </div>
    <p v-if="twitchOverview?.chat_missed_writes" role="alert">{{ t('chat.missedWrites', { n: twitchOverview.chat_missed_writes }) }}</p>
    <p v-if="twitchOverview?.chat_write_delayed" role="alert">{{ t('chat.writeDelayed') }}</p>
    <p v-if="historyFailed" role="alert">{{ t('chat.historyRetry') }}</p>
    <div class="tools">
      <InputText
        v-if="searchOpen"
        id="dock-search"
        ref="searchEl"
        v-model="query"
        type="search"
        size="small"
        :placeholder="t('dock.search')"
        :aria-label="t('dock.search')"
      />
      <div class="filters" role="group" :aria-label="t('dock.filters')">
        <button
          v-for="f in FILTERS"
          :key="f"
          type="button"
          :aria-pressed="filter === f"
          :disabled="f === 'mods'"
          :title="f === 'mods' ? t('dock.modsLater') : undefined"
          @click="filter = f"
        >
          {{ t(`dock.filter.${f}`) }}
        </button>
      </div>
    </div>
    <div class="body">
      <ul v-if="filter === 'events'" class="events" role="log" aria-live="off" :aria-label="t('dock.filter.events')">
        <li v-if="!shownEvents.length">{{ t('alerts.empty') }}</li>
        <!-- Viewer input only ever goes in as text. -->
        <li v-for="e in shownEvents" :key="e.id"><i class="pi pi-bolt" aria-hidden="true"></i>{{ e.title }}</li>
      </ul>
      <ChatNow
        v-else
        ref="chatNow"
        :log="shownLog"
        :broadcaster-id="broadcaster?.id"
        :disconnected="live.status === 'failed'"
        in-dock
        @update:log="(next) => (log = next.paused ? pause(log) : resume(log))"
      />
    </div>
    <!-- Sending needs the chat send channel (a later phase); the box shows where it will be. -->
    <div class="send">
      <RewardPicker />
      <InputText disabled :placeholder="t('dock.sendLater')" :aria-label="t('dock.send')" />
      <Button icon="pi pi-send" disabled :aria-label="t('dock.send')" />
    </div>
  </aside>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { errorMessage } from '../i18n'
import { alertTitle } from '../overlay/alerts/alerts'
import { ApiError } from './api'
import ChatDisplayMenu from './ChatDisplayMenu.vue'
import RewardPicker from './RewardPicker.vue'
import ChatNow from './ChatNow.vue'
import { pause, resume, useChatHistory } from './chatLog'
import { DOCK_MAX, DOCK_MIN, DOCK_WIDE_QUERY, clampWidth, saveDock, shownWidth, useDock } from './dock'
import { followDrag } from './drag'
import { useModules } from './features'
import { errorToast, twitchOverview, useLiveEvents } from './home'

const FILTERS = ['all', 'mentions', 'events', 'mods'] as const
const EVENT_LIMIT = 100

const { t } = useI18n()
const toast = useToast()
const dock = useDock()
const { hidden } = useModules()

const wideQuery = window.matchMedia(DOCK_WIDE_QUERY)
const wide = ref(wideQuery.matches)
// A smaller window shows the panel narrower; the saved width stays for when it is wide again.
const viewport = ref(window.innerWidth)
const onResize = () => (viewport.value = window.innerWidth)
const shownPx = computed(() => shownWidth(dock.width, viewport.value))
const onWide = () => {
  wide.value = wideQuery.matches
  dock.sheet = false
}
// Turning the chat feature off in settings removes the panel too.
// The chat log page shows the chat itself: the panel steps aside there and comes back after, with its saved
// open state untouched.
const route = useRoute()
const router = useRouter()
const shown = computed(
  () => !hidden.value.has('/chat') && route.path !== '/chat-log' && (wide.value ? dock.open : dock.sheet),
)
watchEffect(() => (dock.docked = wide.value && shown.value))

const filter = ref<(typeof FILTERS)[number]>('all')
const query = ref('')
const searchOpen = ref(false)
const searchEl = ref<{ $el: HTMLInputElement }>()
const chatNow = ref<InstanceType<typeof ChatNow>>()
function toggleSearch() {
  searchOpen.value = !searchOpen.value
  if (searchOpen.value) void nextTick(() => searchEl.value?.$el.focus())
  // A closed search box must not keep filtering messages out of sight.
  else query.value = ''
}
function togglePause() {
  if (!log.value.paused) log.value = pause(log.value)
  else if (chatNow.value) void chatNow.value.jump()
  else log.value = resume(log.value)
}
function openFullPage() {
  if (!wide.value) dock.sheet = false
  void router.push('/chat-log')
}
const { log, accept, historyFailed } = useChatHistory()
const events = shallowRef<{ id: string; title: string }[]>([])
const live = useLiveEvents(2000, (msg) => {
  accept(msg)
  // Every alert that arrived, whether or not an overlay in OBS plays it.
  if (msg.type === 'alert.received') events.value = [...events.value, { id: msg.id, title: alertTitle(msg.payload) }].slice(-EVENT_LIMIT)
})

// "Mentions me" needs the broadcaster's login; it follows signing in, out and switching accounts (app bar polling).
const broadcaster = computed(() => {
  const status = twitchOverview.value?.status
  return status?.state === 'signed_in' ? { id: status.user_id, login: status.login } : undefined
})

const matches = (text: string) => !query.value.trim() || text.toLowerCase().includes(query.value.trim().toLowerCase())
const shownLog = computed(() => {
  const login = broadcaster.value?.login.toLowerCase()
  const mention = new RegExp(`@${login}\\b`, 'i')
  const messages = log.value.messages.filter(
    (m) =>
      matches(`${m.payload.userName} ${m.payload.text}`) && (filter.value !== 'mentions' || (!!login && mention.test(m.payload.text))),
  )
  return { ...log.value, messages }
})
const shownEvents = computed(() => [...events.value].reverse().filter((e) => matches(e.title)))

async function save() {
  try {
    await saveDock()
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}

function close() {
  if (wide.value) {
    dock.open = false
    void save()
  } else dock.sheet = false
  document.querySelector<HTMLElement>('[aria-controls="chat-dock"]')?.focus()
}

// Resizing: drag the edge facing the pages, or focus it and use ←/→ (Shift for bigger steps); the arrows move the
// edge the way they point, so they swap with the side. Saved when it ends.
function startResize(e: PointerEvent) {
  followDrag(
    e,
    (ev) => (dock.width = clampWidth(dock.side === 'left' ? ev.clientX : window.innerWidth - ev.clientX)),
    () => void save(),
  )
}
function resizeByKey(e: KeyboardEvent) {
  const step = e.shiftKey ? 64 : 16
  const delta = (dock.side === 'left' ? { ArrowLeft: -step, ArrowRight: step } : { ArrowLeft: step, ArrowRight: -step })[e.key]
  if (!delta) return
  e.preventDefault()
  dock.width = clampWidth(shownPx.value + delta)
  void save()
}

// The slide-in panel takes focus when it opens and closes with Esc.
const panel = ref<HTMLElement>()
watch(
  () => dock.sheet,
  (open) => open && void nextTick(() => panel.value?.focus()),
)
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && dock.sheet && !wide.value && !e.defaultPrevented) close()
}

onMounted(() => {
  wideQuery.addEventListener('change', onWide)
  window.addEventListener('resize', onResize)
  document.addEventListener('keydown', onKeydown)
})
onBeforeUnmount(() => {
  wideQuery.removeEventListener('change', onWide)
  window.removeEventListener('resize', onResize)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<style scoped>
.dock {
  container: dock / inline-size;
  position: sticky;
  top: 0;
  flex: none;
  display: flex;
  flex-direction: column;
  height: 100vh;
  border-left: 1px solid var(--p-surface-700);
  background: var(--p-surface-900);
}

.dock:focus {
  outline: none;
}

.dock.sheet {
  position: fixed;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 8;
  width: min(100%, 24rem);
  height: auto;
  box-shadow: -1rem 0 2rem rgb(0 0 0 / 0.6);
}

.resize {
  position: absolute;
  top: 0;
  bottom: 0;
  left: -5px;
  z-index: 1;
  width: 9px;
  cursor: col-resize;
  touch-action: none;
}

/* Docked on the left: the border and the resize edge face the pages on the right. */
.dock.left {
  order: -1;
  border-left: none;
  border-right: 1px solid var(--p-surface-700);
}

.dock.left .resize {
  left: auto;
  right: -5px;
}

.resize:hover,
.resize:focus-visible {
  background: linear-gradient(to right, transparent 4px, var(--p-primary-color) 4px, var(--p-primary-color) 6px, transparent 6px);
  outline: none;
}

.head {
  position: relative;
  display: flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.5rem 0.5rem 0.5rem 0.875rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.head h2 {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  margin: 0 auto 0 0;
  font-size: 1rem;
  white-space: nowrap;
}

.head h2 > i {
  color: var(--p-blue-400);
}

.head :deep(.p-button) {
  flex: none;
  width: 2rem;
  height: 2rem;
}

/* Five header buttons need ~310px; a narrower dock drops the title icon to keep them on one row. */
@container dock (max-width: 320px) {
  .head h2 > i {
    display: none;
  }
}

.tools {
  display: grid;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.filters {
  display: flex;
  gap: 0.25rem;
  overflow-x: auto;
  scrollbar-width: none;
}

.filters button {
  flex: none;
  padding: 0.125rem 0.625rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 999px;
  background: none;
  color: var(--p-text-muted-color);
  font: inherit;
  font-size: 0.8125rem;
  white-space: nowrap;
  cursor: pointer;
}

.filters button[aria-pressed='true'] {
  border-color: var(--p-primary-color);
  color: var(--p-primary-color);
}

.filters button:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.events {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  padding: 0.5rem 0.75rem;
  list-style: none;
}

.events li {
  display: flex;
  gap: 0.5rem;
  padding: 0.25rem 0;
  overflow-wrap: anywhere;
}

.events i {
  margin-top: 0.25rem;
  color: var(--p-yellow-400);
}

.send {
  display: flex;
  gap: 0.375rem;
  padding: 0.625rem 0.75rem;
  border-top: 1px solid var(--p-surface-800);
}

.send :deep(.p-inputtext) {
  flex: 1;
  min-width: 0;
}
</style>
