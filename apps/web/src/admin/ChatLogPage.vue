<template>
  <FullPage :title="t('pages.chatLog.name')">
    <p v-if="failed" role="alert">{{ failed }}</p>
    <div class="controls">
      <label for="chat-period" class="sr-only">{{ t('chatLog.period') }}</label>
      <Select v-model="periodKey" input-id="chat-period" :options="periodOptions" option-label="label" option-value="value" />
      <InputText v-model="query" type="search" :aria-label="t('chatLog.search')" :placeholder="t('chatLog.search')" />
      <span class="check">
        <Checkbox v-model="hideBots" input-id="hide-bots" binary />
        <label for="hide-bots">{{ t('chatLog.hideBots') }}</label>
      </span>
      <Button
        :label="t('chatLog.openTwitch')"
        icon="pi pi-external-link"
        text
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
    <p v-if="truncated" class="muted">{{ t('chatLog.truncated') }}</p>
    <p v-if="loading" class="muted">{{ t('app.loading') }}</p>
    <p v-else-if="!shown.length" class="muted">{{ t('chatLog.empty') }}</p>
    <!-- Viewer input only ever goes in as text ({{ }}), never as markup. -->
    <ol v-else class="plain lines" :aria-label="t('chatLog.lines')">
      <li v-for="(e, i) in shown" :key="e.kind === 'message' ? e.message.id : `e${i}`" :class="e.kind">
        <time :datetime="e.at">{{ clockText(e.at) }}</time>
        <template v-if="e.kind === 'message'">
          <span class="name">{{ e.message.payload.userName }}</span
          >：<span v-if="e.message.deleted" class="deleted">{{ t('chat.deleted') }}</span
          ><template v-else>{{ e.message.payload.text }}</template>
        </template>
        <span v-else class="event">{{ eventText(e.event) }}</span>
      </li>
    </ol>
  </FullPage>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { EMPTY_LOG, applyChat, fromStored, withHistory } from './chatLog'
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
import FullPage from './FullPage.vue'
import { errorToast, twitchOverview, useLiveEvents } from './home'

const LIVE = 'live'

const { t } = useI18n()
const toast = useToast()
const filter = ref<LogFilter>('all')
const query = ref('')
const hideBots = ref(false)
const failed = ref('')

const broadcaster = computed(() => {
  const status = twitchOverview.value?.status
  return status?.state === 'signed_in' ? status.login : undefined
})

// 即時: the stored chat before this page opened, then the live messages and the alerts heard since.
const live = shallowRef(EMPTY_LOG)
const liveAlerts = shallowRef<LogEvent[]>([])
useLiveEvents(2000, (msg) => {
  live.value = applyChat(live.value, msg)
  if (msg.type === 'alert.received') liveAlerts.value = [...liveAlerts.value, fromAlert(msg)]
})
void api(client.GET('/api/admin/chat/messages'))
  .then((stored) => (live.value = withHistory(live.value, stored)))
  .catch(() => {})

// Past periods: each stream, and the chat between streams.
const periods = shallowRef<Schemas['ChatPeriod'][]>([])
void api(client.GET('/api/admin/chat/periods'))
  .then((p) => (periods.value = p))
  .catch((e) => (failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)))
const periodKey = ref(LIVE)
const periodOptions = computed(() => [
  { value: LIVE, label: t('chatLog.live') },
  ...periods.value.map((p, i) => ({
    value: String(i),
    label:
      p.kind === 'stream'
        ? t('chatLog.stream', { day: dayText(p.from!) })
        : t('chatLog.offline', { day: dayText(p.firstAt ?? p.from ?? new Date().toISOString()) }),
  })),
])

const past = shallowRef<Schemas['ChatLogBody']>()
const loading = ref(false)
watch(periodKey, async (key) => {
  past.value = undefined
  if (key === LIVE) return
  const p = periods.value[Number(key)]
  loading.value = true
  try {
    const query = { from: p.from ?? undefined, to: p.to ?? undefined }
    past.value = await api(client.GET('/api/admin/chat/log', { params: { query } }))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  } finally {
    loading.value = false
  }
})

const entries = computed(() =>
  periodKey.value === LIVE
    ? merge(live.value.messages, liveAlerts.value)
    : merge((past.value?.messages ?? []).map(fromStored), past.value?.events ?? []),
)
const truncated = computed(() => periodKey.value !== LIVE && !!past.value?.truncated)
const shown = computed(() => filterEntries(entries.value, filter.value, query.value, hideBots.value, broadcaster.value))

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
.controls,
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.filters {
  margin-top: 0.5rem;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.muted,
time,
.deleted {
  color: var(--p-text-muted-color);
}

.deleted {
  font-style: italic;
}

.lines {
  display: grid;
  gap: 0.25rem;
  margin-top: 0.75rem;
}

.lines li {
  overflow-wrap: anywhere;
}

time {
  margin-right: 0.5rem;
  font-variant-numeric: tabular-nums;
}

.name {
  font-weight: 600;
}

.event {
  color: var(--p-primary-color);
}
</style>
