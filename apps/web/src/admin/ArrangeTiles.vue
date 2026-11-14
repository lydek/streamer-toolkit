<template>
  <div ref="root" class="arrange">
    <p class="sr-only" aria-live="polite">{{ announcement }}</p>
    <div>
      <Button :label="t('arrange.restore')" icon="pi pi-replay" severity="secondary" outlined @click="restore" />
    </div>

    <template v-for="g in groups" :key="g.group">
      <h2 :id="`arrange-${g.group}`">{{ t(`home.groups.${g.group}`) }}</h2>
      <ul class="plain grid" :aria-labelledby="`arrange-${g.group}`">
        <li
          v-for="(tile, i) in g.tiles"
          :key="tile.id"
          class="item"
          :class="{ over: over === tile.id }"
          :data-tile="tile.id"
          draggable="true"
          @dragstart="onDragstart($event, tile.id)"
          @dragover="onDragover($event, tile)"
          @dragleave="over = null"
          @drop.prevent="onDrop(tile.id)"
          @dragend="dragged = over = null"
        >
          <span class="name"><i :class="tile.icon" aria-hidden="true"></i> {{ name(tile.id) }}</span>
          <span v-if="disabled(tile.id)" class="muted">{{ t('arrange.notEnabled') }}</span>
          <div class="buttons">
            <Button
              data-action="earlier"
              :label="t('arrange.earlier')"
              :aria-label="t('arrange.for', { action: t('arrange.earlier'), name: name(tile.id) })"
              icon="pi pi-arrow-left"
              severity="secondary"
              outlined
              size="small"
              :disabled="i === 0"
              @click="move(tile.id, -1)"
            />
            <Button
              data-action="later"
              :label="t('arrange.later')"
              :aria-label="t('arrange.for', { action: t('arrange.later'), name: name(tile.id) })"
              icon="pi pi-arrow-right"
              icon-pos="right"
              severity="secondary"
              outlined
              size="small"
              :disabled="i === g.tiles.length - 1"
              @click="move(tile.id, 1)"
            />
            <Button
              data-action="remove"
              :label="t('arrange.remove')"
              :aria-label="t('arrange.for', { action: t('arrange.remove'), name: name(tile.id) })"
              icon="pi pi-times"
              severity="secondary"
              outlined
              size="small"
              @click="remove(tile.id)"
            />
          </div>
        </li>
      </ul>
    </template>

    <section v-if="available.length" aria-labelledby="available-heading">
      <h2 id="available-heading" tabindex="-1">{{ t('arrange.available') }}</h2>
      <ul class="plain grid" aria-labelledby="available-heading">
        <li v-for="tile in available" :key="tile.id" class="item" :data-available="tile.id">
          <span class="name"><i :class="tile.icon" aria-hidden="true"></i> {{ name(tile.id) }}</span>
          <span v-if="disabled(tile.id)" class="muted">{{ t('arrange.notEnabled') }}</span>
          <div class="buttons">
            <Button
              data-action="add"
              :label="t('arrange.add')"
              :aria-label="t('arrange.for', { action: t('arrange.add'), name: name(tile.id) })"
              icon="pi pi-plus"
              severity="secondary"
              outlined
              size="small"
              @click="add(tile.id)"
            />
          </div>
        </li>
      </ul>
    </section>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import {
  type Tile,
  arrangedTiles,
  availableTiles,
  dropTile,
  groupPosition,
  groupTiles,
  moveTile,
} from './home'

/*
 * Arrange mode of the admin home: every action saves at once; a failed save puts the previous order back and
 * reports the error through `failed`. Results are announced in a polite live region.
 */

const saved = defineModel<string[] | null>({ required: true })
const props = defineProps<{ modules: Schemas['ModuleState'][]; name: (id: string) => string }>()
const emit = defineEmits<{ failed: [message: string] }>()

const { t } = useI18n()
const root = ref<HTMLElement>()
const announcement = ref('')
const dragged = ref<string | null>(null)
const over = ref<string | null>(null)
let saving = false

const arranged = computed(() => arrangedTiles(saved.value))
const groups = computed(() => groupTiles(arranged.value))
const available = computed(() => availableTiles(arranged.value))
const disabled = (id: string) => props.modules.some((m) => m.id === id && !m.enabled)
const groupName = (id: string) => t(`home.groups.${arranged.value.find((tile) => tile.id === id)?.group}`)

async function announce(text: string) {
  // Cleared first, so the same text twice in a row is still announced.
  announcement.value = ''
  await nextTick()
  announcement.value = text
}

/** Focuses the first enabled button of `actions` on a tile (`[data-tile]`) or an "Available" item. */
async function focusOn(attr: 'data-tile' | 'data-available', id: string, actions: string[]) {
  await nextTick()
  const item = root.value?.querySelector(`[${attr}="${id}"]`)
  for (const action of actions) {
    const button = item?.querySelector<HTMLButtonElement>(`[data-action="${action}"]`)
    if (button && !button.disabled) return button.focus()
  }
}

/**
 * Shows `next` at once (`focus` runs after it renders), then saves it. Clicks during a save are ignored, so
 * saves never overlap. On failure the previous order returns and `undo` puts focus back.
 */
async function save(next: string[] | null, focus: () => unknown, done: () => string, undo: () => unknown) {
  if (saving) return
  saving = true
  const previous = saved.value
  saved.value = next
  focus()
  try {
    await api(client.PUT('/api/admin/home-tiles', { body: next }))
    await announce(done())
  } catch (e) {
    saved.value = previous
    undo()
    emit('failed', e instanceof ApiError ? errorMessage(e.code) : String(e))
  } finally {
    saving = false
  }
}

const moved = (id: string) => () =>
  t('arrange.moved', { name: props.name(id), group: groupName(id), position: groupPosition(arranged.value, id) })

function move(id: string, step: -1 | 1) {
  const actions = step < 0 ? ['earlier', 'later'] : ['later', 'earlier']
  const refocus = () => focusOn('data-tile', id, actions)
  save(moveTile(arranged.value, id, step), refocus, moved(id), refocus)
}

function remove(id: string) {
  const order = groups.value.flatMap((g) => g.tiles.map((tile) => tile.id))
  const at = order.indexOf(id)
  const neighbour = order[at + 1] ?? order[at - 1]
  save(
    order.filter((other) => other !== id),
    async () => {
      if (neighbour) await focusOn('data-tile', neighbour, ['remove'])
      else root.value?.querySelector<HTMLElement>('#available-heading')?.focus()
    },
    () => t('arrange.removed', { name: props.name(id) }),
    () => focusOn('data-tile', id, ['remove']),
  )
}

function add(id: string) {
  save(
    [...arranged.value.map((tile) => tile.id), id],
    () => focusOn('data-tile', id, ['earlier', 'later', 'remove']),
    () => t('arrange.added', { name: props.name(id), group: groupName(id), position: groupPosition(arranged.value, id) }),
    () => focusOn('data-available', id, ['add']),
  )
}

const restore = () =>
  save(
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

function onDragover(e: DragEvent, target: Tile) {
  const from = arranged.value.find((tile) => tile.id === dragged.value)
  // Within a group only; preventing the default is what allows the drop.
  if (!from || from.id === target.id || from.group !== target.group) return
  e.preventDefault()
  over.value = target.id
}

function onDrop(target: string) {
  const id = dragged.value
  dragged.value = over.value = null
  if (!id || id === target) return
  save(
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
  grid-template-columns: repeat(auto-fill, minmax(12.5rem, 1fr));
  gap: 0.75rem;
}

/* Below 600px: one column, buttons wrap. */
@media (max-width: 599.98px) {
  .grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.item {
  display: grid;
  gap: 0.5rem;
  min-width: 0;
  padding: 0.75rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.item[draggable='true'] {
  cursor: grab;
}

.item.over {
  border-style: dashed;
}

.name {
  font-weight: 600;
  overflow-wrap: anywhere;
}

.muted {
  color: var(--p-text-muted-color);
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

#available-heading:focus {
  outline: none;
}
</style>
