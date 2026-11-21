<template>
  <div ref="root" class="arrange">
    <p class="sr-only" aria-live="polite">{{ announcement }}</p>

    <!-- The home grid itself: drag a tile onto another, or use its buttons. -->
    <ul class="plain grid" :aria-label="t('home.tiles')">
      <li
        v-for="(tile, i) in arranged"
        :key="tile.id"
        class="item"
        :class="{ over: over === tile.id }"
        :data-tile="tile.id"
        draggable="true"
        @dragstart="onDragstart($event, tile.id)"
        @dragover="onDragover($event, tile.id)"
        @dragleave="over = null"
        @drop.prevent="onDrop(tile.id)"
        @dragend="dragged = over = null"
      >
        <TileFace :name="name(tile.id)" :icon="tile.icon" :tint="tile.tint" :line="lineOf(tile.id)" />
        <Button
          data-action="remove"
          class="hide"
          icon="pi pi-times"
          severity="secondary"
          text
          rounded
          size="small"
          :aria-label="t('arrange.for', { action: t('arrange.remove'), name: name(tile.id) })"
          @click="remove(tile.id)"
        />
        <div class="buttons">
          <Button
            data-action="earlier"
            icon="pi pi-arrow-left"
            severity="secondary"
            outlined
            size="small"
            :aria-label="t('arrange.for', { action: t('arrange.earlier'), name: name(tile.id) })"
            :disabled="i === 0"
            @click="move(tile.id, -1)"
          />
          <Button
            data-action="later"
            icon="pi pi-arrow-right"
            severity="secondary"
            outlined
            size="small"
            :aria-label="t('arrange.for', { action: t('arrange.later'), name: name(tile.id) })"
            :disabled="i === arranged.length - 1"
            @click="move(tile.id, 1)"
          />
        </div>
      </li>
    </ul>

    <section v-if="available.length" aria-labelledby="available-heading">
      <h2 id="available-heading" tabindex="-1">{{ t('arrange.available') }}</h2>
      <ul class="plain chips" aria-labelledby="available-heading">
        <li v-for="tile in available" :key="tile.id" :data-available="tile.id">
          <Button
            data-action="add"
            :label="name(tile.id)"
            :aria-label="t('arrange.for', { action: t('arrange.add'), name: name(tile.id) })"
            icon="pi pi-plus"
            severity="secondary"
            outlined
            size="small"
            @click="add(tile.id)"
          />
        </li>
      </ul>
    </section>

    <div>
      <Button :label="t('arrange.restore')" icon="pi pi-replay" severity="secondary" outlined @click="restore" />
    </div>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import { type Line, arrangedTiles, availableTiles, dropTile, moveTile, tilePosition } from './home'
import TileFace from './TileFace.vue'

/*
 * Edit mode of the admin home: the tiles stay in the home grid with buttons to move and hide them, and hidden ones
 * are listed below to add back. Every action saves at once through `save`, which the page owns, so a save still
 * restores and reports its failure after this component is gone (Done during a save). Results are announced in a
 * polite live region.
 */

const props = defineProps<{
  saved: string[] | null
  /** Shows `next` at once and saves it; resolves `false` after a failure was restored and reported, and returns
   * nothing (changing nothing) while another save is running. */
  save: (next: string[] | null) => Promise<boolean> | undefined
  modules: Schemas['ModuleState'][]
  name: (id: string) => string
  /** What each tile shows on home. */
  lines: Record<string, Line>
}>()

const { t } = useI18n()
const root = ref<HTMLElement>()
const announcement = ref('')
const dragged = ref<string | null>(null)
const over = ref<string | null>(null)

const arranged = computed(() => arrangedTiles(props.saved))
const available = computed(() => availableTiles(arranged.value))
const disabled = (id: string) => props.modules.some((m) => m.id === id && !m.enabled)
const lineOf = (id: string): Line =>
  disabled(id) ? { state: 'ready', text: t('arrange.notEnabled') } : (props.lines[id] ?? { state: 'loading' })

async function announce(text: string) {
  // Cleared first, so the same text twice in a row is still announced.
  announcement.value = ''
  await nextTick()
  announcement.value = text
}

/** Focuses the first enabled button of `actions` on a tile (`[data-tile]`) or a hidden one (`[data-available]`). */
async function focusOn(attr: 'data-tile' | 'data-available', id: string, actions: string[]) {
  await nextTick()
  const item = root.value?.querySelector(`[${attr}="${id}"]`)
  for (const action of actions) {
    const button = item?.querySelector<HTMLButtonElement>(`[data-action="${action}"]`)
    if (button && !button.disabled) return button.focus()
  }
}

/** Saves `next`; `focus` runs once it renders, then `done` is announced, or `undo` puts focus back on failure. */
async function apply(next: string[] | null, focus: () => unknown, done: () => string, undo: () => unknown) {
  const saving = props.save(next)
  // Ignored while another save runs, so saves never overlap.
  if (!saving) return
  focus()
  if (await saving) await announce(done())
  else undo()
}

const moved = (id: string) => () =>
  t('arrange.moved', { name: props.name(id), position: tilePosition(arranged.value, id) })

function move(id: string, step: -1 | 1) {
  const actions = step < 0 ? ['earlier', 'later'] : ['later', 'earlier']
  const refocus = () => focusOn('data-tile', id, actions)
  apply(moveTile(arranged.value, id, step), refocus, moved(id), refocus)
}

function remove(id: string) {
  const order = arranged.value.map((tile) => tile.id)
  const at = order.indexOf(id)
  const neighbour = order[at + 1] ?? order[at - 1]
  apply(
    order.filter((other) => other !== id),
    async () => {
      if (neighbour) return focusOn('data-tile', neighbour, ['remove'])
      // The hidden tiles section may only render now that the tile is in it.
      await nextTick()
      root.value?.querySelector<HTMLElement>('#available-heading')?.focus()
    },
    () => t('arrange.removed', { name: props.name(id) }),
    () => focusOn('data-tile', id, ['remove']),
  )
}

function add(id: string) {
  apply(
    [...arranged.value.map((tile) => tile.id), id],
    () => focusOn('data-tile', id, ['earlier', 'later', 'remove']),
    () => t('arrange.added', { name: props.name(id), position: tilePosition(arranged.value, id) }),
    () => focusOn('data-available', id, ['add']),
  )
}

const restore = () =>
  apply(
    null,
    () => {},
    () => t('arrange.restored'),
    () => {},
  )

function onDragstart(e: DragEvent, id: string) {
  dragged.value = id
  e.dataTransfer?.setData('text/plain', id)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onDragover(e: DragEvent, target: string) {
  // Preventing the default is what allows the drop.
  if (!dragged.value || dragged.value === target) return
  e.preventDefault()
  over.value = target
}

function onDrop(target: string) {
  const id = dragged.value
  dragged.value = over.value = null
  if (!id || id === target) return
  apply(
    dropTile(arranged.value, id, target),
    () => {},
    moved(id),
    () => {},
  )
}
</script>

<style scoped>
.arrange {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.arrange h2 {
  margin-bottom: 0;
}

.plain {
  list-style: none;
  margin: 0;
  padding: 0;
}

.grid {
  display: grid;
  /* 12.5rem = 200px at the default font size. */
  grid-template-columns: repeat(auto-fill, minmax(min(12.5rem, 100%), 1fr));
  gap: 0.75rem;
}

/* A page narrower than 36rem (a window below about 600px): one column. */
@container view (max-width: 36rem) {
  .grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

/* Laid out like a home tile (HomeTile.vue), with a dashed border while home is edited. */
.item {
  position: relative;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-content: start;
  gap: 0.25rem 0.875rem;
  min-width: 0;
  padding: 1rem 2.5rem 1rem 1.125rem;
  border: 1px dashed var(--p-surface-400);
  border-radius: 6px;
  background: var(--p-surface-900);
  cursor: grab;
}

.item.over {
  border-color: var(--p-primary-color);
}

.hide {
  position: absolute;
  top: 0.25rem;
  right: 0.25rem;
}

.buttons {
  grid-column: 2;
  display: flex;
  gap: 0.5rem;
  margin-top: 0.25rem;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

#available-heading:focus {
  outline: none;
}
</style>
