<template>
  <!-- Edit the tab bar: which groups are pinned and in what order (the rest go into "More"), top or bottom. -->
  <dialog ref="dialog" class="sheet editor" aria-labelledby="tab-editor-title" @close="error = ''">
    <h2 id="tab-editor-title">{{ t('nav.editor.title') }}</h2>
    <p v-if="error" role="alert">{{ error }}</p>
    <fieldset>
      <legend>{{ t('nav.editor.pinned') }}</legend>
      <ul class="rows">
        <li v-for="(row, i) in draft" :key="row.id">
          <Checkbox v-model="row.pinned" :input-id="`pin-${row.id}`" binary />
          <label :for="`pin-${row.id}`">{{ t(`nav.groups.${row.id}`) }}</label>
          <Button
            icon="pi pi-arrow-up"
            text
            rounded
            size="small"
            :disabled="i === 0"
            :aria-label="`${t('arrange.earlier')}：${t(`nav.groups.${row.id}`)}`"
            @click="move(i, -1)"
          />
          <Button
            icon="pi pi-arrow-down"
            text
            rounded
            size="small"
            :disabled="i === draft.length - 1"
            :aria-label="`${t('arrange.later')}：${t(`nav.groups.${row.id}`)}`"
            @click="move(i, 1)"
          />
        </li>
      </ul>
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
import { ref } from 'vue'
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

function move(i: number, step: -1 | 1) {
  const rows = [...draft.value]
  ;[rows[i], rows[i + step]] = [rows[i + step], rows[i]]
  draft.value = rows
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
