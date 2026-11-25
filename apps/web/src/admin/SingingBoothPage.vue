<template>
  <!-- 練唱台: the singing booth program's screen, embedded. Drawn ahead of its backend (dev mode only): no embed yet. -->
  <FullPage>
    <section class="embed" :aria-label="t('pages.singingBooth.name')">
      <div class="embed-bar">
        <span>{{ t('singingBooth.off') }}</span>
        <Button :label="t('singingBooth.openOwn')" icon="pi pi-external-link" text disabled />
      </div>
      <div class="embed-body">
        <i class="pi pi-microphone" aria-hidden="true"></i>
        <span>{{ t('singingBooth.body') }}</span>
      </div>
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
.embed {
  border-radius: 6px;
  background: var(--p-surface-900);
  overflow: hidden;
}

.embed-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.25rem 0.5rem 0.25rem 1rem;
  border-bottom: 1px solid var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

/* The dark checkerboard the template previews use, where the booth's screen will show. */
.embed-body {
  display: grid;
  place-items: center;
  align-content: center;
  gap: 0.5rem;
  min-height: max(18rem, calc(100vh - 20rem));
  background: repeating-conic-gradient(#1b1d2a 0 25%, #222433 0 50%) 0 0 / 1rem 1rem;
  color: var(--p-text-muted-color);
}

.embed-body .pi {
  font-size: 2.5rem;
  color: var(--p-primary-color);
}
</style>
