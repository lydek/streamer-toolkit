<template>
  <!-- A page not built yet and not drawn yet, open in dev mode only (tabs.ts showUnbuilt). -->
  <FullPage :status="page?.soon && t(`pages.${page.soon.id}.desc`)">
    <p class="soon"><i class="pi pi-hourglass" aria-hidden="true"></i>{{ t('pages.soon') }}</p>
  </FullPage>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import FullPage from './FullPage.vue'
import { tabPage } from './tabs'

const { t } = useI18n()
const route = useRoute()
const page = computed(() => tabPage(route.path))
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
