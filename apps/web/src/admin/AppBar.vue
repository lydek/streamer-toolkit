<template>
  <!-- Sticky on every page (docs/decisions.md 管理 UI 版面); its height is --appbar-h for other sticky parts. -->
  <header ref="bar" class="app-bar" :class="{ scrolled }">
    <div class="bar-top">
      <span class="brand"><i class="pi pi-video" aria-hidden="true"></i>{{ t('app.name') }}</span>
      <div class="bar-status">
        <StatusLight
          name="Twitch"
          :light="twitch"
          v-bind="lightAttrs"
          @click="openConnections('twitch', $event.currentTarget)"
        />
        <StatusLight name="OBS" :light="obs" v-bind="lightAttrs" @click="openConnections('obs', $event.currentTarget)" />
        <Button
          icon="pi pi-cog"
          text
          rounded
          :aria-label="t('settings.title')"
          :aria-current="route.path === '/settings' ? 'page' : undefined"
          data-page-opener="settings"
          @click="router.push('/settings')"
        />
      </div>
    </div>
    <nav ref="tabsEl" class="tabs" :aria-label="t('nav.label')">
      <a
        v-for="(g, i) in GROUPS"
        :key="g.id"
        :href="router.resolve(target(g)).href"
        :hidden="i >= shown"
        :aria-current="groupCurrent(g)"
        data-group-tab
        @click.prevent="router.push(target(g))"
      >
        <i :class="g.icon" aria-hidden="true"></i>{{ t(`nav.groups.${g.id}`) }}
      </a>
      <button
        ref="moreEl"
        type="button"
        class="more"
        :class="{ current: currentInMore }"
        :hidden="!measuring && shown >= GROUPS.length"
        aria-haspopup="true"
        aria-controls="tabs-more"
        :aria-expanded="moreOpen"
        @click="moreMenu?.toggle($event)"
      >
        <i class="pi pi-ellipsis-h" aria-hidden="true"></i>{{ t('nav.more') }}
      </button>
      <Menu id="tabs-more" ref="moreMenu" :model="moreItems" popup @show="moreOpen = true" @hide="moreOpen = false" />
    </nav>
    <nav v-if="group && group.pages.length > 1" class="sub-tabs" :aria-label="t('nav.sub')">
      <RouterLink v-for="p in group.pages" :key="p.path" :to="p.path">{{ t(p.label) }}</RouterLink>
    </nav>
  </header>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { type Schemas, api, client } from './api'
import { drawerId, useDrawer } from './drawer'
import { obsLight, twitchLight, useLiveEvents } from './home'
import StatusLight from './StatusLight.vue'
import { GROUPS, type TabGroup, fittingTabs, groupOf } from './tabs'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const drawer = useDrawer()

// shortcut: home polls the same Twitch status and opens its own /ws/overlay for its tiles and drawers; share one
// source when more pages need these values.
const overview = ref<Schemas['Overview']>()
const overviewFailed = ref(false)
const live = useLiveEvents()
const twitch = computed(() => twitchLight(overview.value, overviewFailed.value))
const obs = computed(() => obsLight(live.value))

async function pollTwitch() {
  try {
    overview.value = await api(client.GET('/api/admin/twitch'))
    overviewFailed.value = false
  } catch {
    overviewFailed.value = true
  }
}

// On home the lights open the connections drawer; elsewhere they go to that section of settings.
const lightAttrs = computed(() =>
  route.path === '/'
    ? { 'aria-expanded': drawer.current.value === 'connections', 'aria-controls': drawerId('connections') }
    : {},
)
function openConnections(section: 'twitch' | 'obs', el: EventTarget | null) {
  if (route.path !== '/') void router.push({ path: '/settings', hash: `#${section}` })
  else if (drawer.current.value === 'connections') void drawer.close()
  else void drawer.open('connections', el)
}

// A group tab goes back to the sub-page last seen in that group.
const lastPage = reactive<Record<string, string>>({})
const group = computed(() => groupOf(route.path))
watch(
  () => route.path,
  (path) => {
    const g = groupOf(path)
    if (g) lastPage[g.id] = path
  },
  { immediate: true },
)
const target = (g: TabGroup) => lastPage[g.id] ?? g.pages[0].path
// A one-page group is the page itself; a group with sub-tabs is only the current group, its sub-tab is the page.
const groupCurrent = (g: TabGroup) => (g !== group.value ? undefined : g.pages.length === 1 ? 'page' : 'true')

// Tabs that do not fit go into "More"; with room for all of them there is no "More".
const tabsEl = ref<HTMLElement>()
const moreEl = ref<HTMLElement>()
const moreMenu = ref<InstanceType<typeof Menu>>()
const moreOpen = ref(false)
const shown = ref(GROUPS.length)
const measuring = ref(false)
const currentInMore = computed(() => !!group.value && GROUPS.indexOf(group.value) >= shown.value)
const moreItems = computed<MenuItem[]>(() =>
  GROUPS.slice(shown.value).map((g) => ({
    label: t(`nav.groups.${g.id}`),
    icon: g.icon,
    class: g === group.value ? 'current' : undefined,
    command: () => void router.push(target(g)),
  })),
)

async function fit() {
  measuring.value = true
  shown.value = GROUPS.length
  await nextTick()
  const nav = tabsEl.value
  if (nav && moreEl.value) {
    const tabs = [...nav.querySelectorAll<HTMLElement>('[data-group-tab]')]
    const gap = parseFloat(getComputedStyle(nav).columnGap) || 0
    shown.value = fittingTabs(
      tabs.map((el) => el.offsetWidth),
      gap,
      nav.clientWidth,
      moreEl.value.offsetWidth,
    )
  }
  measuring.value = false
}

const bar = ref<HTMLElement>()
const scrolled = ref(false)
const onScroll = () => (scrolled.value = window.scrollY > 0)
let timer: number | undefined
let barObserver: ResizeObserver | undefined
let tabsObserver: ResizeObserver | undefined

onMounted(() => {
  void pollTwitch()
  timer = window.setInterval(pollTwitch, 2000)
  window.addEventListener('scroll', onScroll, { passive: true })
  onScroll()
  barObserver = new ResizeObserver(() =>
    document.documentElement.style.setProperty('--appbar-h', `${bar.value?.offsetHeight ?? 0}px`),
  )
  if (bar.value) barObserver.observe(bar.value)
  tabsObserver = new ResizeObserver(() => {
    moreMenu.value?.hide()
    void fit()
  })
  if (tabsEl.value) tabsObserver.observe(tabsEl.value)
})
onBeforeUnmount(() => {
  window.clearInterval(timer)
  window.removeEventListener('scroll', onScroll)
  barObserver?.disconnect()
  tabsObserver?.disconnect()
  document.documentElement.style.removeProperty('--appbar-h')
})
</script>

<style scoped>
.app-bar {
  position: sticky;
  top: 0;
  z-index: 5;
  padding: 0.75rem 1rem 0;
  background: var(--p-surface-950);
  border-bottom: 1px solid var(--p-surface-800);
}

.app-bar.scrolled {
  box-shadow: 0 0.5rem 1rem -0.5rem rgb(0 0 0 / 0.5);
}

.bar-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
}

.brand {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  margin-right: auto;
  font-size: 1.125rem;
  font-weight: 600;
}

.brand i {
  color: var(--p-primary-color);
}

.bar-status {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

.tabs {
  display: flex;
  gap: 0.25rem;
  margin-top: 0.5rem;
  /* Until a resize has been measured, tabs that no longer fit must not widen the page; the margin keeps focus rings. */
  overflow: clip;
  overflow-clip-margin: 0.25rem;
}

.tabs > a,
.tabs > .more {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border: 0;
  border-bottom: 2px solid transparent;
  background: none;
  color: var(--p-text-muted-color);
  font: inherit;
  white-space: nowrap;
  text-decoration: none;
  cursor: pointer;
}

.tabs > a:hover,
.tabs > .more:hover {
  color: var(--p-text-color);
}

.tabs > a[aria-current],
.tabs > .more.current {
  border-bottom-color: var(--p-primary-color);
  color: var(--p-text-color);
}

.tabs > [hidden] {
  display: none;
}

.sub-tabs {
  display: flex;
  gap: 0.375rem;
  padding: 0.625rem 0;
  overflow-x: auto;
}

.sub-tabs a {
  flex: none;
  padding: 0.25rem 0.75rem;
  border: 1px solid var(--p-surface-600);
  border-radius: 999px;
  color: var(--p-text-muted-color);
  white-space: nowrap;
  text-decoration: none;
}

.sub-tabs a:hover {
  color: var(--p-text-color);
}

.sub-tabs a[aria-current='page'] {
  border-color: var(--p-primary-color);
  color: var(--p-primary-color);
}
</style>
