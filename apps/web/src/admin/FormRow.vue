<template>
  <!-- One row of a FormPanel: the label on the left, the field on the right, an optional hint under the field. -->
  <div class="form-row" :class="{ top, end }">
    <label v-if="inputId" :for="inputId" class="label">{{ label }}</label>
    <span v-else :id="labelId" class="label">{{ label }}</span>
    <div class="field" :class="{ end }"><slot /></div>
    <div v-if="$slots.hint" class="hint"><slot name="hint" /></div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  label: string
  /** The field's input, which the label names; without it the label is a span with `labelId` (for a group). */
  inputId?: string
  labelId?: string
  /** A field taller than one line: the label lines up with its top. */
  top?: boolean
  /** A switch: at the right end of the row. */
  end?: boolean
}>()
</script>

<style scoped>
.form-row {
  display: grid;
  grid-template-columns: minmax(8rem, 12rem) minmax(0, 1fr);
  gap: 0.25rem 1.5rem;
  align-items: center;
  padding: 0.625rem 0;
}

.form-row + .form-row {
  border-top: 1px solid var(--p-surface-800);
}

.form-row.top {
  align-items: start;
}

.form-row.top .label {
  padding-top: 0.375rem;
}

.field {
  min-width: 0;
}

.field.end {
  justify-self: end;
}

.hint {
  grid-column: 2;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

/* A narrow panel: the label goes over the field. */
@container (max-width: 34rem) {
  .form-row {
    grid-template-columns: minmax(0, 1fr);
  }

  .hint {
    grid-column: 1;
  }

  .form-row.top .label {
    padding-top: 0;
  }

  /* A switch is short: it stays beside its label. */
  .form-row.end {
    grid-template-columns: minmax(0, 1fr) auto;
  }
}

@media (max-height: 760px) {
  .form-row {
    padding: 0.5rem 0;
  }
}
</style>
