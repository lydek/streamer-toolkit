<template>
  <!-- 點歌規則: who can request, how often and how titles are matched. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('songRules.summary')">
    <FormPanel class="song-rules">
      <FormRow :label="t('songRules.randomCooldown')" input-id="sr-random">
        <span class="unit-field">
          <InputNumber v-model="randomDays" input-id="sr-random" :min="0" :max="365" :use-grouping="false" />
          <span>{{ t('songRules.days') }}</span>
        </span>
      </FormRow>
      <FormRow :label="t('songRules.favorOld')" label-id="sr-old" end>
        <ToggleSwitch disabled aria-labelledby="sr-old" />
      </FormRow>
      <FormRow :label="t('songRules.fuzzy')" label-id="sr-fuzzy" end>
        <ToggleSwitch disabled aria-labelledby="sr-fuzzy" />
      </FormRow>
      <FormRow :label="t('songRules.perViewer')" input-id="sr-per-viewer">
        <span class="unit-field">
          <InputNumber v-model="perViewer" input-id="sr-per-viewer" :min="1" :max="20" :use-grouping="false" />
          <span>{{ t('songRules.songs') }}</span>
        </span>
      </FormRow>
      <FormRow :label="t('songRules.sameSong')" input-id="sr-same">
        <span class="unit-field">
          <InputNumber v-model="sameDays" input-id="sr-same" :min="0" :max="365" :use-grouping="false" />
          <span>{{ t('songRules.sameSongUnit') }}</span>
        </span>
      </FormRow>
      <FormRow :label="t('songRules.blocked')" label-id="sr-blocked">
        <span class="unit-field">
          <span class="muted">{{ t('songRules.noneBlocked') }}</span>
          <Button :label="t('songRules.change')" text size="small" disabled />
        </span>
      </FormRow>
      <FormRow :label="t('songRules.hint')" label-id="sr-hint" end>
        <ToggleSwitch disabled aria-labelledby="sr-hint" />
        <template #hint>{{ t('songRules.hintPreview') }}</template>
      </FormRow>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const { t } = useI18n()
const randomDays = ref(7)
const perViewer = ref(2)
const sameDays = ref(3)
</script>

<style scoped>
.song-rules {
  max-width: 60rem;
}

.unit-field {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.unit-field :deep(.p-inputnumber-input) {
  width: 5rem;
}

.muted {
  color: var(--p-text-muted-color);
}
</style>
