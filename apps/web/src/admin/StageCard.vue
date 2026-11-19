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
    <p v-if="failed" role="alert">{{ failed }}</p>
    <p v-else-if="shown === 'before' || (shown === 'live' && stream?.state !== 'live')" class="muted">
      {{ t('stage.notLive') }}
    </p>
    <p v-else-if="!session" class="muted">{{ shown === 'live' ? t('stage.waiting') : t('stage.noStream') }}</p>
    <template v-else>
      <p class="line">{{ line }}</p>
      <ul class="plain stats">
        <li v-for="s in stats" :key="s.key">
          <span class="value">{{ s.value.toLocaleString('zh-TW') }}</span>
          <span class="label">{{ t(`stage.stats.${s.key}`) }}</span>
        </li>
      </ul>
    </template>
  </section>
</template>

<script setup lang="ts">
import SelectButton from 'primevue/selectbutton'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { type Stage, clockText, sessionRange, streamStage } from './home'

const STAGES: Stage[] = ['before', 'live', 'after']
// Follows, subs and the rest arrive between status events; read them again this often while live.
const REFRESH_MS = 30_000

const props = defineProps<{ stream?: Schemas['StreamStatusPayload'] }>()
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

const auto = computed(() => streamStage(props.stream, summary.value))
const picked = ref<Stage>()
const shown = computed({ get: () => picked.value ?? auto.value, set: (s) => (picked.value = s) })
watch(auto, () => (picked.value = undefined))

// A start, an end or a sign-in changes which session is the latest: read it again.
watch(
  [() => props.stream?.state, () => props.stream?.startedAt],
  () => void load(),
  { immediate: true },
)
const now = ref(Date.now())
const clock = window.setInterval(() => (now.value = Date.now()), 1000)
const refresh = window.setInterval(() => {
  if (props.stream?.state === 'live') void load()
}, REFRESH_MS)
onBeforeUnmount(() => {
  window.clearInterval(clock)
  window.clearInterval(refresh)
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
const stats = computed(() => {
  const s = session.value!
  return [
    { key: 'follows', value: s.follows },
    { key: 'subs', value: s.subs },
    { key: 'bits', value: s.bits },
    { key: 'raids', value: s.raids },
    { key: 'redemptions', value: s.redemptions },
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
