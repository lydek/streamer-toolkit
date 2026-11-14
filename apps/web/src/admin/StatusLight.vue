<template>
  <button type="button" :class="['light', light.state]" :aria-label="`${name}：${t(light.key)}`">
    <i :class="ICONS[light.state]" aria-hidden="true"></i>
    <span>{{ name }}</span>
    <span class="light-text">{{ t(light.key) }}</span>
  </button>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { Light } from './home'

defineProps<{ name: string; light: Light }>()
const { t } = useI18n()

// Distinct shapes per state, so the light does not rely on color (WCAG 1.4.1).
const ICONS: Record<Light['state'], string> = {
  connected: 'pi pi-check-circle',
  reconnecting: 'pi pi-sync',
  error: 'pi pi-times-circle',
  loading: 'pi pi-ellipsis-h',
}
</script>

<style scoped>
.light {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.5rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

/* Icon colors against surface-950 (#070815): green-400 11.4:1, yellow-400 13:1, red-400 7.2:1, surface-400 7.6:1. */
.connected i {
  color: var(--p-green-400);
}

.reconnecting i {
  color: var(--p-yellow-400);
}

.error i {
  color: var(--p-red-400);
}

.loading i {
  color: var(--p-surface-400);
}

/* Narrow widths keep the name and icon; the state stays in aria-label. */
@media (max-width: 599.98px) {
  .light-text {
    display: none;
  }
}
</style>
