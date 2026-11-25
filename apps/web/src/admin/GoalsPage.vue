<template>
  <!-- 目標: Twitch goals or the streamer's own, on screen. Drawn ahead of its backend (dev mode only). -->
  <FullPage :status="t('goals.summary')">
    <div class="tbl-wrap goals">
      <p class="empty-cell">{{ t('goals.empty') }}</p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const { t } = useI18n()
</script>

<style scoped>
.goals {
  max-width: 72rem;
}
</style>
