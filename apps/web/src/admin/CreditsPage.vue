<template>
  <!-- 感謝名單: who to thank this stream, by kind. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <template #actions>
      <Button :label="t('credits.playThanks')" icon="pi pi-play" severity="secondary" disabled />
      <Button :label="t('credits.playEnd')" icon="pi pi-play" disabled />
    </template>
    <div class="credits">
      <div class="kinds">
        <section v-for="k in KINDS" :key="k" class="tbl-wrap" :aria-labelledby="`credits-${k}`">
          <h3 :id="`credits-${k}`" class="tbl-head">{{ t(`credits.kinds.${k}`) }}</h3>
          <p class="empty-cell">{{ t('credits.none') }}</p>
        </section>
      </div>
      <p class="note">
        {{ t('credits.backgrounds') }}
        <Button :label="t('credits.manageBackgrounds')" text size="small" disabled />
      </p>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

const KINDS = ['raids', 'subs', 'bits', 'follows', 'redemptions', 'songs', 'chatters']

const { t } = useI18n()
</script>

<style scoped>
.credits {
  max-width: 72rem;
}

.kinds {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(16rem, 100%), 1fr));
  gap: 1rem;
}

.kinds .tbl-wrap {
  margin-top: 0;
}

.note {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.5rem;
  margin: 0.75rem 0 0;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
