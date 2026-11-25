<template>
  <!-- A feature not built yet: the same tile, grey, as a plain block that does nothing (no drawer, not focusable);
       in dev mode it opens its page (tabs.ts showUnbuilt), whose path is the tile ID. -->
  <RouterLink v-if="unbuilt && showUnbuilt()" :to="`/${id}`" class="tile off dev" :data-page-opener="id">
    <TileFace :name="name" :icon="icon" :tint="tint" :line="{ state: 'ready', text: t('home.unbuilt') }" rows />
  </RouterLink>
  <div v-else-if="unbuilt" class="tile off" :title="t('home.unbuilt')">
    <TileFace :name="name" :icon="icon" :tint="tint" :line="{ state: 'ready', text: t('home.unbuilt') }" rows />
  </div>
  <!-- Actions a tile does in place sit beside its button (never inside it), over the line they replace. Clicks and
       attributes given to the tile go to the button. -->
  <div v-else class="tile-wrap">
    <button
      v-bind="$attrs"
      type="button"
      class="tile"
      :class="{ acts: !!$slots.actions }"
      :aria-expanded="expanded"
      :aria-controls="controls ?? drawerId(id)"
      :data-drawer-tile="controls ? undefined : id"
    >
      <TileFace :name="name" :icon="icon" :tint="tint" :line="shown" rows :masked="masked" />
      <template v-if="attention || shown.state === 'failed'">
        <span class="dot" aria-hidden="true"></span>
        <span class="sr-only">{{ t('home.attention') }}</span>
      </template>
    </button>
    <div v-if="$slots.actions" class="tile-actions" role="group" :aria-label="name"><slot name="actions" /></div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { drawerId } from './drawer'
import { type Line, type Tile, useThrottled } from './home'
import { showUnbuilt } from './tabs'
import TileFace from './TileFace.vue'

defineOptions({ inheritAttrs: false })
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
  /** Its number shows as 「––」 (下播後, masked). */
  masked?: boolean
  /** Its line shows every change at once (the mic's mute, masked numbers), not at most every few seconds. */
  immediate?: boolean
}>()
const { t } = useI18n()

const shown = useThrottled(() => props.line, () => (props.immediate ? 0 : 3000))
</script>

<style scoped>
.tile-wrap {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-block-size: 7.25rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.tile-wrap:has(> .tile[aria-expanded='true']) {
  border-color: var(--p-primary-color);
}

/* Normal flow below the number row, lined up with the text column, so wrapped buttons never cover the number. */
.tile-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.375rem;
  padding: 0 1.125rem 0.75rem calc(1.125rem + 2.75rem + 0.875rem);
}

.tile-actions :deep(.p-button) {
  padding: 0.1875rem 0.5rem;
  font-size: 0.8125rem;
}

.tile.acts {
  padding-bottom: 0.375rem;
}

.tile.acts :deep(.line) {
  display: none;
}

.tile {
  position: relative;
  flex: 1;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-content: start;
  gap: 0.25rem 0.875rem;
  width: 100%;
  min-width: 0;
  padding: 1rem 1.125rem;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

/* Grey, with the muted text color (dev.md contrast), not lowered opacity. */
.tile.off {
  height: 100%;
  min-block-size: 7.25rem;
  border: 1px dashed var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
  color: var(--p-text-muted-color);
  cursor: default;
}

.tile.off.dev {
  text-decoration: none;
  cursor: pointer;
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
