<template>
  <!-- 自訂身分組: the streamer's own viewer groups. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="tbl-wrap roles">
      <p class="empty-cell">{{ t('viewers.groups.empty') }}</p>
      <div class="tbl-foot">
        <span class="note">{{ t('viewers.groups.note') }}</span>
        <Button :label="t('viewers.groups.add')" icon="pi pi-plus" disabled />
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
.roles {
  max-width: 72rem;
}
</style>
