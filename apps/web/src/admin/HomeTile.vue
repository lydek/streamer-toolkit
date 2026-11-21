<template>
  <button type="button" class="tile" :aria-expanded="expanded" :aria-controls="drawerId(id)" :data-drawer-tile="id">
    <span class="badge" :class="`tint-${tint}`" aria-hidden="true"><i :class="icon"></i></span>
    <span class="name">{{ name }}</span>
    <span v-if="num" class="num" :class="{ word: !/^[\d,+]+$/.test(num) }">{{ num }}<small v-if="unit">{{ unit }}</small></span>
    <span v-if="text" class="line">{{ text }}</span>
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
import { type Line, type Tile, useThrottled } from './home'

const props = defineProps<{
  id: string
  name: string
  icon: string
  tint: Tile['tint']
  line: Line
  attention?: boolean
  expanded: boolean
}>()
const { t } = useI18n()

const shown = useThrottled(() => props.line)
const text = computed(() =>
  shown.value.state === 'ready'
    ? shown.value.text
    : t(shown.value.state === 'loading' ? 'app.loading' : 'home.loadFailed'),
)
const num = computed(() => (shown.value.state === 'ready' ? shown.value.num : undefined))
const unit = computed(() => (shown.value.state === 'ready' ? shown.value.unit : undefined))
</script>

<style scoped>
.tile {
  position: relative;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-content: start;
  gap: 0.25rem 0.875rem;
  width: 100%;
  min-width: 0;
  padding: 1rem 1.125rem;
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

.badge {
  grid-row: span 3;
  display: grid;
  place-items: center;
  width: 2.75rem;
  height: 2.75rem;
  border-radius: 6px;
  font-size: 1.25rem;
}

/* Solid tints: 14% of the hue over the tile, as in the prototype. */
.tint-primary {
  background: color-mix(in srgb, var(--p-primary-400) 14%, var(--p-surface-900));
  color: var(--p-primary-400);
}

.tint-green {
  background: color-mix(in srgb, var(--p-green-400) 14%, var(--p-surface-900));
  color: var(--p-green-400);
}

.tint-yellow {
  background: color-mix(in srgb, var(--p-yellow-400) 14%, var(--p-surface-900));
  color: var(--p-yellow-400);
}

.tint-red {
  background: color-mix(in srgb, var(--p-red-400) 14%, var(--p-surface-900));
  color: var(--p-red-400);
}

.tint-blue {
  background: color-mix(in srgb, var(--p-blue-400) 14%, var(--p-surface-900));
  color: var(--p-blue-400);
}

.name {
  grid-column: 2;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.num {
  grid-column: 2;
  overflow: hidden;
  font-size: 1.75rem;
  font-weight: 700;
  line-height: 1.15;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* A word (a song or scene name) instead of a count. */
.num.word {
  font-size: 1.25rem;
}

.num small {
  margin-left: 0.25rem;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
  font-weight: 400;
}

/* One line; long values end in an ellipsis on purpose. */
.line {
  grid-column: 2;
  overflow: hidden;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
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
