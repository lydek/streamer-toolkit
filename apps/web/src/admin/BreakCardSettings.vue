<template>
  <div class="break-layout">
    <FormPanel class="break-panel">
      <FormRow :label="t('screenTools.break.text')" input-id="break-text">
        <InputText
          id="break-text"
          v-model="text.value.value"
          maxlength="100"
          :disabled="!card"
          @keydown.enter="text.commit()"
          @blur="text.commit()"
        />
      </FormRow>
      <FormRow :label="t('screenTools.break.countdown')" label-id="break-mode-label">
        <SelectButton
          v-model="mode.value.value"
          :options="MODES"
          :option-label="(m: Mode) => t(`screenTools.break.modes.${m}`)"
          :allow-empty="false"
          aria-labelledby="break-mode-label"
          :disabled="!card"
          @change="mode.commit()"
        />
      </FormRow>
      <FormRow v-if="mode.value.value === 'minutes'" :label="t('screenTools.break.minutes')" input-id="break-minutes">
        <InputNumber
          v-model="minutes.value.value"
          input-id="break-minutes"
          :min="1"
          :max="1440"
          :allow-empty="false"
          :use-grouping="false"
          :disabled="!card"
          @keydown.enter="minutes.commit()"
          @blur="minutes.commit()"
        />
      </FormRow>
      <FormRow v-else :label="t('screenTools.break.until')" input-id="break-until">
        <!-- 24-hour text: a native time input shows 上午/下午 on a Chinese Windows. -->
        <InputMask
          id="break-until"
          v-model="until.value.value"
          mask="99:99"
          placeholder="20:00"
          inputmode="numeric"
          :disabled="!card"
          @keydown.enter="until.commit()"
          @blur="until.commit()"
        />
      </FormRow>
      <FormRow :label="t('screenTools.break.endText')" input-id="break-end-text">
        <InputText
          id="break-end-text"
          v-model="endText.value.value"
          maxlength="100"
          :disabled="!card"
          @keydown.enter="endText.commit()"
          @blur="endText.commit()"
        />
      </FormRow>
      <template #foot>
        <div class="actions">
          <Button :label="t('screenTools.break.hide')" text :disabled="!card?.shown" @click="hide" />
          <Button :label="t('screenTools.break.show')" :disabled="!card" @click="show" />
        </div>
      </template>
    </FormPanel>

    <section :aria-label="t('screenTools.break.preview')" class="preview">
      <div class="card">
        <b v-if="(card?.shown ? card.text : text.value.value)">{{ card?.shown ? card.text : text.value.value }}</b>
        <output>{{ previewNumber }}</output>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputMask from 'primevue/inputmask'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { type BreakCard, breakLeftMs, countdownText } from '../overlay/break/breakCard'
import { type Schemas, api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import { countdownOf, hideBreak, showBreak } from './screenTools'

type Mode = BreakCard['mode']
const MODES: Mode[] = ['minutes', 'until']

const props = defineProps<{
  /** The latest `screen.break`; `undefined` until the live connection has it. */
  card?: BreakCard
}>()
const emit = defineEmits<{ error: [unknown] }>()
const { t } = useI18n()

const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 500)
onBeforeUnmount(() => clearInterval(ticker))
// A tick up to 500 ms old would count a card that just arrived past its length (10:01 before 10:00).
watch(
  () => props.card,
  () => (now.value = Date.now()),
)

// The settings as saved; PUT takes them all, so each save sends the others as they are now.
const saved = {
  text: ref(''),
  end_text: ref(''),
  mode: ref<Mode>('minutes'),
  minutes: ref(10),
  until_time: ref('20:00'),
}
watch(
  () => props.card,
  (c) => {
    if (!c) return
    saved.text.value = c.text
    saved.end_text.value = c.endText
    saved.mode.value = c.mode
    saved.minutes.value = c.minutes
    saved.until_time.value = c.untilTime
  },
  { immediate: true },
)
const body = (): Schemas['BreakSettingsBody'] => ({
  text: saved.text.value,
  end_text: saved.end_text.value,
  mode: saved.mode.value,
  minutes: saved.minutes.value,
  until_time: saved.until_time.value,
})
const save = (patch: Partial<Schemas['BreakSettingsBody']>) =>
  api(client.PUT('/api/admin/screen-tools/break', { body: { ...body(), ...patch } }))
const queue = settingQueue()
const text = useDrawerSetting(saved.text, (v) => save({ text: v }), queue)
const endText = useDrawerSetting(saved.end_text, (v) => save({ end_text: v }), queue)
const mode = useDrawerSetting(saved.mode, (v) => save({ mode: v }), queue)
const minutes = useDrawerSetting(saved.minutes, (v) => save({ minutes: v }), queue)
const until = useDrawerSetting(saved.until_time, (v) => save({ until_time: v }), queue)

const previewNumber = computed(() => {
  const c = props.card
  if (c?.shown) return breakLeftMs(c, now.value) > 0 ? countdownText(breakLeftMs(c, now.value)) : c.endText
  return mode.value.value === 'minutes' ? countdownText(minutes.value.value * 60_000) : until.value.value
})

async function show() {
  // Fields not yet committed (still focused) go first, so the card shows what is on screen.
  const ok = (await Promise.all([text, endText, mode, minutes, until].map((s) => s.commit()))).every(Boolean)
  if (!ok) return
  try {
    await showBreak(countdownOf({ mode: saved.mode.value, minutes: saved.minutes.value, untilTime: saved.until_time.value }))
  } catch (e) {
    emit('error', e)
  }
}

async function hide() {
  try {
    await hideBreak()
  } catch (e) {
    emit('error', e)
  }
}
</script>

<style scoped>
.break-layout {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.break-panel {
  width: 100%;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 0.5rem;
  width: 100%;
}

/* The OBS canvas scaled to the frame: the overlay's sizes at 1920px wide, in container units (1px = 100cqw / 1920),
   so the card keeps its proportions at any width and its text never wraps. */
.preview {
  container-type: inline-size;
  display: grid;
  place-items: center;
  width: 100%;
  aspect-ratio: 16 / 9;
  border-radius: 6px;
  background: var(--p-surface-800);
}

.card {
  display: grid;
  justify-items: center;
  gap: calc(100cqw * 4 / 1920);
  padding: calc(100cqw * 16 / 1920) calc(100cqw * 28 / 1920);
  border-radius: calc(100cqw * 12 / 1920);
  background: rgb(0 0 0 / 70%);
  color: #fff;
  white-space: nowrap;
}

.card b {
  font-size: calc(100cqw * 24 / 1920);
}

.card output {
  font-size: calc(100cqw * 32 / 1920);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
