<template>
  <!-- One audio input: fader and dB, mute, monitoring. The state shown is OBS's; a change shows once OBS has made it. -->
  <li class="mix-row" :class="{ muted: input.muted }">
    <div class="mix-grid">
    <span class="mix-name">
      <b>{{ input.name }}</b>
      <small class="muted" :title="input.global ? undefined : input.scenes?.join('、')">{{ where }}</small>
    </span>
    <input
      type="range"
      :min="FADER_MIN"
      max="0"
      step="0.5"
      :value="draft ?? Math.max(FADER_MIN, input.volumeDb ?? 0)"
      :aria-label="t('mixer.volume', { input: input.name })"
      :aria-valuetext="dbText"
      @input="draft = Number(($event.target as HTMLInputElement).value)"
      @change="setVolume"
    />
    <output class="mix-db">{{ dbText }}</output>
    <Button
      :icon="input.muted ? 'pi pi-volume-off' : 'pi pi-volume-up'"
      :severity="input.muted ? 'danger' : 'secondary'"
      text
      rounded
      :aria-pressed="input.muted"
      :aria-label="t('mixer.mute', { input: input.name })"
      @click="change({ muted: !input.muted })"
    />
    <Select
      v-if="!compact"
      :model-value="input.monitor ?? 'off'"
      :options="MONITORS"
      :option-label="(m: Monitor) => t(`mixer.monitor.${m}`)"
      :aria-label="t('mixer.monitorOf', { input: input.name })"
      @update:model-value="(m: Monitor) => change({ monitor: m })"
    />
    </div>
  </li>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import { errorToast } from './home'
import { FADER_MIN } from './mixer'

type Monitor = Schemas['Monitor']
const MONITORS: Monitor[] = ['off', 'monitor_only', 'monitor_and_output']

const props = defineProps<{
  input: Schemas['Input']
  /** The drawer's row: no monitoring select. */
  compact?: boolean
}>()

const { t } = useI18n()
const toast = useToast()
// The fader while it is dragged; OBS's value again once the change is sent.
const draft = ref<number>()
const db = computed(() => draft.value ?? props.input.volumeDb ?? 0)
const dbText = computed(() => (db.value <= FADER_MIN ? t('mixer.silent') : `${db.value.toFixed(1)} dB`))
const where = computed(() => {
  const i = props.input
  if (i.global) return t('mixer.global')
  const n = i.scenes?.length ?? 0
  return n ? t('mixer.scenes', { n }) : t('mixer.noScene')
})

async function change(body: { volume_db?: number; muted?: boolean; monitor?: Monitor }) {
  try {
    await api(client.PUT('/api/admin/obs/input', { body: { input: props.input.name, ...body } }))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  }
}

async function setVolume() {
  if (draft.value === undefined) return
  // The floor is OBS's −∞: silent.
  await change({ volume_db: draft.value <= FADER_MIN ? -100 : draft.value })
  draft.value = undefined
}
</script>

<style scoped>
/* Narrow (the drawer, a narrow page): name, dB, mute and monitoring on top, the fader under them across the row;
   wide enough: all on one line. */
.mix-row {
  container-type: inline-size;
}

.mix-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto auto;
  grid-template-areas:
    'name db mute monitor'
    'fader fader fader fader';
  align-items: center;
  gap: 0.25rem 0.5rem;
  padding: 0.25rem 0;
}

@container (min-width: 40rem) {
  .mix-grid {
    grid-template-columns: minmax(8rem, 1fr) minmax(8rem, 2fr) 4.5rem auto auto;
    grid-template-areas: 'name fader db mute monitor';
    gap: 0.25rem 0.75rem;
  }
}

.mix-name {
  grid-area: name;
}

.mix-fader {
  grid-area: fader;
}

.mix-db {
  grid-area: db;
}

.mix-grid > .p-button {
  grid-area: mute;
}

.mix-grid > .p-select {
  grid-area: monitor;
  max-width: 12rem;
}

.mix-name {
  display: grid;
  min-width: 0;
}

.mix-name b {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mix-db {
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.mix-row.muted .mix-name b {
  color: var(--p-text-muted-color);
}
</style>
