<template>
  <!-- 聊天機器人 › 定時訊息. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('bot.summary')">
    <div class="tbl-wrap bot">
      <p class="empty-cell">{{ t('bot.timers.empty') }}</p>
      <div class="tbl-foot">
        <span class="note">{{ t('bot.timers.note') }}</span>
        <Button :label="t('bot.timers.add')" icon="pi pi-plus" disabled />
      </div>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const { t } = useI18n()
</script>

<style scoped>
.bot {
  max-width: 72rem;
}
</style>
