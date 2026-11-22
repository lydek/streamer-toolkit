<template>
  <!-- The layout of every full page (usage in page.ts). -->
  <main ref="root">
    <header class="page-header">
      <Button :label="t('app.home')" icon="pi pi-arrow-left" text @click="goHome(router)" />
      <span v-if="look" class="badge" :class="`tint-${look.tint}`" aria-hidden="true"><i :class="look.icon"></i></span>
      <h1>{{ title }}</h1>
      <!-- This page's state (e.g. the scene on air) and where its overlay is in OBS, at the right end. -->
      <div v-if="status || overlay" class="page-end">
        <span v-if="status" class="status">{{ status }}</span>
        <ObsStatusLink v-if="overlay" :overlay="`/overlay/${overlay}/`" />
      </div>
    </header>
    <!-- Third level: the other pages of this page's category, when it has more than one. -->
    <nav v-if="pageTabs.length > 1" class="page-tabs" :aria-label="t(`nav.categories.${category!.id}`)">
      <template v-for="p in pageTabs" :key="p.path">
        <span v-if="p.soon" class="off" aria-disabled="true" :title="t('home.unbuilt')">
          {{ t(p.tabLabel ?? p.label) }}<span class="sr-only">{{ t('home.unbuilt') }}</span>
        </span>
        <RouterLink v-else :to="p.path" @click="navOpened.path = p.path">{{ t(p.tabLabel ?? p.label) }}</RouterLink>
      </template>
    </nav>
    <!-- Sticky, so a failed setting further down the page still shows its error. -->
    <p v-if="error" role="alert" class="page-error">{{ error }}</p>
    <slot />
  </main>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useDrawer } from './drawer'
import { useModules } from './features'
import { TILES } from './home'
import ObsStatusLink from './ObsStatusLink.vue'
import { goHome, navOpened } from './page'
import { categoryOf } from './tabs'

const props = defineProps<{
  title: string
  /** The home tile this page belongs to, for its colored icon. */
  tile?: string
  /** What this page shows right now, e.g. 目前場景：聊天; nothing on pages without one. */
  status?: string
  /** The page's one overlay (`/overlay/<overlay>/`), whose place in OBS the header shows; a page with several
   * puts each status by its own overlay instead. */
  overlay?: string
}>()

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const { error } = useDrawer()
const root = ref<HTMLElement>()
const look = computed(() => TILES.find((tile) => tile.id === props.tile))
const { hidden } = useModules()
const category = computed(() => categoryOf(route.path))
const pageTabs = computed(() => category.value?.pages.filter((p) => !hidden.value.has(p.path)) ?? [])

// A hash names a section to land on (it needs tabindex="-1"); a page opened from a category or page tab keeps the
// keyboard there (navOpened); otherwise "← Home" takes focus.
onMounted(() => {
  const target = route.hash ? document.getElementById(decodeURIComponent(route.hash.slice(1))) : null
  const fromNav = navOpened.path === route.path
  navOpened.path = ''
  const kept = fromNav && document.activeElement !== document.body && document.activeElement?.isConnected
  if (kept && !target) return
  const tab = fromNav ? root.value?.querySelector<HTMLElement>('.page-tabs [aria-current="page"]') : null
  ;(target ?? tab ?? root.value?.querySelector('button'))?.focus()
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

/* Positioned, so the sr-only text of a grey tab scrolls with the row instead of widening the page. */
.page-tabs {
  position: relative;
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
