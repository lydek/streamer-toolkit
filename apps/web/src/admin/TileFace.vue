<template>
  <span class="badge" :class="`tint-${tint}`" aria-hidden="true"><i :class="icon"></i></span>
  <span class="name">{{ name }}</span>
  <!-- Masked (下播後 numbers the streamer chose not to see yet): 「––」 in the number's place. -->
  <span v-if="num && masked" class="num fixed masked">––<span class="sr-only">{{ t('home.numbers.masked') }}</span></span>
  <span v-else-if="num" class="num" :class="{ word: !COUNT.test(num), fixed: rows }">{{ num }}<small v-if="unit">{{ unit }}</small></span>
  <!-- Small tiles keep all three rows, so every one is the same height: a dash for no number, an empty line kept. -->
  <span v-else-if="rows" class="num fixed none" aria-hidden="true">—</span>
  <span v-if="text || rows" class="line" :class="{ fixed: rows }">{{ text }}</span>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Line, Tile } from './home'

// What a home tile shows, for the tile button and for the same tile while home is edited. The parent is a two-column
// grid (`auto minmax(0, 1fr)`).
// `rows`: a small tile, which always shows the name, number and line rows at fixed heights.
const props = defineProps<{ name: string; icon: string; tint: Tile['tint']; line: Line; rows?: boolean; masked?: boolean }>()
// A count (12, 1,234, +3) or a time (1:23, 2.5); anything else is a word (a scene, a state) and reads smaller.
const COUNT = /^[\d,+:.]+$/
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
  font-size: 1.625rem;
  font-weight: 700;
  line-height: 1.15;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* A small tile's number row is one height whether it holds a count, a word or the dash. */
.num.fixed {
  block-size: 2rem;
  line-height: 2rem;
}

.num.masked {
  color: var(--p-text-muted-color);
}

.num.none {
  color: var(--p-text-muted-color);
}

/* An empty line keeps its height on a small tile. */
.line.fixed {
  min-block-size: 1.3125rem;
}

/* A word (a song or scene name) instead of a count. */
.num.word {
  font-size: 1.125rem;
  font-weight: 600;
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
