<template>
  <!-- A feature not built yet: the same tile, grey, as a plain block that does nothing (no drawer, not focusable). -->
  <div v-if="unbuilt" class="tile off" :title="t('home.unbuilt')">
    <TileFace :name="name" :icon="icon" :tint="tint" :line="{ state: 'ready', text: t('home.unbuilt') }" />
  </div>
  <button v-else type="button" class="tile" :aria-expanded="expanded" :aria-controls="controls ?? drawerId(id)" :data-drawer-tile="controls ? undefined : id">
    <TileFace :name="name" :icon="icon" :tint="tint" :line="shown" />
    <template v-if="attention || shown.state === 'failed'">
      <span class="dot" aria-hidden="true"></span>
      <span class="sr-only">{{ t('home.attention') }}</span>
    </template>
  </button>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { drawerId } from './drawer'
import { type Line, type Tile, useThrottled } from './home'
import TileFace from './TileFace.vue'

const props = defineProps<{
  id: string
  name: string
  icon: string
  tint: Tile['tint']
  line: Line
  attention?: boolean
  expanded: boolean
  /** The element it opens when not a drawer (a home block); such a tile is no drawer tile for focus. */
  controls?: string
  /** A feature not built yet: grey, and it does nothing. */
  unbuilt?: boolean
}>()
const { t } = useI18n()

const shown = useThrottled(() => props.line)
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

/* Grey, with the muted text color (dev.md contrast), not lowered opacity. */
.tile.off {
  border-style: dashed;
  color: var(--p-text-muted-color);
  cursor: default;
}

.tile[aria-expanded='true'] {
  border-color: var(--p-primary-color);
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
