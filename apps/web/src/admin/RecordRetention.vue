<template>
  <!-- Settings › 資料與備份: how long activity records are kept. Shortening it deletes older ones, so it asks first. -->
  <FormPanel :heading="t('retention.title')">
    <p v-if="!saved" class="loading">{{ loadFailed ? t('home.loadFailed') : t('app.loading') }}</p>
    <FormRow v-else :label="t('retention.records')" label-id="retention-label" top>
      <div class="options" role="radiogroup" aria-labelledby="retention-label" aria-describedby="retention-hint">
        <label class="option">
          <input type="radio" name="retention" value="days" :checked="mode === 'days'" :disabled="busy" @change="pick('days')" />
          {{ t('retention.keep') }}
          <input
            v-model.number="days"
            class="days"
            type="number"
            min="1"
            :max="MAX_DAYS"
            :aria-label="t('retention.days')"
            :disabled="busy || mode !== 'days'"
            @change="pick('days')"
          />
          {{ t('retention.daysUnit') }}
        </label>
        <label class="option">
          <input
            type="radio"
            name="retention"
            value="forever"
            :checked="mode === 'forever'"
            :disabled="busy"
            @change="pick('forever')"
          />
          {{ t('retention.forever') }}
        </label>
      </div>
      <template #hint>
        <span id="retention-hint">
          {{ t('retention.hint') }}
          <template v-if="mode === 'forever'">{{ t('retention.foreverNote') }}</template>
        </span>
      </template>
    </FormRow>

    <dialog ref="confirmEl" class="sheet" aria-labelledby="retention-confirm-title" @close="onConfirmClose">
      <h2 id="retention-confirm-title">{{ t('retention.confirmTitle', { days: pending }) }}</h2>
      <p>{{ t('retention.confirmText', { days: pending, count: expired }) }}</p>
      <div class="buttons sheet-actions">
        <Button :label="t('twitch.cancel')" text autofocus @click="confirmEl?.close()" />
        <Button :label="t('retention.confirmOk', { days: pending })" severity="danger" @click="confirmEl?.close('ok')" />
      </div>
    </dialog>
  </FormPanel>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import { errorToast, successToast } from './home'

type Retention = Schemas['RecordRetention']
const MAX_DAYS = 3650

const { t } = useI18n()
const toast = useToast()
const saved = ref<Retention>()
const loadFailed = ref(false)
const busy = ref(false)
const mode = ref<'days' | 'forever'>('days')
const days = ref(30)
const pending = ref(0)
const expired = ref(0)
const confirmEl = ref<HTMLDialogElement>()

const savedDays = (r: Retention | undefined) => (r && r !== 'forever' ? r.days : undefined)

/** Shows `r` in the controls. */
function show(r: Retention) {
  mode.value = r === 'forever' ? 'forever' : 'days'
  if (r !== 'forever') days.value = r.days
}

const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))

async function load() {
  try {
    saved.value = await api(client.GET('/api/admin/records/retention'))
    show(saved.value)
  } catch {
    loadFailed.value = true
  }
}

async function save(next: Retention) {
  busy.value = true
  try {
    const { deleted } = await api(client.PUT('/api/admin/records/retention', { body: next }))
    saved.value = next
    toast.add(
      successToast(
        next === 'forever'
          ? t('retention.savedForever')
          : deleted > 0
            ? t('retention.deleted', { days: next.days })
            : t('retention.saved', { days: next.days }),
      ),
    )
  } catch (e) {
    // A save can fail after deleting (the settings write), so show what the Host now has, not what was here.
    report(e)
    await load()
  } finally {
    busy.value = false
  }
}

async function pick(next: 'days' | 'forever') {
  mode.value = next
  if (next === 'forever') return save('forever')
  const n = Math.round(days.value)
  if (!(n >= 1 && n <= MAX_DAYS)) {
    toast.add(errorToast(t('retention.outOfRange', { max: MAX_DAYS })))
    return show(saved.value!)
  }
  const before = savedDays(saved.value)
  if (before !== undefined && n >= before) return n === before ? undefined : save({ days: n })
  // Shorter (or forever before): say how many records go, and ask.
  try {
    expired.value = (await api(client.GET('/api/admin/records/expired', { params: { query: { days: n } } }))).count
  } catch (e) {
    show(saved.value!)
    return report(e)
  }
  pending.value = n
  confirmEl.value!.returnValue = ''
  confirmEl.value!.showModal()
}

// Esc and Cancel close with an empty return value: nothing changes.
function onConfirmClose() {
  if (confirmEl.value!.returnValue === 'ok') void save({ days: pending.value })
  else show(saved.value!)
}

void load()
</script>

<style scoped>
.loading {
  margin: 0;
  padding: 0.875rem 0;
}

.options {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.5rem;
}

.option {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.days {
  width: 5.5rem;
  padding: 0.25rem 0.5rem;
  border: 1px solid var(--p-content-border-color);
  border-radius: var(--p-content-border-radius);
  background: var(--p-content-background);
  color: var(--p-text-color);
  font: inherit;
}

dialog {
  max-width: min(30rem, calc(100vw - 2rem));
  border: 1px solid var(--p-content-border-color);
  border-radius: var(--p-content-border-radius);
  background: var(--p-content-background);
  color: var(--p-text-color);
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 0.5rem;
}
</style>
