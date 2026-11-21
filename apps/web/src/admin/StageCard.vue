<template>
  <!-- The stream stage on home: before, live or after, following Twitch; picking another stage holds until the next
       change from Twitch. -->
  <section class="stage" aria-labelledby="stage-heading">
    <div class="stage-head">
      <h2 id="stage-heading" class="sr-only">{{ t('stage.title') }}</h2>
      <SelectButton
        v-model="shown"
        :options="STAGES"
        :option-label="(s: Stage) => t(`stage.${s}`)"
        :allow-empty="false"
        aria-labelledby="stage-heading"
      />
    </div>
    <template v-if="shown === 'before'">
      <p class="ready">{{ t('stage.checks.ready', { ok: checks.filter((c) => c.ok).length, all: checks.length }) }}</p>
      <ul class="plain checks">
        <li v-for="c in checks" :key="c.id" :class="{ bad: !c.ok }">
          <i :class="c.ok ? 'pi pi-check-circle' : 'pi pi-exclamation-triangle'" aria-hidden="true"></i>
          <span>{{ t(`stage.checks.${c.id}.${c.ok ? 'ok' : 'bad'}`) }}</span>
          <!-- An overlay is fixed by adding its URL to OBS as a browser source: copy it right here. -->
          <Button
            v-if="!c.ok && c.id.startsWith('overlay.')"
            :label="t('chat.copyObsUrl')"
            icon="pi pi-copy"
            text
            size="small"
            class="fix"
            @click="copyOverlayUrl(c.id.slice('overlay.'.length))"
          />
          <RouterLink v-else-if="!c.ok" :to="c.fix" class="fix">{{ t('stage.checks.fix') }}</RouterLink>
        </li>
      </ul>
      <p v-if="copyNotice" :role="copyNotice.error ? 'alert' : 'status'">{{ copyNotice.text }}</p>
    </template>
    <p v-else-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="shown === 'live' && live.stream?.state !== 'live'" class="muted">{{ t('stage.notLive') }}</p>
    <p v-else-if="!session" class="muted">{{ shown === 'live' ? t('stage.waiting') : t('stage.noStream') }}</p>
    <template v-else>
      <p class="line">{{ line }}</p>
      <ul class="plain stats">
        <li v-for="s in stats" :key="s.key">
          <span class="value">{{ s.value.toLocaleString('zh-TW') }}</span>
          <span class="label">{{ t(`stage.stats.${s.key}`) }}</span>
        </li>
      </ul>
      <div v-if="session.endedAt" class="export">
        <Button :label="t('record.export')" icon="pi pi-download" outlined :disabled="exporting" @click="exportIt" />
        <p v-if="exportNotice" :role="exportNotice.error ? 'alert' : 'status'">{{ exportNotice.text }}</p>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import SelectButton from 'primevue/selectbutton'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { type LiveState, type Stage, clockText, preStreamChecks, sessionRange, streamStage, twitchOverview } from './home'
import { exportRecord } from './streamRecord'

const STAGES: Stage[] = ['before', 'live', 'after']
// Follows, subs and the rest arrive between status events; read them again this often while live.
const REFRESH_MS = 30_000
// How often 開播前 asks which overlays are open in OBS.
const OVERLAYS_MS = 3000

const props = defineProps<{ live: LiveState; modules?: Schemas['ModuleState'][] }>()
const { t } = useI18n()

const summary = shallowRef<Schemas['StreamSummary'] | null>()
const failed = ref('')
async function load() {
  try {
    summary.value = await api(client.GET('/api/admin/stream/summary'))
    failed.value = ''
  } catch (e) {
    failed.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  }
}

const auto = computed(() => streamStage(props.live.stream, summary.value))
const picked = ref<Stage>()
const shown = computed({ get: () => picked.value ?? auto.value, set: (s) => (picked.value = s) })
watch(auto, () => (picked.value = undefined))

// A start, an end or a sign-in changes which session is the latest: read it again.
watch(
  [() => props.live.stream?.state, () => props.live.stream?.startedAt],
  () => void load(),
  { immediate: true },
)
const now = ref(Date.now())
const clock = window.setInterval(() => (now.value = Date.now()), 1000)
const refresh = window.setInterval(() => {
  if (props.live.stream?.state === 'live') void load()
}, REFRESH_MS)

// The app bar keeps the Twitch status current; OBS comes with the live events; overlays are asked for here.
const overlays = shallowRef<Schemas['OverlayCounts']>()
async function loadOverlays() {
  if (shown.value !== 'before') return
  overlays.value = await api(client.GET('/api/admin/overlays')).catch(() => undefined)
}
watch(shown, () => void loadOverlays(), { immediate: true })
const overlayTimer = window.setInterval(() => void loadOverlays(), OVERLAYS_MS)
const checks = computed(() => preStreamChecks(twitchOverview.value, props.live, props.modules, overlays.value))

onBeforeUnmount(() => {
  window.clearInterval(clock)
  window.clearInterval(refresh)
  window.clearInterval(overlayTimer)
})

// 直播中 shows the live session; 下播後 the last ended one (a live session has no summary yet).
const session = computed(() => {
  const s = summary.value
  if (!s) return undefined
  return (shown.value === 'live') === !s.endedAt ? s : undefined
})
const line = computed(() => {
  const s = session.value!
  const peak = s.peakViewers == null ? '' : ` · ${t('stage.peak', { n: s.peakViewers })}`
  const when = s.endedAt ? sessionRange(s.startedAt, s.endedAt) : clockText(now.value - Date.parse(s.startedAt))
  return when + peak
})
const copyNotice = ref<{ text: string; error: boolean }>()
async function copyOverlayUrl(kind: string) {
  try {
    // The admin window loads the Host URL, so its origin is what OBS needs.
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/${kind}/`)
    copyNotice.value = { text: t('stage.checks.copied'), error: false }
  } catch {
    copyNotice.value = { text: t('chat.copyFailed'), error: true }
  }
}

const exporting = ref(false)
const exportNotice = ref<{ text: string; error: boolean }>()
async function exportIt() {
  exporting.value = true
  exportNotice.value = undefined
  try {
    if (await exportRecord(session.value!.startedAt)) exportNotice.value = { text: t('record.exported'), error: false }
  } catch (e) {
    exportNotice.value = { text: e instanceof ApiError ? errorMessage(e.code) : String(e), error: true }
  } finally {
    exporting.value = false
  }
}

const stats = computed(() => {
  const s = session.value!
  return [
    { key: 'follows', value: s.follows },
    { key: 'subs', value: s.subs },
    { key: 'bits', value: s.bits },
    { key: 'raids', value: s.raids },
    { key: 'redemptions', value: s.redemptions },
    { key: 'chatMessages', value: s.chatMessages },
    { key: 'firstChats', value: s.firstChats },
  ]
})
</script>

<style scoped>
.stage {
  display: grid;
  gap: 0.5rem;
}

.muted,
.label {
  color: var(--p-text-muted-color);
}

.line {
  margin: 0;
  font-variant-numeric: tabular-nums;
}

.ready {
  margin: 0;
}

.checks {
  display: grid;
  gap: 0.25rem;
}

.checks li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.checks .pi-check-circle {
  color: var(--p-green-400);
}

.checks .bad .pi {
  color: var(--p-yellow-400);
}

.fix {
  margin-left: auto;
}

.export {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.export p {
  margin: 0;
}

.stats {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.5rem;
}

.stats li {
  display: grid;
}

.value {
  font-size: 1.25rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.label {
  font-size: 0.8125rem;
}
</style>
