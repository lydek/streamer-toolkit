<template>
  <!-- Closes a home block back into its small tile (HomeView.vue `data-expand` puts focus on whichever is shown). -->
  <Button
    icon="pi pi-window-minimize"
    severity="secondary"
    text
    rounded
    size="small"
    class="block-close"
    aria-expanded="true"
    :aria-label="t('arrange.for', { action: t('home.collapse'), name })"
    :data-expand="expandId"
    @click="emit('close')"
  />
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useI18n } from 'vue-i18n'

defineProps<{ name: string; expandId?: string }>()
const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
</script>

<style scoped>
.block-close {
  margin-left: auto;
}
</style>
