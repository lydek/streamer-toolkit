<template>
  <span class="badge" :class="`tint-${tint}`" aria-hidden="true"><i :class="icon"></i></span>
  <span class="name">{{ name }}</span>
  <span v-if="num" class="num" :class="{ word: !/^[\d,+]+$/.test(num) }">{{ num }}<small v-if="unit">{{ unit }}</small></span>
  <span v-if="text" class="line">{{ text }}</span>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Line, Tile } from './home'

// What a home tile shows, for the tile button and for the same tile while home is edited. The parent is a two-column
// grid (`auto minmax(0, 1fr)`).
const props = defineProps<{ name: string; icon: string; tint: Tile['tint']; line: Line }>()
const { t } = useI18n()

const text = computed(() =>
  props.line.state === 'ready' ? props.line.text : t(props.line.state === 'loading' ? 'app.loading' : 'home.loadFailed'),
)
const num = computed(() => (props.line.state === 'ready' ? props.line.num : undefined))
const unit = computed(() => (props.line.state === 'ready' ? props.line.unit : undefined))
</script>

<style scoped>
.badge {
  grid-row: span 3;
  display: grid;
  place-items: center;
  width: 2.75rem;
  height: 2.75rem;
  border-radius: 6px;
  font-size: 1.25rem;
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
</style>
