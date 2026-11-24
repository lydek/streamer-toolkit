<template>
  <FormPanel>
    <div class="warn-row">
      <span id="warn-chat">{{ t('screenTools.warnings.chatOverlay') }}</span>
      <ToggleSwitch v-model="chat.value.value" aria-labelledby="warn-chat" :disabled="!warnings" @update:model-value="chat.commit()" />
    </div>

    <div class="warn-row">
      <span class="mic">
        <span id="warn-mic">{{ t('screenTools.warnings.mic') }}</span>
        <InputNumber
          v-model="seconds.value.value"
          :min="1"
          :max="3600"
          :allow-empty="false"
          :use-grouping="false"
          :aria-label="t('screenTools.warnings.seconds')"
          :disabled="!warnings"
          @keydown.enter="seconds.commit()"
          @blur="seconds.commit()"
        />
        <span>{{ t('screenTools.warnings.secondsUnit') }}</span>
      </span>
      <ToggleSwitch v-model="mic.value.value" aria-labelledby="warn-mic" :disabled="!warnings" @update:model-value="mic.commit()" />
    </div>

    <FormRow :label="t('screenTools.warnings.input')" input-id="warn-mic-input">
      <Select
        v-model="input.value.value"
        input-id="warn-mic-input"
        :options="inputChoices"
        :placeholder="t('screenTools.warnings.pickInput')"
        :aria-label="t('screenTools.warnings.input')"
        :disabled="!warnings"
        show-clear
        append-to="self"
        @update:model-value="input.commit()"
      />
      <p v-if="warnings?.micMissing" class="hint alert" role="alert">{{ t('screenTools.warnings.inputMissing', { name: warnings.micInput }) }}</p>
      <p v-else-if="warnings && !warnings.micInput" class="hint">{{ t('screenTools.warnings.noInput') }}</p>
    </FormRow>

    <div class="warn-row">
      <span id="warn-twitch">{{ t('screenTools.warnings.twitch') }}</span>
      <ToggleSwitch v-model="twitch.value.value" aria-labelledby="warn-twitch" :disabled="!warnings" @update:model-value="twitch.commit()" />
    </div>
  </FormPanel>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { type Schemas, api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'

const props = defineProps<{
  /** The latest `screen.warnings`; `undefined` until the live connection has it. */
  warnings?: Schemas['WarningsPayload']
  /** OBS's audio inputs; empty while OBS is not connected. */
  inputs: Schemas['Input'][]
}>()
const { t } = useI18n()

// The settings as saved; PUT takes them all, so each save sends the others as they are now.
const saved = {
  chat_overlay: ref(true),
  twitch: ref(false),
  mic: ref(true),
  mic_seconds: ref(30),
  mic_input: ref<string | null>(null),
}
watch(
  () => props.warnings,
  (w) => {
    if (!w) return
    saved.chat_overlay.value = w.chatOverlay
    saved.twitch.value = w.twitch
    saved.mic.value = w.mic
    saved.mic_seconds.value = w.micSeconds
    saved.mic_input.value = w.micInput ?? null
  },
  { immediate: true },
)
const save = (patch: Partial<Schemas['WarningSettingsBody']>) =>
  api(
    client.PUT('/api/admin/screen-tools/warnings', {
      body: {
        chat_overlay: saved.chat_overlay.value,
        twitch: saved.twitch.value,
        mic: saved.mic.value,
        mic_seconds: saved.mic_seconds.value,
        mic_input: saved.mic_input.value,
        ...patch,
      },
    }),
  )
const queue = settingQueue()
const chat = useDrawerSetting(saved.chat_overlay, (v) => save({ chat_overlay: v }), queue)
const twitch = useDrawerSetting(saved.twitch, (v) => save({ twitch: v }), queue)
const mic = useDrawerSetting(saved.mic, (v) => save({ mic: v }), queue)
const seconds = useDrawerSetting(saved.mic_seconds, (v) => save({ mic_seconds: v }), queue)
const input = useDrawerSetting(saved.mic_input, (v) => save({ mic_input: v }), queue)

// The picked input stays listed while OBS is away, so the choice is still visible.
const inputChoices = computed(() => {
  const names = props.inputs.map((i) => i.name)
  const picked = saved.mic_input.value
  return picked && !names.includes(picked) ? [picked, ...names] : names
})
</script>

<style scoped>
.warn-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 0.75rem 1rem;
  padding: 0.75rem 0;
}

.warn-row + .warn-row,
.warn-row + :deep(.form-row),
:deep(.form-row) + .warn-row {
  border-top: 1px solid var(--p-surface-800);
}

.mic {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.mic :deep(.p-inputnumber-input) {
  width: 5rem;
}

.hint {
  margin: 0.375rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.alert {
  color: var(--p-red-400);
}
</style>
