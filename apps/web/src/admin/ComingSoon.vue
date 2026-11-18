<template>
  <!-- A screen of the design that is not built yet (tabs.ts `soon`): its name, what it will do, and that it is not
       available. It calls no API. -->
  <FullPage :title="t(`pages.${id}.name`)">
    <p>{{ t(`pages.${id}.desc`) }}</p>
    <p class="soon"><i class="pi pi-hourglass" aria-hidden="true"></i>{{ t('pages.soon') }}</p>
  </FullPage>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import FullPage from './FullPage.vue'

defineProps<{ id: string }>()
const { t } = useI18n()
</script>

<style scoped>
.soon {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border: 1px dashed var(--p-surface-500);
  border-radius: 6px;
  color: var(--p-text-muted-color);
}
</style>
