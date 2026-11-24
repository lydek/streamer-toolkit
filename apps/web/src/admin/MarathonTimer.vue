<template>
  <FormPanel class="marathon-panel">
    <template #head>
      <div class="panel-header remaining">
        <span class="badge tint-primary" aria-hidden="true"><i class="pi pi-stopwatch"></i></span>
        <div class="title">
          <h2>{{ t('counter.marathon.heading') }}</h2>
          <span v-if="status" class="state">{{ t(stateKey) }}</span>
        </div>
        <output class="clock">{{ status ? clockText(remaining) : t('app.loading') }}</output>
      </div>
    </template>

    <FormRow :label="t('counter.marathon.sub')" input-id="marathon-sub" end>
      <span class="unit-field">
        <span aria-hidden="true">+</span>
        <InputNumber
          v-model="sub.value.value"
          input-id="marathon-sub"
          :min="0"
          :max="1440"
          :allow-empty="false"
          :use-grouping="false"
          :aria-label="t('counter.marathon.subMinutes')"
          :disabled="!status"
          @keydown.enter="sub.commit()"
          @blur="sub.commit()"
        />
        <span>{{ t('counter.marathon.minutesUnit') }}</span>
      </span>
    </FormRow>
    <FormRow :label="t('counter.marathon.bits')" input-id="marathon-bits" end>
      <span class="unit-field">
        <span aria-hidden="true">+</span>
        <InputNumber
          v-model="bits.value.value"
          input-id="marathon-bits"
          :min="0"
          :max="1440"
          :allow-empty="false"
          :use-grouping="false"
          :aria-label="t('counter.marathon.bitsMinutes')"
          :disabled="!status"
          @keydown.enter="bits.commit()"
          @blur="bits.commit()"
        />
        <span>{{ t('counter.marathon.minutesUnit') }}</span>
      </span>
    </FormRow>
    <FormRow :label="t('counter.marathon.max')" input-id="marathon-hours" end>
      <span class="unit-field">
        <InputNumber
          v-model="hours.value.value"
          input-id="marathon-hours"
          :min="1"
          :max="168"
          :allow-empty="false"
          :use-grouping="false"
          :aria-label="t('counter.marathon.maxHours')"
          :disabled="!status"
          @keydown.enter="hours.commit()"
          @blur="hours.commit()"
        />
        <span>{{ t('counter.marathon.hoursUnit') }}</span>
      </span>
    </FormRow>

    <template #foot>
      <Button
        :label="t(status?.paused ? 'counter.marathon.resume' : 'counter.marathon.pause')"
        :icon="status?.paused ? 'pi pi-play' : 'pi pi-pause'"
        severity="secondary"
        outlined
        :disabled="!status"
        @click="pause(!status?.paused)"
      />
      <form class="add" @submit.prevent="add">
        <label for="marathon-add-minutes" class="sr-only">{{ t('counter.marathon.addMinutes') }}</label>
        <InputNumber
          v-model="addMinutes"
          input-id="marathon-add-minutes"
          :min="-1440"
          :max="1440"
          :use-grouping="false"
        />
        <span class="unit-text">{{ t('counter.marathon.minutesUnit') }}</span>
        <Button type="submit" :label="t('counter.marathon.add')" icon="pi pi-plus" severity="secondary" outlined :disabled="!status || !addMinutes" />
      </form>
      <ObsStatusLink class="obs-link" overlay="/overlay/marathon/" />
    </template>
  </FormPanel>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { remainingMs } from '../overlay/marathon/marathon'
import { type Schemas, api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import { clockText } from './home'
import ObsStatusLink from './ObsStatusLink.vue'

const props = defineProps<{
  /** The latest `counter.marathon` event; `undefined` until the live connection has it. */
  event?: Schemas['MarathonEvent']
}>()
const emit = defineEmits<{ error: [unknown] }>()
const { t } = useI18n()

const status = computed(() => props.event?.payload)
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 500)
onBeforeUnmount(() => clearInterval(ticker))
const remaining = computed(() => (props.event ? remainingMs(props.event.payload, props.event.occurredAt, now.value) : 0))
const stateKey = computed(() => {
  const s = status.value
  if (s?.paused) return 'counter.marathon.paused'
  if (!s?.live) return 'counter.marathon.waiting'
  return s.running ? 'counter.marathon.running' : 'counter.marathon.done'
})

// The options as saved; PUT takes all three, so each save sends the others as they are now.
const subSaved = ref(0)
const bitsSaved = ref(0)
const hoursSaved = ref(24)
watch(
  status,
  (s) => {
    if (!s) return
    subSaved.value = s.subMinutes
    bitsSaved.value = s.bitsMinutes
    hoursSaved.value = s.maxHours
  },
  { immediate: true },
)
const saveOptions = (patch: Partial<Schemas['MarathonOptionsBody']>) =>
  api(
    client.PUT('/api/admin/counters/marathon/options', {
      body: { sub_minutes: subSaved.value, bits_minutes: bitsSaved.value, max_hours: hoursSaved.value, ...patch },
    }),
  )
const queue = settingQueue()
const sub = useDrawerSetting(subSaved, (v) => saveOptions({ sub_minutes: v }), queue)
const bits = useDrawerSetting(bitsSaved, (v) => saveOptions({ bits_minutes: v }), queue)
const hours = useDrawerSetting(hoursSaved, (v) => saveOptions({ max_hours: v }), queue)

async function pause(paused: boolean) {
  try {
    await api(client.POST('/api/admin/counters/marathon/pause', { body: { paused } }))
  } catch (e) {
    emit('error', e)
  }
}

const addMinutes = ref<number | null>(10)
async function add() {
  if (!addMinutes.value) return
  try {
    await api(client.POST('/api/admin/counters/marathon/add', { body: { minutes: addMinutes.value } }))
  } catch (e) {
    emit('error', e)
  }
}
</script>

<style scoped>
.marathon-panel {
  max-width: 52rem;
  margin-top: 0.75rem;
}

.panel-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.75rem;
  padding: 0.875rem 1rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.badge {
  display: grid;
  place-items: center;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 6px;
  font-size: 1.0625rem;
}

.title h2 {
  margin: 0;
  font-size: 1.0625rem;
}

.state {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.clock {
  margin-left: auto;
  font-size: 1.75rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.unit-field {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.unit-field :deep(.p-inputnumber-input),
.add :deep(.p-inputnumber-input) {
  width: 4.5rem;
}

.add {
  flex-direction: row;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.375rem;
  max-width: none;
}

.unit-text {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.obs-link {
  margin-left: auto;
}
</style>
