<template>
  <!-- 設定 › 快捷鍵: global shortcuts that work in other windows too. Drawn ahead of its backend (dev mode only). -->
  <FormPanel>
    <FormRow :label="t('hotkeys.global')" label-id="hk-global" end>
      <ToggleSwitch disabled aria-labelledby="hk-global" />
      <template #hint>{{ t('hotkeys.globalHint') }}</template>
    </FormRow>
    <FormRow :label="t('hotkeys.list')" label-id="hk-list">
      <span class="muted">{{ t('hotkeys.empty') }}</span>
    </FormRow>
    <template #foot>
      <span class="note">{{ t('hotkeys.conflicts') }}</span>
      <Button :label="t('hotkeys.add')" icon="pi pi-plus" disabled />
    </template>
  </FormPanel>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import ToggleSwitch from 'primevue/toggleswitch'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'

const { t } = useI18n()
</script>

<style scoped>
.muted,
.note {
  color: var(--p-text-muted-color);
}

.note {
  margin-right: auto;
  font-size: 0.875rem;
}
</style>
