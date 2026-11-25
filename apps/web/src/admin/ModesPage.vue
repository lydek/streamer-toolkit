<template>
  <!-- 模式: one switch for the scene, sound, rewards and chat of a part of the stream. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <section class="tbl-wrap modes" aria-labelledby="modes-heading">
      <div class="tbl-top">
        <h3 id="modes-heading" class="tbl-head">{{ t('pages.modes.name') }}</h3>
        <Button :label="t('modes.fromObs')" icon="pi pi-plus" severity="secondary" disabled />
      </div>
      <p class="empty-cell">{{ t('modes.empty') }}</p>
    </section>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const { t } = useI18n()
</script>

<style scoped>
.modes {
  max-width: 72rem;
}
</style>
