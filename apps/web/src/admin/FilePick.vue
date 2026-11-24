<template>
  <!-- Choosing files: a button, or with `dropText` a drop zone (one row: icon, text, button) that opens the picker
       anywhere it is clicked. The native input stays hidden (its English "Choose File" never shows) and is cleared
       after every pick, so a refused file can be picked again once fixed. -->
  <div
    v-if="dropText"
    class="drop"
    :class="{ over }"
    @click="open"
    @dragover.prevent="over = !disabled"
    @dragleave="left"
    @drop.prevent="dropped"
  >
    <i :class="icon" aria-hidden="true"></i>
    <div class="drop-text">
      <p :id="textId">{{ dropText }}</p>
      <p v-if="hint" :id="hintId" class="hint">{{ hint }}</p>
    </div>
    <!-- Its click reaches the zone, which opens the picker. -->
    <Button
      ref="button"
      :label="label ?? t('filePick.choose')"
      outlined
      :disabled="disabled"
      :aria-describedby="describedBy"
    />
  </div>
  <span v-else class="pick">
    <Button ref="button" :label="label" :icon="icon" outlined :disabled="disabled" :aria-describedby="describedBy" @click="open" />
    <span v-if="hint" :id="hintId" class="hint">{{ hint }}</span>
  </span>
  <input :id="inputId" ref="input" type="file" hidden :accept="accept" :multiple="multiple" :disabled="disabled" @change="changed" />
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(
  defineProps<{
    /** The button; a drop zone's defaults to 選擇檔案. */
    label?: string
    icon?: string
    /** What is accepted and how big, beside the button or under the drop zone's text. */
    hint?: string
    /** Makes it a drop zone with this text. */
    dropText?: string
    accept?: string
    multiple?: boolean
    disabled?: boolean
    /** The hidden input's ID. */
    inputId?: string
    /** An element naming what the file is for (e.g. its form row's label), read with the button. */
    labelledBy?: string
  }>(),
  { icon: 'pi pi-upload' },
)
const emit = defineEmits<{ pick: [files: File[]] }>()
const { t } = useI18n()
const input = ref<HTMLInputElement>()
const button = ref<{ $el: HTMLElement }>()
const over = ref(false)
const textId = useId()
const hintId = useId()
// The button is read with what the file is for, the zone's text and the limits.
const describedBy = computed(
  () =>
    [props.labelledBy, props.dropText && textId, props.hint && hintId].filter(Boolean).join(' ') || undefined,
)

const open = () => {
  if (!props.disabled) input.value?.click()
}

function changed() {
  const files = [...(input.value?.files ?? [])]
  if (input.value) input.value.value = ''
  if (files.length) emit('pick', files)
}

// Moving over the zone's own icon, text or button is still inside it.
function left(e: DragEvent) {
  if (!(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node | null)) over.value = false
}

// The page checks what it gets, as for a picked file, and says why it refuses one.
function dropped(e: DragEvent) {
  over.value = false
  if (props.disabled) return
  const files = [...(e.dataTransfer?.files ?? [])]
  if (files.length) emit('pick', props.multiple ? files : files.slice(0, 1))
}

/** The button, for a page to put focus back on. */
defineExpose({ element: (): HTMLElement | undefined => button.value?.$el })
</script>

<style scoped>
.pick {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.hint {
  margin: 0;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.drop {
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 1rem;
  width: 100%;
  padding: 1rem;
  border: 1px dashed var(--p-surface-500);
  border-radius: 6px;
  cursor: pointer;
}

.drop.over {
  border-color: var(--p-primary-color);
  background: color-mix(in srgb, var(--p-primary-color) 8%, transparent);
}

.drop > i {
  font-size: 1.5rem;
  color: var(--p-text-muted-color);
}

.drop-text {
  min-width: 0;
  margin-right: auto;
}

.drop-text p {
  margin: 0;
}

.drop-text p:first-child {
  color: var(--p-text-muted-color);
}

.drop :deep(.p-button) {
  flex: none;
  white-space: nowrap;
}

@media (max-width: 600px) {
  .drop {
    flex-direction: column;
    align-items: stretch;
    text-align: center;
  }

  .drop-text {
    margin-right: 0;
  }
}
</style>
