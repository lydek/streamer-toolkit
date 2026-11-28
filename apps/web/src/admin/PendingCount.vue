<template>
  <!-- A tab's waiting items: the number for the eye, a phrase for screen readers; nothing when there are none. -->
  <template v-if="n > 0">
    <span class="pending-count" aria-hidden="true">{{ n > 99 ? '99+' : n }}</span>
    <span class="sr-only">{{ t('nav.pending', { n }) }}</span>
  </template>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

defineProps<{ n: number }>()
const { t } = useI18n()
</script>

<style scoped>
.pending-count {
  min-width: 1.25rem;
  margin-left: 0.25rem;
  padding: 0 0.375rem;
  border-radius: 999px;
  background: var(--p-primary-color);
  color: var(--p-primary-contrast-color);
  font-size: 0.6875rem;
  font-weight: 700;
  line-height: 1.25rem;
  text-align: center;
}
</style>
