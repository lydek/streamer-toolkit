<template>
  <button type="button" class="tile" :aria-expanded="expanded" :aria-controls="drawerId(id)" :data-drawer-tile="id">
    <i :class="icon" aria-hidden="true"></i>
    <span class="name">{{ name }}</span>
    <span class="line">{{ text }}</span>
    <template v-if="attention || shown.state === 'failed'">
      <span class="dot" aria-hidden="true"></span>
      <span class="sr-only">{{ t('home.attention') }}</span>
    </template>
  </button>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { drawerId } from './drawer'
import { type Line, useThrottled } from './home'

const props = defineProps<{ id: string; name: string; icon: string; line: Line; attention?: boolean; expanded: boolean }>()
const { t } = useI18n()

const shown = useThrottled(() => props.line)
const text = computed(() =>
  shown.value.state === 'ready'
    ? shown.value.text
    : t(shown.value.state === 'loading' ? 'app.loading' : 'home.loadFailed'),
)
</script>

<style scoped>
.tile {
  position: relative;
  display: grid;
  gap: 0.25rem;
  width: 100%;
  min-width: 0;
  padding: 0.75rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

.tile[aria-expanded='true'] {
  border-color: var(--p-primary-color);
}

.tile i {
  font-size: 1.25rem;
}

.name {
  font-weight: 600;
  overflow-wrap: anywhere;
}

/* One line; long values end in an ellipsis on purpose. */
.line {
  overflow: hidden;
  color: var(--p-text-muted-color);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dot {
  position: absolute;
  top: 0.75rem;
  right: 0.75rem;
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: var(--p-primary-color);
}
</style>
