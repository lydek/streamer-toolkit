<template>
  <div class="stack">
    <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined class="copy" @click="emit('copy')" />
    <p class="remaining">
      <output>{{ status ? clockText(remaining) : t('app.loading') }}</output>
      <span v-if="status" class="state">{{ t(stateKey) }}</span>
    </p>
    <div class="fields">
      <div class="field">
        <label for="marathon-sub">{{ t('counter.marathon.subMinutes') }}</label>
        <InputNumber
          v-model="sub.value.value"
          input-id="marathon-sub"
          :min="0"
          :max="1440"
          :allow-empty="false"
          :use-grouping="false"
          :disabled="!status"
          @keydown.enter="sub.commit()"
          @blur="sub.commit()"
        />
      </div>
      <div class="field">
        <label for="marathon-bits">{{ t('counter.marathon.bitsMinutes') }}</label>
        <InputNumber
          v-model="bits.value.value"
          input-id="marathon-bits"
          :min="0"
          :max="1440"
          :allow-empty="false"
          :use-grouping="false"
          :disabled="!status"
          @keydown.enter="bits.commit()"
          @blur="bits.commit()"
        />
      </div>
      <div class="field">
        <label for="marathon-hours">{{ t('counter.marathon.maxHours') }}</label>
        <InputNumber
          v-model="hours.value.value"
          input-id="marathon-hours"
          :min="1"
          :max="168"
          :allow-empty="false"
          :use-grouping="false"
          :disabled="!status"
          @keydown.enter="hours.commit()"
          @blur="hours.commit()"
        />
      </div>
    </div>
    <div class="actions">
      <Button
        :label="t(status?.paused ? 'counter.marathon.resume' : 'counter.marathon.pause')"
        :icon="status?.paused ? 'pi pi-play' : 'pi pi-pause'"
        severity="secondary"
        :disabled="!status"
        @click="pause(!status?.paused)"
      />
      <form class="add" @submit.prevent="add">
        <label for="marathon-add-minutes">{{ t('counter.marathon.addMinutes') }}</label>
        <InputNumber
          v-model="addMinutes"
          input-id="marathon-add-minutes"
          :min="-1440"
          :max="1440"
          :use-grouping="false"
        />
        <Button type="submit" :label="t('counter.marathon.add')" :disabled="!status || !addMinutes" />
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { remainingMs } from '../overlay/marathon/marathon'
import { type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'
import { clockText } from './home'

const props = defineProps<{
  /** The latest `counter.marathon` event; `undefined` until the live connection has it. */
  event?: Schemas['MarathonEvent']
}>()
const emit = defineEmits<{ copy: []; error: [unknown] }>()
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
const sub = useDrawerSetting(subSaved, (v) => saveOptions({ sub_minutes: v }))
const bits = useDrawerSetting(bitsSaved, (v) => saveOptions({ bits_minutes: v }))
const hours = useDrawerSetting(hoursSaved, (v) => saveOptions({ max_hours: v }))

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
.copy {
  align-self: flex-start;
}

.remaining {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
  margin: 0;
}

.remaining output {
  font-size: 2.25rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.state {
  color: var(--p-text-muted-color);
}

.fields,
.actions,
.add {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 0.5rem 1rem;
}

.field,
.add {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.add {
  flex-direction: row;
  align-items: center;
}

/* A narrow page: the label above its field. */
@container view (max-width: 36rem) {
  .add {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
