<template>
  <!-- The docked chat panel: beside the pages on wide windows (resizable, saved), a slide-in panel below 900px. -->
  <aside
    v-if="shown"
    id="chat-dock"
    ref="panel"
    class="dock"
    :class="{ sheet: !wide }"
    :style="wide ? { width: `${dock.width}px` } : undefined"
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
      :aria-valuenow="dock.width"
      :aria-valuemin="DOCK_MIN"
      :aria-valuemax="DOCK_MAX"
      @pointerdown="startResize"
      @keydown="resizeByKey"
    ></div>
    <div class="head">
      <h2>{{ t('dock.title') }}</h2>
      <Button icon="pi pi-angle-double-right" text rounded :aria-label="t('dock.close')" @click="close" />
    </div>
    <div class="tools">
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
      <InputText v-model="query" type="search" size="small" :placeholder="t('dock.search')" :aria-label="t('dock.search')" />
    </div>
    <div class="body">
      <ul v-if="filter === 'events'" class="events" role="log" aria-live="off" :aria-label="t('dock.filter.events')">
        <li v-if="!shownEvents.length">{{ t('alerts.empty') }}</li>
        <!-- Viewer input only ever goes in as text. -->
        <li v-for="e in shownEvents" :key="e.id"><i class="pi pi-bolt" aria-hidden="true"></i>{{ e.title }}</li>
      </ul>
      <ChatNow
        v-else
        :log="shownLog"
        :broadcaster-id="broadcaster?.id"
        :disconnected="live.status === 'failed'"
        @update:log="(next) => (log = next.paused ? pause(log) : resume(log))"
      />
    </div>
    <!-- Sending needs the chat send channel (a later phase); the box shows where it will be. -->
    <div class="send">
      <InputText disabled :placeholder="t('dock.sendLater')" :aria-label="t('dock.send')" />
      <Button icon="pi pi-send" disabled :aria-label="t('dock.send')" />
    </div>
  </aside>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { alertTitle } from '../overlay/alerts/alerts'
import { ApiError } from './api'
import ChatNow from './ChatNow.vue'
import { EMPTY_LOG, applyChat, pause, resume } from './chatLog'
import { DOCK_MAX, DOCK_MIN, clampWidth, saveDock, useDock } from './dock'
import { useModules } from './features'
import { errorToast, twitchOverview, useLiveEvents } from './home'

const FILTERS = ['all', 'mentions', 'events', 'mods'] as const
const EVENT_LIMIT = 100

const { t } = useI18n()
const toast = useToast()
const dock = useDock()
const { hidden } = useModules()

const wideQuery = window.matchMedia('(min-width: 900px)')
const wide = ref(wideQuery.matches)
const onWide = () => {
  wide.value = wideQuery.matches
  dock.sheet = false
}
// Turning the chat feature off in settings removes the panel too.
const shown = computed(() => !hidden.value.has('/chat') && (wide.value ? dock.open : dock.sheet))

const filter = ref<(typeof FILTERS)[number]>('all')
const query = ref('')
const log = shallowRef(EMPTY_LOG)
const events = shallowRef<{ id: string; title: string }[]>([])
const live = useLiveEvents(2000, (msg) => {
  log.value = applyChat(log.value, msg)
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

// Resizing: drag the left edge, or focus it and use ←/→ (Shift for bigger steps). Saved when it ends.
function startResize(e: PointerEvent) {
  const edge = e.currentTarget as HTMLElement
  edge.setPointerCapture(e.pointerId)
  const move = (ev: PointerEvent) => (dock.width = clampWidth(window.innerWidth - ev.clientX))
  const up = () => {
    edge.removeEventListener('pointermove', move)
    void save()
  }
  edge.addEventListener('pointermove', move)
  edge.addEventListener('pointerup', up, { once: true })
}
function resizeByKey(e: KeyboardEvent) {
  const step = e.shiftKey ? 64 : 16
  const delta = { ArrowLeft: step, ArrowRight: -step }[e.key]
  if (!delta) return
  e.preventDefault()
  dock.width = clampWidth(dock.width + delta)
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
  document.addEventListener('keydown', onKeydown)
})
onBeforeUnmount(() => {
  wideQuery.removeEventListener('change', onWide)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<style scoped>
.dock {
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

.resize:hover,
.resize:focus-visible {
  background: linear-gradient(to right, transparent 4px, var(--p-primary-color) 4px, var(--p-primary-color) 6px, transparent 6px);
  outline: none;
}

.head {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.5rem 0.5rem 1rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.head h2 {
  margin: 0 auto 0 0;
  font-size: 1rem;
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
  overflow-y: auto;
  padding: 0.5rem 0.75rem;
}

.events {
  margin: 0;
  padding: 0;
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
