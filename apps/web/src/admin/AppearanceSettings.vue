<template>
  <div class="rows">
    <div class="field">
      <span id="overlay-theme-label">{{ t('screenTools.appearance.theme') }}</span>
      <SelectButton
        v-model="theme.value.value"
        :options="THEMES"
        :option-label="(v: Theme) => t(`screenTools.appearance.themes.${v}`)"
        :allow-empty="false"
        aria-labelledby="overlay-theme-label"
        :disabled="!appearance"
        @change="theme.commit()"
      />
    </div>
    <div class="field">
      <label for="overlay-text-size">{{ t('screenTools.appearance.textSize') }}</label>
      <InputNumber
        v-model="size.value.value"
        input-id="overlay-text-size"
        :min="50"
        :max="200"
        :step="10"
        suffix="%"
        show-buttons
        :allow-empty="false"
        :disabled="!appearance"
        @keydown.enter="size.commit()"
        @blur="size.commit()"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import SelectButton from 'primevue/selectbutton'
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Appearance } from '../overlay/appearance'
import { api, client } from './api'
import { settingQueue, useDrawerSetting } from './drawer'

type Theme = Appearance['theme']
const THEMES: Theme[] = ['dark', 'cream', 'contrast']

const props = defineProps<{
  /** The latest `screen.appearance`; `undefined` until the live connection has it. */
  appearance?: Appearance
}>()
const { t } = useI18n()

// As saved; PUT takes both, so each save sends the other as it is now.
const savedTheme = ref<Theme>('dark')
const savedSize = ref(100)
watch(
  () => props.appearance,
  (a) => {
    if (!a) return
    savedTheme.value = a.theme
    savedSize.value = a.textSize
  },
  { immediate: true },
)
const save = (patch: Partial<Appearance>) =>
  api(
    client.PUT('/api/admin/screen-tools/appearance', {
      body: { theme: savedTheme.value, textSize: savedSize.value, ...patch },
    }),
  )
const queue = settingQueue()
const theme = useDrawerSetting(savedTheme, (v) => save({ theme: v }), queue)
const size = useDrawerSetting(savedSize, (v) => save({ textSize: v }), queue)
</script>

<style scoped>
.rows {
  display: grid;
  gap: 1rem;
  max-width: 52rem;
}

.field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.25rem;
}
</style>
