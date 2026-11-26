<template>
  <FullPage tile="chat">
    <p v-if="twitchOverview?.chat_missed_writes" role="alert">{{ t('chat.missedWrites', { n: twitchOverview.chat_missed_writes }) }}</p>
    <p v-if="twitchOverview?.chat_write_delayed" role="alert">{{ t('chat.writeDelayed') }}</p>
    <p v-if="periodKey === LIVE && historyFailed" role="alert">{{ t('chat.historyRetry') }}</p>
    <p v-if="failed" role="alert">{{ failed }}</p>
    <section class="chatpage">
      <div class="controls">
        <InputText v-model="query" class="search" type="search" :aria-label="t('chatLog.search')" :placeholder="t('chatLog.search')" />
        <label for="chat-period" class="sr-only">{{ t('chatLog.period') }}</label>
        <Select v-model="periodKey" input-id="chat-period" :options="periodOptions" option-label="label" option-value="value" />
        <span class="check">
          <Checkbox v-model="hideBots" input-id="hide-bots" binary />
          <label for="hide-bots">{{ t('chatLog.hideBots') }}</label>
        </span>
        <span class="count">{{ t('chatLog.count', { n: shown.length }) }}</span>
        <ChatDisplayMenu popover />
        <Button
          :label="t('chatLog.openTwitch')"
          icon="pi pi-external-link"
          severity="secondary"
          outlined
          size="small"
          :disabled="!broadcaster"
          @click="openTwitch"
        />
      </div>
      <div class="filters" role="group" :aria-label="t('chatLog.filters')">
        <Button
          v-for="f in LOG_FILTERS"
          :key="f"
          :label="t(`chatLog.filter.${f}`)"
          size="small"
          :severity="filter === f ? undefined : 'secondary'"
          :outlined="filter !== f"
          :aria-pressed="filter === f"
          @click="filter = f"
        />
      </div>
      <p v-if="partners.length" class="shared">
        {{ t('chat.shared', { channels: partners.join('、') }) }}
        <Button :label="t('chat.ownOnly')" size="small" text :aria-pressed="ownOnly" @click="ownOnly = !ownOnly" />
      </p>
      <p v-if="truncated" class="muted note">{{ t('chatLog.truncated') }}</p>
      <p v-if="loading" class="muted note">{{ t('app.loading') }}</p>
      <p v-else-if="!shown.length" class="muted note">{{ t('chatLog.empty') }}</p>
      <!-- Viewer input only ever goes in as text ({{ }}), never as markup. -->
      <ol v-else class="plain lines" :aria-label="t('chatLog.lines')" :style="{ fontSize: `${chatDisplay.textSize}%` }">
        <li v-for="(e, i) in shown" :key="e.kind === 'message' ? e.message.id : `e${i}`" :class="e.kind">
          <ChatLine v-if="e.kind === 'message'" :message="e.message" :broadcaster-id="broadcasterId" />
          <template v-else>
            <time v-if="chatDisplay.time" :datetime="e.at">{{ clockText(e.at) }}</time>
            <span class="event">{{ eventText(e.event) }}</span>
          </template>
        </li>
      </ol>
      <ChatComposer class="composer" />
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import ChatComposer from './ChatComposer.vue'
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import { applyChat, fromStored, useChatHistory, EMPTY_LOG, type ChatUpdate } from './chatLog'
import {
  type LogEvent,
  type LogFilter,
  LOG_FILTERS,
  clockText,
  dayText,
  filterEntries,
  fromAlert,
  merge,
} from './chatLogView'
import { chatDisplay, loadBadges, loadChatDisplay, sharedChannels, wantAvatars } from './chatDisplay'
import ChatDisplayMenu from './ChatDisplayMenu.vue'
import ChatLine from './ChatLine.vue'
import FullPage from './FullPage.vue'
import { errorToast, twitchOverview, useLiveEvents } from './home'

const LIVE = 'live'

const { t } = useI18n()
const toast = useToast()
const filter = ref<LogFilter>('all')
const query = ref('')
const hideBots = ref(false)
// The Host's chat bot list, shared with the leaderboard.
const bots = shallowRef<ReadonlySet<string>>(new Set())
const failed = ref('')

const broadcaster = computed(() => {
  const status = twitchOverview.value?.status
  return status?.state === 'signed_in' ? status.login : undefined
})
const broadcasterId = computed(() => {
  const status = twitchOverview.value?.status
  return status?.state === 'signed_in' ? status.user_id : undefined
})
void loadChatDisplay().catch(() => {})
void api(client.GET('/api/admin/chat/bots'))
  .then((list) => (bots.value = new Set(list)))
  .catch(() => {})
watch(broadcasterId, loadBadges, { immediate: true })

// 即時: the stored chat before this page opened, then the live messages and the alerts heard since.
const { log: live, accept, historyFailed } = useChatHistory()
const liveAlerts = shallowRef<LogEvent[]>([])
const periods = shallowRef<Schemas['ChatPeriod'][]>([])
let periodRequest = 0
async function refreshPeriods() {
  const version = ++periodRequest
  try {
    const rows = await api(client.GET('/api/admin/chat/periods'))
    if (version === periodRequest) periods.value = rows
  } catch (e) {
    if (version === periodRequest) failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}
const redeemed = shallowRef<LogEvent[]>([])
const openedAt = new Date().toISOString()
let redemptionRequest = 0
async function refreshRedemptions() {
  const request = ++redemptionRequest
  try {
    const page = await api(client.GET('/api/admin/redemptions', { params: { query: { from: openedAt, limit: 100 } } }))
    if (request === redemptionRequest) redeemed.value = page.items.map((row) => ({
      at: row.redeemed_at, kind: 'redemption', userName: row.user_name, reward: row.reward_title, message: row.user_input || undefined,
    }))
  } catch (e) {
    if (request === redemptionRequest) failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}
const snapshot = shallowRef<Extract<Incoming, { type: 'snapshot' }>>()
let offlineRefresh: ReturnType<typeof setTimeout> | undefined
let disposed = false
let sawOfflineChat = false
const connection = useLiveEvents(2000, (msg) => {
  accept(msg)
  if (msg.type === 'snapshot') {
    snapshot.value = msg
    if (msg.payload['chat.message'].some((m) => m.source === 'twitch')) refreshAfterOfflineChat()
  }
  if (msg.type === 'chat.message' && msg.source === 'twitch') refreshAfterOfflineChat()
  if (msg.type === 'redemption.changed' || msg.type === 'snapshot') void refreshRedemptions()
  if (msg.type === 'alert.received') liveAlerts.value = [...liveAlerts.value, fromAlert(msg)].slice(-200)
  if (msg.type === 'chat.removed' || msg.type === 'chat.first') {
    pendingUpdates?.push(msg)
    if (past.value) past.value = updatePast(past.value, [msg])
  }
})
watch([() => connection.value.status, () => connection.value.stream?.state, () => connection.value.stream?.startedAt], () => {
  if (connection.value.stream?.state === 'live') sawOfflineChat = false
  if (connection.value.status !== 'failed') void refreshPeriods()
}, { immediate: true })
function refreshAfterOfflineChat() {
  if (connection.value.stream?.state === 'live') return
  sawOfflineChat = true
  scheduleOfflineRefresh()
}
function scheduleOfflineRefresh(remaining = 3) {
  if (disposed || !remaining || offlineRefresh || connection.value.stream?.state === 'live'
    || periods.value.some((p) => p.kind === 'offline' && p.to === null)) return
  offlineRefresh = setTimeout(async () => {
    if (connection.value.stream?.state !== 'live') await refreshPeriods()
    offlineRefresh = undefined
    scheduleOfflineRefresh(remaining - 1)
  }, 2000)
}
watch(() => twitchOverview.value?.chat_write_delayed, (delayed, wasDelayed) => {
  if (wasDelayed && !delayed && sawOfflineChat) scheduleOfflineRefresh()
})
onScopeDispose(() => {
  disposed = true
  clearTimeout(offlineRefresh)
})
const periodId = (p: Schemas['ChatPeriod']) => p.kind + '/' + (p.from ?? '')
// Two periods on one day differ by their time.
const dayClock = (at: string) => `${dayText(at)} ${clockText(at)}`
const periodKey = ref(LIVE)
const periodOptions = computed(() => [
  { value: LIVE, label: t('chatLog.live') },
  ...periods.value.map((p) => ({
    value: periodId(p),
    label:
      p.kind === 'stream'
        ? t('chatLog.stream', { day: dayClock(p.from!) })
        : t('chatLog.offline', { day: dayClock(p.firstAt ?? p.from ?? new Date().toISOString()) }),
  })),
])

const past = shallowRef<Schemas['ChatLogBody']>()
const loading = ref(false)
let pendingUpdates: ChatUpdate[] | undefined
function updatePast(body: Schemas['ChatLogBody'], updates: ChatUpdate[]) {
  if (!updates.length) return body
  const log = updates.reduce((s, msg) => applyChat(s, msg), { ...EMPTY_LOG, messages: body.messages.map(fromStored) })
  return { ...body, messages: body.messages.map((m, i) => ({ ...m, payload: log.messages[i].payload, deleted: !!log.messages[i].deleted })) }
}
const selected = computed(() => periods.value.find((p) => periodId(p) === periodKey.value))
watch([periodKey, () => selected.value?.to, snapshot], async (_, __, cleanup) => {
  let current = true
  cleanup(() => { current = false })
  const changes: ChatUpdate[] = [...(snapshot.value?.payload['chat.removed'] ?? [])]
  const first = new Set(snapshot.value?.payload['chat.message'].filter((m) => m.payload.firstChat).map((m) => m.id))
  pendingUpdates = changes
  past.value = undefined
  failed.value = ''
  loading.value = !!selected.value
  const p = selected.value
  if (!p) { pendingUpdates = undefined; return }
  try {
    const query = { from: p.from ?? undefined, to: p.to ?? undefined }
    const body = await api(client.GET('/api/admin/chat/log', { params: { query } }))
    if (current) {
      const messages = first.size ? body.messages.map((m) => first.has(m.id)
        ? { ...m, payload: { ...m.payload, firstChat: true } }
        : m) : body.messages
      past.value = updatePast({ ...body, messages }, changes)
    }
  } catch (e) {
    if (current) failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  } finally {
    if (current) { loading.value = false; pendingUpdates = undefined }
  }
})

const entries = computed(() =>
  periodKey.value === LIVE
    ? merge(live.value.messages, [...liveAlerts.value, ...redeemed.value])
    : merge((past.value?.messages ?? []).map(fromStored), past.value?.events ?? []),
)
const truncated = computed(() => periodKey.value !== LIVE && !!past.value?.truncated)
const messages = computed(() => entries.value.flatMap((e) => (e.kind === 'message' ? [e.message] : [])))
watch(messages, (m) => wantAvatars(m))
// A shared chat: the other channels taking part, and 只看本頻道 to hide their messages.
const partners = computed(() => sharedChannels(messages.value))
const ownOnly = ref(false)
const shown = computed(() =>
  filterEntries(entries.value, filter.value, query.value, hideBots.value ? bots.value : new Set(), broadcaster.value).filter(
    (e) => !(ownOnly.value && e.kind === 'message' && e.message.payload.sharedFrom),
  ),
)

function eventText(e: LogEvent): string {
  return t(`chatLog.event.${e.kind}`, {
    name: e.userName ?? t('alerts.anonymous'),
    months: e.months,
    count: e.count,
    bits: e.bits,
    viewers: e.viewers,
    reward: e.reward,
  })
}

async function openTwitch() {
  try {
    await invoke('open_twitch_chat', { login: broadcaster.value })
  } catch (code) {
    toast.add(errorToast(errorMessage(String(code))))
  }
}
</script>

<style scoped>
.chatpage {
  display: flex;
  flex-direction: column;
  margin-top: 0.75rem;
  border: 1px solid var(--p-surface-800);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.controls,
.filters {
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.controls .search {
  flex: 1 1 14rem;
  min-width: 10rem;
}

.count {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
  font-variant-numeric: tabular-nums;
}

.shared {
  margin: 0;
  padding: 0.5rem 1rem;
  border-bottom: 1px solid var(--p-surface-800);
  color: var(--p-text-muted-color);
}

.note {
  margin: 0;
  padding: 0.75rem 1rem;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.muted,
time {
  color: var(--p-text-muted-color);
}

.lines {
  display: grid;
  gap: 0.25rem;
  max-height: 65vh;
  margin: 0;
  padding: 0.625rem 1rem;
  overflow-y: auto;
  list-style: none;
}

.lines li {
  overflow-wrap: anywhere;
}

time {
  margin-right: 0.5rem;
  font-variant-numeric: tabular-nums;
}

.event {
  color: var(--p-primary-color);
}
.composer {
  padding: 0.75rem 1rem;
  border-top: 1px solid var(--p-surface-800);
}
</style>
