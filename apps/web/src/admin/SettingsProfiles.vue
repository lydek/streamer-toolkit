<template>
  <!-- 設定 › 設定檔: one set of settings per kind of stream. Drawn ahead of its backend (dev mode only). -->
  <div class="tbl-wrap">
    <p class="empty-cell">{{ t('profiles.empty') }}</p>
    <div class="tbl-foot">
      <span class="note">{{ t('profiles.note') }}</span>
      <Button :label="t('profiles.add')" icon="pi pi-plus" disabled />
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
</script>
