<template>
  <!-- The layout of every full page (usage in page.ts). -->
  <main ref="root">
    <header class="page-header">
      <Button :label="t('app.home')" icon="pi pi-arrow-left" text @click="goHome(router)" />
      <span v-if="look" class="badge" :class="`tint-${look.tint}`" aria-hidden="true"><i :class="look.icon"></i></span>
      <h1>{{ title }}</h1>
      <!-- This page's state (e.g. the scene on air) and its overlay's URL, at the right end. -->
      <div v-if="status || overlay" class="page-end">
        <span v-if="status" class="status">{{ status }}</span>
        <Button v-if="overlay" :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl(overlay)" />
      </div>
    </header>
    <!-- Third level: the other pages of this page's category, when it has more than one. -->
    <nav v-if="pageTabs.length > 1" class="page-tabs" :aria-label="t(`nav.categories.${category!.id}`)">
      <template v-for="p in pageTabs" :key="p.path">
        <span v-if="p.soon" class="off" aria-disabled="true" :title="t('home.unbuilt')">
          {{ t(p.tabLabel ?? p.label) }}<span class="sr-only">{{ t('home.unbuilt') }}</span>
        </span>
        <RouterLink v-else :to="p.path">{{ t(p.tabLabel ?? p.label) }}</RouterLink>
      </template>
    </nav>
    <!-- Sticky, so a failed setting further down the page still shows its error. -->
    <p v-if="error" role="alert" class="page-error">{{ error }}</p>
    <slot />
  </main>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { useToast } from 'primevue/usetoast'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useDrawer } from './drawer'
import { useModules } from './features'
import { TILES, errorToast, successToast } from './home'
import { goHome } from './page'
import { categoryOf } from './tabs'

const props = defineProps<{
  title: string
  /** The home tile this page belongs to, for its colored icon. */
  tile?: string
  /** What this page shows right now, e.g. 目前場景：聊天; nothing on pages without one. */
  status?: string
  /** The page's one overlay (`/overlay/<overlay>/`), whose URL 「複製 OBS 網址」 copies; a page with several puts
   * each button by its own overlay instead. */
  overlay?: string
}>()

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const { error } = useDrawer()
const root = ref<HTMLElement>()
// Only a page with an overlay copies; the others need no toast service.
const toast = props.overlay ? useToast() : undefined
const look = computed(() => TILES.find((tile) => tile.id === props.tile))
const { hidden } = useModules()
const category = computed(() => categoryOf(route.path))
const pageTabs = computed(() => category.value?.pages.filter((p) => !hidden.value.has(p.path)) ?? [])

async function copyObsUrl(overlay: string) {
  try {
    // The admin window loads the Host URL, so its origin is what OBS needs.
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/${overlay}/`)
    toast?.add(successToast(t('chat.copied')))
  } catch {
    toast?.add(errorToast(t('chat.copyFailed')))
  }
}

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

.page-tabs {
  display: flex;
  gap: 0.25rem;
  margin-top: 0.75rem;
  overflow-x: auto;
  border-bottom: 1px solid var(--p-surface-800);
  scrollbar-width: none;
}

.page-tabs a,
.page-tabs .off {
  flex: none;
  margin-bottom: -1px;
  padding: 0.5rem 0.875rem;
  border-bottom: 2px solid transparent;
  color: var(--p-text-muted-color);
  white-space: nowrap;
  text-decoration: none;
}

.page-tabs a:hover {
  color: var(--p-text-color);
}

.page-tabs a[aria-current='page'] {
  border-bottom-color: var(--p-primary-color);
  color: var(--p-primary-color);
}

/* Not built yet: grey and inert (same contrast note as the app bar). */
.page-tabs .off {
  color: var(--p-surface-500);
  cursor: not-allowed;
}

.badge {
  display: grid;
  place-items: center;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 6px;
  font-size: 1.125rem;
}

.page-end {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
  margin-left: auto;
}

.status {
  color: var(--p-text-muted-color);
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
