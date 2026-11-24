<template>
  <!-- A label-left form on a tinted panel (prototype `.card > .rows`): FormRow children, one setting per row. -->
  <section class="form-panel" :aria-labelledby="heading ? id : undefined">
    <slot name="head" />
    <h3 v-if="heading" :id="id" class="panel-head">{{ heading }}</h3>
    <div class="rows"><slot /></div>
    <div v-if="$slots.foot" class="panel-foot"><slot name="foot" /></div>
  </section>
</template>

<script setup lang="ts">
import { useId } from 'vue'

defineProps<{ heading?: string }>()
const id = useId()
</script>

<style scoped>
/* Its rows stack their label over the field when the panel itself is narrow (FormRow). */
.form-panel {
  container-type: inline-size;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.panel-head {
  margin: 0;
  padding: 0.75rem 1rem 0;
  font-size: 1.0625rem;
}

.rows {
  padding: 0 1rem;
}

.panel-foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
  padding: 0.75rem 1rem;
  border-top: 1px solid var(--p-surface-800);
}
</style>
