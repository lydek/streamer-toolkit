<template>
  <p v-if="!loaded">{{ t(loadFailed ? 'home.loadFailed' : 'app.loading') }}</p>
  <div v-else class="stack">
    <label for="checkin-command">{{ t('settings.checkin.command') }}</label>
    <div class="command">
      <span aria-hidden="true">!</span>
      <InputText
        id="checkin-command"
        v-model="command.value.value"
        autocomplete="off"
        @keydown.enter="commitCommand"
        @blur="commitCommand"
      />
    </div>
    <label id="checkin-zone-label">{{ t('settings.checkin.timeZone') }}</label>
    <Select
      v-model="zone.value.value"
      input-id="checkin-zone"
      :options="zones"
      filter
      aria-labelledby="checkin-zone-label"
      @update:model-value="zone.commit()"
    />
    <label id="checkin-hour-label">{{ t('settings.checkin.resetHour') }}</label>
    <Select
      v-model="hour.value.value"
      input-id="checkin-hour"
      :options="hours"
      option-label="label"
      option-value="value"
      aria-labelledby="checkin-hour-label"
      @update:model-value="hour.commit()"
    />
    <label for="checkin-slots">{{ t('settings.checkin.cardSlots') }}</label>
    <InputText
      id="checkin-slots"
      v-model="slots.value.value"
      inputmode="numeric"
      autocomplete="off"
      @keydown.enter="slots.commit()"
      @blur="slots.commit()"
    />
  </div>
</template>

<script setup lang="ts">
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ApiError, type Schemas, api, client } from './api'
import { useDrawerSetting } from './drawer'

// Applied at once like the other settings; PUT takes all four values, so saves run one at a time from the last
// saved settings and two fields committed in a row never undo each other.
const { t } = useI18n()
const emit = defineEmits<{ error: [unknown] }>()

const loaded = ref(false)
const loadFailed = ref(false)
let saved: Schemas['CheckinSettingsBody'] = { command: '', time_zone: '', reset_hour: 0, card_slots: 0 }
let queue: Promise<unknown> = Promise.resolve()
function put(patch: Partial<Schemas['CheckinSettingsBody']>) {
  const run = queue.then(async () => {
    saved = await api(client.PUT('/api/admin/checkin/settings', { body: { ...saved, ...patch } }))
  })
  queue = run.catch(() => {})
  return run
}

const commandSaved = ref('')
const command = useDrawerSetting(commandSaved, (v) => put({ command: v }))
// The "!" is shown in front of the field, so a typed one is dropped (the Host drops it too).
function commitCommand() {
  command.value.value = command.value.value.trim().replace(/^!/, '')
  void command.commit()
}
const zoneSaved = ref('')
const zone = useDrawerSetting(zoneSaved, (v) => put({ time_zone: v }))
const hourSaved = ref(0)
const hour = useDrawerSetting(hourSaved, (v) => put({ reset_hour: v }))
// Kept as text, the way the field holds it.
const slotsSaved = ref('')
const slots = useDrawerSetting(slotsSaved, (text) => {
  const n = Number(text)
  return Number.isInteger(n) && n >= 1 && n <= 100
    ? put({ card_slots: n })
    : Promise.reject(new ApiError('checkin.invalid_settings'))
})

const hours = Array.from({ length: 24 }, (_, h) => ({ value: h, label: `${String(h).padStart(2, '0')}:00` }))
// The browser's IANA list; the saved zone stays selectable even if this engine does not list it.
const zones = computed(() => {
  const list = Intl.supportedValuesOf('timeZone')
  return zoneSaved.value && !list.includes(zoneSaved.value) ? [zoneSaved.value, ...list] : list
})

onMounted(async () => {
  try {
    saved = await api(client.GET('/api/admin/checkin/settings'))
    commandSaved.value = saved.command
    zoneSaved.value = saved.time_zone
    hourSaved.value = saved.reset_hour
    slotsSaved.value = String(saved.card_slots)
    loaded.value = true
  } catch (e) {
    loadFailed.value = true
    emit('error', e)
  }
})
</script>

<style scoped>
.command {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}
</style>
