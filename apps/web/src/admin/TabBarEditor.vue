<template>
  <!-- Edit the tab bar: which groups are pinned and in what order (the rest go into "More"), top or bottom. -->
  <dialog ref="dialog" class="sheet editor" aria-labelledby="tab-editor-title" @close="error = ''">
    <h2 id="tab-editor-title">{{ t('nav.editor.title') }}</h2>
    <p v-if="error" role="alert">{{ error }}</p>
    <fieldset>
      <legend>{{ t('nav.editor.pinned') }}</legend>
      <!-- Drag a row by its grip (or anywhere on it); the arrow buttons do the same from the keyboard. -->
      <ul class="rows">
        <li
          v-for="(row, i) in draft"
          :key="row.id"
          :data-row="row.id"
          draggable="true"
          :class="{ moved: row.id === moved, dragging: row.id === dragged, [`drop-${dropSide}`]: row.id === over }"
          @dragstart="onDragstart($event, row.id)"
          @dragover="onDragover($event, row.id)"
          @dragleave="over = null"
          @drop="onDrop($event, row.id)"
          @dragend="dragged = over = null"
        >
          <i class="pi pi-bars grip" aria-hidden="true"></i>
          <Checkbox v-model="row.pinned" :input-id="`pin-${row.id}`" binary />
          <label :for="`pin-${row.id}`">{{ t(`nav.groups.${row.id}`) }}</label>
          <Button
            icon="pi pi-arrow-up"
            text
            rounded
            size="small"
            data-dir="up"
            :disabled="i === 0"
            :aria-label="`${t('arrange.earlier')}：${t(`nav.groups.${row.id}`)}`"
            @click="move(i, -1)"
          />
          <Button
            icon="pi pi-arrow-down"
            text
            rounded
            size="small"
            data-dir="down"
            :disabled="i === draft.length - 1"
            :aria-label="`${t('arrange.later')}：${t(`nav.groups.${row.id}`)}`"
            @click="move(i, 1)"
          />
        </li>
      </ul>
      <p class="sr-only" aria-live="polite">{{ announce }}</p>
    </fieldset>
    <fieldset class="position">
      <legend>{{ t('nav.editor.position') }}</legend>
      <span v-for="p in POSITIONS" :key="p" class="choice">
        <RadioButton v-model="position" :input-id="`pos-${p}`" :value="p" name="tab-position" />
        <label :for="`pos-${p}`">{{ t(`nav.editor.${p}`) }}</label>
      </span>
    </fieldset>
    <p class="note">{{ t('nav.editor.phoneNote') }}</p>
    <div class="buttons sheet-actions">
      <Button :label="t('nav.editor.save')" :disabled="busy" @click="save(arrangement())" />
      <Button :label="t('nav.editor.restore')" outlined :disabled="busy" @click="save(null)" />
      <Button :label="t('nav.editor.cancel')" text @click="dialog?.close()" />
    </div>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import RadioButton from 'primevue/radiobutton'
import { nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas } from './api'
import { saveTabBar, useTabBar } from './tabbar'

const POSITIONS = ['top', 'bottom'] as const
const props = defineProps<{ groups: string[] }>()
const { t } = useI18n()
const bar = useTabBar()

const dialog = ref<HTMLDialogElement>()
const draft = ref<{ id: string; pinned: boolean }[]>([])
const position = ref<Schemas['TabBarPosition']>('top')
const busy = ref(false)
const error = ref('')
let opener: HTMLElement | null = null

/** Opens with the current arrangement: pinned groups first in their order, then the others. */
function open() {
  const pinned = bar.groups ?? props.groups
  const known = pinned.filter((id) => props.groups.includes(id))
  draft.value = [...known.map((id) => ({ id, pinned: true })), ...props.groups.filter((id) => !known.includes(id)).map((id) => ({ id, pinned: false }))]
  position.value = bar.position
  opener = document.activeElement as HTMLElement | null
  dialog.value?.showModal()
}
defineExpose({ open })

const moved = ref<string | null>(null)
const announce = ref('')
const dragged = ref<string | null>(null)
const over = ref<string | null>(null)
const dropSide = ref<'before' | 'after'>('before')
let movedTimer: ReturnType<typeof setTimeout> | undefined

/** Puts the row at `to`, flashes it and says where it went, so it is not lost after the move. */
function place(id: string, to: number, focusDir?: 'up' | 'down') {
  const rows = draft.value.filter((r) => r.id !== id)
  rows.splice(to, 0, draft.value.find((r) => r.id === id)!)
  draft.value = rows
  moved.value = id
  clearTimeout(movedTimer)
  movedTimer = setTimeout(() => (moved.value = null), 1600)
  announce.value = t('arrange.moved', { name: t(`nav.groups.${id}`), position: to + 1 })
  // The row's buttons move with it; keep focus on the one pressed, or its pair once it hits an end.
  if (focusDir)
    void nextTick(() => {
      const li = dialog.value?.querySelector(`[data-row="${id}"]`)
      const want = li?.querySelector<HTMLButtonElement>(`[data-dir="${focusDir}"]`)
      ;(want && !want.disabled ? want : li?.querySelector<HTMLButtonElement>(`[data-dir="${focusDir === 'up' ? 'down' : 'up'}"]`))?.focus()
    })
}

function move(i: number, step: -1 | 1) {
  place(draft.value[i].id, i + step, step < 0 ? 'up' : 'down')
}

function onDragstart(e: DragEvent, id: string) {
  dragged.value = id
  e.dataTransfer?.setData('text/plain', id)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onDragover(e: DragEvent, id: string) {
  if (!dragged.value || dragged.value === id) return
  e.preventDefault()
  const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dropSide.value = e.clientY < box.top + box.height / 2 ? 'before' : 'after'
  over.value = id
}

function onDrop(e: DragEvent, id: string) {
  const from = dragged.value
  dragged.value = over.value = null
  if (!from || from === id) return
  e.preventDefault()
  const rest = draft.value.filter((r) => r.id !== from)
  const at = rest.findIndex((r) => r.id === id) + (dropSide.value === 'after' ? 1 : 0)
  place(from, at)
}

// Groups hidden right now (their features are off) keep their saved place after the shown ones.
const arrangement = (): Schemas['TabBar'] => ({
  groups: [
    ...draft.value.filter((r) => r.pinned).map((r) => r.id),
    ...(bar.groups ?? []).filter((id) => !props.groups.includes(id)),
  ],
  position: position.value,
})

async function save(next: Schemas['TabBar'] | null) {
  busy.value = true
  error.value = ''
  try {
    await saveTabBar(next)
    dialog.value?.close()
    opener?.focus()
  } catch (e) {
    error.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
.editor {
  max-width: min(28rem, calc(100vw - 2rem));
  border: 1px solid var(--p-content-border-color);
  border-radius: var(--p-content-border-radius);
  background: var(--p-content-background);
  color: var(--p-text-color);
}

.editor[open] {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.editor::backdrop {
  background: rgb(0 0 0 / 0.6);
}

h2,
p {
  margin: 0;
}

fieldset {
  margin: 0;
  padding: 0;
  border: 0;
}

legend {
  margin-bottom: 0.25rem;
  font-weight: 600;
}

.rows {
  margin: 0;
  padding: 0;
  list-style: none;
}

.rows li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding-inline: 0.25rem;
  border-radius: var(--p-content-border-radius);
  cursor: grab;
}

.grip {
  color: var(--p-text-muted-color);
}

.rows li.dragging {
  opacity: 0.4;
}

.rows li.drop-before {
  box-shadow: inset 0 2px 0 var(--p-primary-color);
}

.rows li.drop-after {
  box-shadow: inset 0 -2px 0 var(--p-primary-color);
}

.rows li.moved {
  animation: moved 1.6s ease-out;
}

@keyframes moved {
  from {
    background: color-mix(in srgb, var(--p-primary-color) 25%, transparent);
  }
}

@media (prefers-reduced-motion: reduce) {
  .rows li.moved {
    animation: none;
    outline: 1px solid var(--p-primary-color);
  }
}

.rows label {
  margin-right: auto;
}

.position {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
}

.position legend {
  flex-basis: 100%;
}

.choice {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
