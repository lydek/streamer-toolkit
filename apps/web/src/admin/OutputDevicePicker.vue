<template>
  <div class="field">
    <label for="preview-output">{{ t('outputDevice.label') }}</label>
    <div class="row">
      <Select
        :model-value="outputDevice?.id ?? DEFAULT"
        input-id="preview-output"
        :options="options"
        option-label="label"
        option-value="id"
        @update:model-value="pick"
      />
      <Button v-if="!named" :label="t('outputDevice.names')" text size="small" @click="load(true)" />
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Select from 'primevue/select'
import { computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { type OutputDevice, chooseOutput, listOutputs, outputDevice } from './outputDevice'

const { t } = useI18n()
const devices = shallowRef<OutputDevice[]>([])
// Until the one-time microphone question, devices show as #1, #2… or not at all.
const named = ref(true)
// Not '': the Select takes an empty string as no choice and shows nothing. Device lists never hold this ID.
const DEFAULT = 'default'
const options = computed(() => {
  const list = [{ id: DEFAULT, label: t('outputDevice.default') }, ...devices.value]
  // The saved device stays listed while it is unplugged, so the choice still shows (the default plays meanwhile).
  const saved = outputDevice.value
  if (saved && !devices.value.some((d) => d.id === saved.id)) list.push({ id: saved.id, label: saved.label })
  return list
})

async function load(withNames = false) {
  ;({ devices: devices.value, named: named.value } = await listOutputs(withNames))
}

function pick(id: string) {
  chooseOutput(id !== DEFAULT ? options.value.find((d) => d.id === id) : undefined)
}

void load()
</script>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  margin-top: 1rem;
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}
</style>
