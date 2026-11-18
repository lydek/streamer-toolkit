<template>
  <!-- The layout of every full page (usage in page.ts). -->
  <main ref="root">
    <header class="page-header">
      <Button :label="t('app.home')" icon="pi pi-arrow-left" text @click="goHome(router)" />
      <h1>{{ title }}</h1>
    </header>
    <!-- Sticky, so a failed setting further down the page still shows its error. -->
    <p v-if="error" role="alert" class="page-error">{{ error }}</p>
    <slot />
  </main>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useDrawer } from './drawer'
import { goHome } from './page'

defineProps<{ title: string }>()

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const { error } = useDrawer()
const root = ref<HTMLElement>()

// A hash names a section to land on (it needs tabindex="-1"); otherwise "← Home" takes focus.
onMounted(() => {
  const target = route.hash ? document.getElementById(decodeURIComponent(route.hash.slice(1))) : null
  ;(target ?? root.value?.querySelector('button'))?.focus()
})
</script>

<style scoped>
.page-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.page-header h1 {
  margin: 0;
}

.page-error {
  position: sticky;
  top: var(--appbar-h, 0px);
  z-index: 1;
  padding: 0.5rem 0.75rem;
  border-radius: 6px;
  background: var(--p-surface-800);
}
</style>
